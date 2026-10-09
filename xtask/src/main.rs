// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Static checks for the device layer.
//!
//! One process spawn site, no shell program names in product source, and the
//! AGPL heading in LICENSE. Also: no Win32 or JavaScript process spawn, no
//! device codename in the product UI, a reuse log, no private paths, and a
//! devices table that matches the compatibility list.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use syn::visit::{self, Visit};
use syn::{
    Attribute, Expr, ExprCall, ImplItemFn, Item, ItemUse, Local, Meta, TraitItemFn, Type, UseTree,
};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check") | None => match check(&workspace_root()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        Some("lint-spawn") => match lint_spawn_command(&workspace_root()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        Some(other) => {
            eprintln!("unknown xtask command {other}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .to_path_buf()
}

fn check(root: &Path) -> Result<(), String> {
    let files = rust_files(root)?;
    lint_spawn(root, &files)?;
    lint_shell_names(root, &files)?;
    lint_window(root)?;
    lint_js_spawn(root)?;
    lint_codenames(root)?;
    lint_reuse_log(root)?;
    lint_private_data(root)?;
    lint_devices_schema(root)?;
    let license = fs::read_to_string(root.join("LICENSE")).map_err(|err| err.to_string())?;
    if !license.contains("GNU AFFERO GENERAL PUBLIC LICENSE") {
        return Err("LICENSE is missing the GNU AGPL heading".into());
    }
    check_update_metadata(root)?;
    Ok(())
}

fn lint_spawn_command(root: &Path) -> Result<(), String> {
    lint_spawn(root, &rust_files(root)?)
}

fn rust_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    walk(&root.join("crates"), &mut files)?;
    walk(&root.join("xtask"), &mut files)?;
    walk(&root.join("apps"), &mut files)?;
    Ok(files)
}

struct SpawnHits {
    allows: Vec<PathBuf>,
    calls: Vec<PathBuf>,
    hidden: Vec<PathBuf>,
    win32: Vec<String>,
    tokio_command: Vec<PathBuf>,
}

fn lint_spawn(root: &Path, files: &[PathBuf]) -> Result<(), String> {
    let mut hits = SpawnHits {
        allows: Vec::new(),
        calls: Vec::new(),
        hidden: Vec::new(),
        win32: Vec::new(),
        tokio_command: Vec::new(),
    };
    for path in files {
        let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
        collect_rust_spawn(path, &text, &mut hits)?;
    }
    judge_workspace_spawn(root, &hits)
}

fn collect_rust_spawn(path: &Path, text: &str, hits: &mut SpawnHits) -> Result<(), String> {
    let parsed = syn::parse_file(text).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut aliases = HashSet::new();
    let mut collector = AliasCollector {
        aliases: &mut aliases,
    };
    collector.visit_file(&parsed);
    let mut visitor = SpawnVisitor {
        path: path.to_path_buf(),
        aliases: &aliases,
        hits,
    };
    visitor.visit_file(&parsed);
    if !skip_text_needles(path) {
        for (number, line) in text.lines().enumerate() {
            if let Some(needle) = win32_needle(line) {
                hits.win32.push(format!(
                    "{}:{} contains {needle}",
                    path.display(),
                    number + 1
                ));
            }
        }
    }
    if !skip_tokio_command_scan(path) && text.contains(tokio_command_needle()) {
        hits.tokio_command.push(path.to_path_buf());
    }
    Ok(())
}

fn judge_workspace_spawn(root: &Path, hits: &SpawnHits) -> Result<(), String> {
    if let Some(hit) = hits.win32.first() {
        return Err(hit.clone());
    }
    if let Some(path) = hits.tokio_command.first() {
        return Err(format!(
            "{} names tokio process Command",
            display(root, path)
        ));
    }
    if let Some(path) = hits.hidden.first() {
        return Err(format!(
            "{} has an allow or expect next to Command",
            display(root, path)
        ));
    }
    if hits.allows.len() != 1 {
        return Err(format!(
            "expected exactly one clippy::disallowed_methods allow, found {}",
            hits.allows.len()
        ));
    }
    let allow_path = display(root, &hits.allows[0]);
    if !is_spawn_file(&hits.allows[0]) {
        return Err(format!("spawn allow is in {allow_path}"));
    }
    for call in &hits.calls {
        if !is_spawn_file(call) {
            return Err(format!("{} calls Command::new", display(root, call)));
        }
    }
    if hits.calls.is_empty() {
        return Err("spawn.rs does not call Command::new".into());
    }
    Ok(())
}

#[cfg(test)]
fn snippet_spawn_error(path: &str, source: &str) -> Result<(), String> {
    let mut hits = SpawnHits {
        allows: Vec::new(),
        calls: Vec::new(),
        hidden: Vec::new(),
        win32: Vec::new(),
        tokio_command: Vec::new(),
    };
    collect_rust_spawn(Path::new(path), source, &mut hits)?;
    if let Some(hit) = hits.win32.first() {
        return Err(hit.clone());
    }
    if hits.tokio_command.iter().any(|item| !is_spawn_file(item)) {
        return Err(format!("{path} names tokio process Command"));
    }
    if hits.hidden.iter().any(|item| !is_spawn_file(item))
        || hits.allows.iter().any(|item| !is_spawn_file(item))
    {
        return Err(format!("{path} has an allow or expect next to Command"));
    }
    if hits.calls.iter().any(|item| !is_spawn_file(item)) {
        return Err(format!("{path} calls Command::new"));
    }
    Ok(())
}

struct AliasCollector<'a> {
    aliases: &'a mut HashSet<String>,
}

impl Visit<'_> for AliasCollector<'_> {
    fn visit_item_use(&mut self, item: &ItemUse) {
        record_use(&item.tree, &[], self.aliases);
        visit::visit_item_use(self, item);
    }

    fn visit_item_type(&mut self, item: &syn::ItemType) {
        if type_is_command(&item.ty) {
            self.aliases.insert(item.ident.to_string());
        }
        visit::visit_item_type(self, item);
    }
}

struct SpawnVisitor<'a> {
    path: PathBuf,
    aliases: &'a HashSet<String>,
    hits: &'a mut SpawnHits,
}

impl Visit<'_> for SpawnVisitor<'_> {
    fn visit_item(&mut self, item: &Item) {
        let hit = mentions_command(self.aliases, |visitor| visitor.visit_item(item));
        self.consider(item_attrs(item), hit);
        visit::visit_item(self, item);
    }

    fn visit_impl_item_fn(&mut self, method: &ImplItemFn) {
        let hit = mentions_command(self.aliases, |visitor| visitor.visit_impl_item_fn(method));
        self.consider(&method.attrs, hit);
        visit::visit_impl_item_fn(self, method);
    }

    fn visit_trait_item_fn(&mut self, method: &TraitItemFn) {
        let hit = mentions_command(self.aliases, |visitor| visitor.visit_trait_item_fn(method));
        self.consider(&method.attrs, hit);
        visit::visit_trait_item_fn(self, method);
    }

    fn visit_local(&mut self, local: &Local) {
        if !local.attrs.is_empty() {
            let hit = mentions_command(self.aliases, |visitor| visitor.visit_local(local));
            self.consider(&local.attrs, hit);
        }
        visit::visit_local(self, local);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if is_command_new(expr, self.aliases) {
            self.hits.calls.push(self.path.clone());
        }
        visit::visit_expr(self, expr);
    }
}

impl SpawnVisitor<'_> {
    fn consider(&mut self, attrs: &[Attribute], mentions: bool) {
        for attr in attrs {
            if !is_allow_or_expect(attr) {
                continue;
            }
            let exempt = is_spawn_file(&self.path) && allow_disallowed(attr);
            if allow_disallowed(attr) {
                self.hits.allows.push(self.path.clone());
            }
            if mentions && !exempt {
                self.hits.hidden.push(self.path.clone());
            }
        }
    }
}

struct Mention<'a> {
    aliases: &'a HashSet<String>,
    hit: bool,
}

fn mentions_command(aliases: &HashSet<String>, visit: impl FnOnce(&mut Mention<'_>)) -> bool {
    let mut visitor = Mention {
        aliases,
        hit: false,
    };
    visit(&mut visitor);
    visitor.hit
}

impl Visit<'_> for Mention<'_> {
    fn visit_expr(&mut self, expr: &Expr) {
        if is_command_new(expr, self.aliases) {
            self.hit = true;
        }
        visit::visit_expr(self, expr);
    }

    fn visit_item_use(&mut self, item: &ItemUse) {
        if use_imports_command(&item.tree, &[]) {
            self.hit = true;
        }
        visit::visit_item_use(self, item);
    }

    fn visit_type(&mut self, ty: &Type) {
        if type_is_command(ty) {
            self.hit = true;
        }
        visit::visit_type(self, ty);
    }
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Fn(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        Item::Const(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        _ => &[],
    }
}

fn allow_disallowed(attr: &Attribute) -> bool {
    let Meta::List(list) = &attr.meta else {
        return false;
    };
    let name = list
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string());
    matches!(name.as_deref(), Some("allow") | Some("expect"))
        && list.tokens.to_string().contains("disallowed_methods")
}

fn is_allow_or_expect(attr: &Attribute) -> bool {
    let name = match &attr.meta {
        Meta::List(list) => list
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Meta::Path(path) => path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Meta::NameValue(_) => None,
    };
    matches!(name.as_deref(), Some("allow") | Some("expect"))
}

fn is_command_new(expr: &Expr, aliases: &HashSet<String>) -> bool {
    let path = match expr {
        Expr::Call(ExprCall { func, .. }) => {
            let Expr::Path(path) = func.as_ref() else {
                return false;
            };
            &path.path
        }
        Expr::Path(path) => &path.path,
        _ => return false,
    };
    let names: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    if names.last().map(String::as_str) != Some("new") || names.len() < 2 {
        return false;
    }
    let previous = &names[names.len() - 2];
    previous == "Command" || aliases.contains(previous)
}

fn record_use(tree: &UseTree, prefix: &[String], aliases: &mut HashSet<String>) {
    match tree {
        UseTree::Path(path) => {
            let mut next = prefix.to_vec();
            next.push(path.ident.to_string());
            record_use(&path.tree, &next, aliases);
        }
        UseTree::Name(name) => {
            if is_process_command(prefix) && name.ident == "Command" {
                aliases.insert("Command".into());
            }
        }
        UseTree::Rename(rename) => {
            if is_process_command(prefix) && rename.ident == "Command" {
                aliases.insert(rename.rename.to_string());
            }
        }
        UseTree::Group(group) => {
            for item in &group.items {
                record_use(item, prefix, aliases);
            }
        }
        UseTree::Glob(_) => {}
    }
}

fn use_imports_command(tree: &UseTree, prefix: &[String]) -> bool {
    match tree {
        UseTree::Path(path) => {
            let mut next = prefix.to_vec();
            next.push(path.ident.to_string());
            use_imports_command(&path.tree, &next)
        }
        UseTree::Name(name) => is_process_command(prefix) && name.ident == "Command",
        UseTree::Rename(rename) => is_process_command(prefix) && rename.ident == "Command",
        UseTree::Group(group) => group
            .items
            .iter()
            .any(|item| use_imports_command(item, prefix)),
        UseTree::Glob(_) => false,
    }
}

fn is_process_command(prefix: &[String]) -> bool {
    let joined = prefix.join("::");
    matches!(
        joined.as_str(),
        "std::process" | "tokio::process" | "tokio::process::command"
    )
}

fn type_is_command(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    let joined = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    matches!(
        joined.as_str(),
        "Command"
            | "std::process::Command"
            | "tokio::process::Command"
            | "tokio::process::command::Command"
    )
}

fn lint_shell_names(root: &Path, files: &[PathBuf]) -> Result<(), String> {
    for path in files {
        if skip_shell_scan(path) {
            continue;
        }
        let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
        let lower = text.to_ascii_lowercase();
        for (number, line) in lower.lines().enumerate() {
            if let Some(needle) = shell_needle(line) {
                return Err(format!(
                    "{}:{} contains shell needle {needle}",
                    display(root, path),
                    number + 1
                ));
            }
        }
    }
    Ok(())
}

fn skip_shell_scan(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    text.ends_with("proc/forbid.rs") || text.ends_with("xtask/src/main.rs")
}

fn shell_needle(line: &str) -> Option<&'static str> {
    const NEEDLES: &[&str] = &["cmd.exe", "cmd /c", "powershell", ".bat", ".cmd", "pwsh"];
    NEEDLES
        .iter()
        .copied()
        .find(|needle| contains_token(line, needle))
}

fn contains_token(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(index, _)| {
        let after = index + needle.len();
        !line[after..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

fn lint_window(root: &Path) -> Result<(), String> {
    let workspace = fs::read_to_string(root.join("Cargo.toml")).map_err(|err| err.to_string())?;
    if workspace.contains("flashwright-wizard") || root.join("crates/flashwright-wizard").exists() {
        return Err("the standalone wizard crate is still in the workspace".into());
    }
    let gui_manifest = root.join("apps/flashwright-gui/src-tauri/Cargo.toml");
    let gui = fs::read_to_string(&gui_manifest).map_err(|err| err.to_string())?;
    for banned in [
        "tauri-plugin-fs",
        "tauri-plugin-shell",
        "tauri-plugin-http",
        "tauri-plugin-process",
        "tauri-plugin-updater",
        "devtools",
    ] {
        if gui.contains(banned) {
            return Err(format!("the window shell must not depend on {banned}"));
        }
    }
    let config = fs::read_to_string(root.join("apps/flashwright-gui/src-tauri/tauri.conf.json"))
        .map_err(|err| err.to_string())?;
    if config.contains("devCsp") || config.contains("devtools") {
        return Err("the window config enables a dev content policy or devtools".into());
    }
    if !config.contains("\"create\": false") {
        return Err("the main window must be created by the shell".into());
    }
    if !config.contains("\"freezePrototype\": true") {
        return Err("the window config must freeze prototypes".into());
    }
    let capability_dir = root.join("apps/flashwright-gui/src-tauri/capabilities");
    let mut capabilities = Vec::new();
    for entry in fs::read_dir(&capability_dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            capabilities.push(path);
        }
    }
    if capabilities.len() != 1
        || capabilities[0].file_name().and_then(|name| name.to_str()) != Some("main-window.json")
    {
        return Err("the window must grant exactly capabilities/main-window.json".into());
    }
    let capability = fs::read_to_string(&capabilities[0]).map_err(|err| err.to_string())?;
    for banned in ["core:default", "dialog:", "opener:", "\"remote\""] {
        if capability.contains(banned) {
            return Err(format!("the window capability contains {banned}"));
        }
    }
    let commands_src =
        fs::read_to_string(root.join("apps/flashwright-gui/src-tauri/src/commands.rs"))
            .map_err(|err| err.to_string())?;
    let commands = phase1_commands(&commands_src)?;
    let shell = fs::read_to_string(root.join("apps/flashwright-gui/src-tauri/src/shell.rs"))
        .map_err(|err| err.to_string())?;
    let client = fs::read_to_string(root.join("apps/flashwright-gui/ui/src/ipc.ts"))
        .map_err(|err| err.to_string())?;
    for command in &commands {
        let allow = format!("\"allow-{}\"", command.replace('_', "-"));
        if !capability.contains(&allow) {
            return Err(format!("the window capability is missing {allow}"));
        }
        if !shell.contains(&format!("fn {command}(")) {
            return Err(format!("{command} is not a window handler"));
        }
        if !client.contains(&format!("\"{command}\"")) {
            return Err(format!("the window client does not call {command}"));
        }
    }
    Ok(())
}

fn phase1_commands(text: &str) -> Result<Vec<String>, String> {
    let start = text
        .find("PHASE1_COMMANDS")
        .ok_or("PHASE1_COMMANDS is missing")?;
    let slice = &text[start..];
    let marker = slice.find("= &[").ok_or("the command list is missing")?;
    let body_start = marker + "= &[".len();
    let body_end = slice[body_start..]
        .find(']')
        .ok_or("the command list is missing")?
        + body_start;
    let mut names = Vec::new();
    for token in slice[body_start..body_end].split(',') {
        let name = token.trim().trim_matches('"');
        if name.is_empty() {
            continue;
        }
        if !name.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_') {
            return Err(format!("unexpected command token {name}"));
        }
        names.push(name.to_string());
    }
    if names.is_empty() {
        return Err("PHASE1_COMMANDS is empty".into());
    }
    Ok(names)
}

fn display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("target") {
                continue;
            }
            walk(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn check_update_metadata(root: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};

    let proto_path = root.join("third_party/aosp/update_engine/update_metadata.proto");
    let bytes = fs::read(&proto_path).map_err(|err| err.to_string())?;
    let digest = Sha256::digest(&bytes);
    let encoded: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    const EXPECTED: &str = "09da1556e3edb9197ca88103b22ea07230a634c004605d4aa1efee6a6ed6e60d";
    if encoded != EXPECTED {
        return Err("update_metadata.proto does not match the recorded AOSP checksum".into());
    }
    let provenance =
        fs::read_to_string(root.join("third_party/aosp/update_engine/PROVENANCE.toml"))
            .map_err(|err| err.to_string())?;
    if !provenance.contains(EXPECTED)
        || !provenance.contains("dc84c2552b2d4cf00d2a843cb1c091d99d0499f1")
    {
        return Err("PROVENANCE.toml does not record the AOSP proto commit".into());
    }
    let dumper = fs::read_to_string(
        root.join("third_party/payload-dumper-rust/proto/update_metadata.proto"),
    )
    .map_err(|err| err.to_string())?;
    let aosp = String::from_utf8(bytes).map_err(|err| err.to_string())?;
    if !zstd_only_delta(&aosp, &dumper) {
        return Err(
            "payload-dumper proto differs from the AOSP proto by more than ZSTD = 14".into(),
        );
    }
    let license = fs::read_to_string(root.join("third_party/payload-dumper-rust/LICENSE"))
        .map_err(|err| err.to_string())?;
    if !license.contains("Apache License") {
        return Err("payload-dumper-rust LICENSE is missing the Apache heading".into());
    }
    if !root.join("third_party/aosp/update_engine/NOTICE").is_file() {
        return Err("AOSP update_engine NOTICE is missing".into());
    }
    if !root.join("third_party/aosp/avb/NOTICE").is_file() {
        return Err("AOSP avb NOTICE is missing".into());
    }
    Ok(())
}

fn zstd_only_delta(aosp: &str, dumper: &str) -> bool {
    let left = proto_tokens(aosp);
    let mut right = proto_tokens(dumper);
    let Some(at) = right.windows(4).position(|window| {
        window[0] == "ZSTD" && window[1] == "=" && window[2] == "14" && window[3] == ";"
    }) else {
        return false;
    };
    right.drain(at..at + 4);
    left == right
}

fn proto_tokens(input: &str) -> Vec<String> {
    let stripped = strip_proto_comments(input);
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in stripped.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            if !ch.is_whitespace() {
                tokens.push(ch.to_string());
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn strip_proto_comments(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '/' && chars.get(index + 1) == Some(&'/') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            continue;
        }
        if chars[index] == '"' {
            out.push('"');
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                out.push(chars[index]);
                if chars[index] == '\\' && index + 1 < chars.len() {
                    index += 1;
                    out.push(chars[index]);
                }
                index += 1;
            }
            if index < chars.len() {
                out.push('"');
                index += 1;
            }
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn is_spawn_file(path: &Path) -> bool {
    normalised(path).ends_with("flashwright-core/src/proc/spawn.rs")
}

fn normalised(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn skip_text_needles(path: &Path) -> bool {
    let text = normalised(path);
    text.ends_with("xtask/src/main.rs") || text.ends_with("proc/forbid.rs")
}

fn skip_tokio_command_scan(path: &Path) -> bool {
    skip_text_needles(path) || is_spawn_file(path)
}

fn tokio_command_needle() -> &'static str {
    "tokio::process::Command"
}

fn win32_needle(line: &str) -> Option<&'static str> {
    const NEEDLES: &[&str] = &[
        "CreateProcessAsUser",
        "CreateProcessW",
        "CreateProcessA",
        "CreateProcess",
        "ShellExecute",
        "WinExec",
    ];
    NEEDLES.iter().copied().find(|needle| line.contains(needle))
}

fn lint_js_spawn(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    walk_ext(
        &root.join("apps"),
        &["js", "mjs", "cjs", "ts", "tsx"],
        &mut files,
    )?;
    walk_ext(
        &root.join("crates"),
        &["js", "mjs", "cjs", "ts", "tsx"],
        &mut files,
    )?;
    for path in files {
        if js_spawn_allowed(&path) {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        if text.contains("child_process") {
            return Err(format!("{} imports child_process", display(root, &path)));
        }
    }
    Ok(())
}

fn js_spawn_allowed(path: &Path) -> bool {
    let text = normalised(path);
    text.ends_with("apps/flashwright-gui/src-tauri/build-ui.mjs")
        || text.ends_with("apps/flashwright-gui/ui/scripts/screenshots.mjs")
        || text.ends_with("apps/flashwright-gui/ui/scripts/keyboard.mjs")
}

#[cfg(test)]
fn js_snippet_error(path: &str, source: &str) -> Result<(), String> {
    if js_spawn_allowed(Path::new(path)) || !source.contains("child_process") {
        return Ok(());
    }
    Err(format!("{path} imports child_process"))
}

fn lint_codenames(root: &Path) -> Result<(), String> {
    let compat = fs::read_to_string(root.join("data/device_compatibility.toml"))
        .map_err(|err| err.to_string())?;
    let names = compatibility_codenames(&compat);
    let mut files = Vec::new();
    walk_ext(
        &root.join("apps/flashwright-gui/ui/src"),
        &["ts", "js", "mjs", "svelte"],
        &mut files,
    )?;
    for path in files {
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        if let Some(name) = first_codename_literal(&names, &text) {
            return Err(format!(
                "{} quotes device codename {name}",
                display(root, &path)
            ));
        }
    }
    Ok(())
}

fn compatibility_codenames(text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("codename") else {
            continue;
        };
        let rest = rest.trim().trim_start_matches('=').trim();
        if let Ok(name) = unquote(rest) {
            names.insert(name);
        }
    }
    names
}

fn first_codename_literal(names: &HashSet<String>, text: &str) -> Option<String> {
    quoted_literals(text)
        .into_iter()
        .find(|literal| names.contains(literal))
}

fn quoted_literals(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let quote = chars[index];
        if quote != '"' && quote != '\'' && quote != '`' {
            index += 1;
            continue;
        }
        index += 1;
        let start = index;
        while index < chars.len() && chars[index] != quote {
            if chars[index] == '\\' {
                index += 1;
            }
            index += 1;
        }
        if index < chars.len() {
            out.push(chars[start..index].iter().collect());
            index += 1;
        }
    }
    out
}

const REUSE_COMMIT: &str = "081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54";

fn lint_reuse_log(root: &Path) -> Result<(), String> {
    let disclaimer =
        fs::read_to_string(root.join("docs/disclaimer.md")).map_err(|err| err.to_string())?;
    let third =
        fs::read_to_string(root.join("docs/THIRD_PARTY.md")).map_err(|err| err.to_string())?;
    reuse_commit_present(&disclaimer, &third)?;
    named_data_files_exist(root, &disclaimer)
}

fn reuse_commit_present(disclaimer: &str, third: &str) -> Result<(), String> {
    if disclaimer.contains(REUSE_COMMIT) && third.contains(REUSE_COMMIT) {
        return Ok(());
    }
    Err(format!("the reuse log is missing commit {REUSE_COMMIT}"))
}

fn named_data_files_exist(root: &Path, disclaimer: &str) -> Result<(), String> {
    for name in data_toml_names(disclaimer) {
        if !root.join("data").join(&name).is_file() {
            return Err(format!("disclaimer names data/{name}, which is missing"));
        }
    }
    Ok(())
}

fn data_toml_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find("data/") {
        rest = &rest[index + "data/".len()..];
        let end = rest
            .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '.')
            .unwrap_or(rest.len());
        let name = &rest[..end];
        if name.ends_with(".toml") && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
        rest = &rest[end..];
    }
    names
}

fn lint_private_data(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for dir in ["crates", "apps", "xtask", "docs", "data"] {
        walk_source(&root.join(dir), &mut files)?;
    }
    for name in ["README.md", "CHANGELOG.md"] {
        let path = root.join(name);
        if path.is_file() {
            files.push(path);
        }
    }
    for path in files {
        if skip_text_needles(&path) {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        if let Some(err) = private_data_hit(&path, &text) {
            return Err(format!("{}: {err}", display(root, &path)));
        }
    }
    Ok(())
}

fn private_data_hit(path: &Path, text: &str) -> Option<String> {
    let shown = normalised(path);
    for (number, line) in text.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        if line_has_email(line) {
            return Some(format!("line {} has an email address", number + 1));
        }
        if lower.contains(r"c:\users\") || lower.contains("/users/") {
            return Some(format!("line {} has a home-directory path", number + 1));
        }
        if line.contains("google/") {
            let exempt = shown.contains("/tests/")
                || shown.ends_with("/tests.rs")
                || line.contains("TEST")
                || lower.contains("synthetic");
            if !exempt {
                return Some(format!("line {} has a google fingerprint", number + 1));
            }
        }
    }
    None
}

fn line_has_email(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '@' || index == 0 {
            continue;
        }
        let mut local = index;
        while local > 0 {
            let prev = chars[local - 1];
            if prev.is_ascii_alphanumeric() || matches!(prev, '.' | '_' | '%' | '+' | '-') {
                local -= 1;
            } else {
                break;
            }
        }
        if local == index {
            continue;
        }
        let mut domain_end = index + 1;
        while domain_end < chars.len() {
            let next = chars[domain_end];
            if next.is_ascii_alphanumeric() || next == '.' || next == '-' {
                domain_end += 1;
            } else {
                break;
            }
        }
        let domain: String = chars[index + 1..domain_end].iter().collect();
        if domain_is_email_tld(&domain) {
            return true;
        }
    }
    false
}

fn domain_is_email_tld(domain: &str) -> bool {
    let Some((host, tld)) = domain.rsplit_once('.') else {
        return false;
    };
    !host.is_empty()
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
        && host.chars().any(|ch| ch.is_ascii_alphanumeric())
        && matches!(tld, "com" | "org" | "net")
}

struct DeviceRow {
    codename: Option<String>,
    model: Option<String>,
    has_init_boot: Option<bool>,
}

fn lint_devices_schema(root: &Path) -> Result<(), String> {
    let text = fs::read_to_string(root.join("data/devices.toml")).map_err(|err| err.to_string())?;
    let rows = parse_device_table(&text)?;
    validate_device_rows(&rows)?;
    let komodo = rows
        .iter()
        .find(|row| row.codename.as_deref() == Some("komodo"))
        .ok_or("devices table is missing komodo")?;
    if komodo.model.as_deref() != Some("Pixel 9 Pro XL") || komodo.has_init_boot != Some(true) {
        return Err("komodo must be Pixel 9 Pro XL with init_boot".into());
    }
    let compat = fs::read_to_string(root.join("data/device_compatibility.toml"))
        .map_err(|err| err.to_string())?;
    let known = compatibility_codenames(&compat);
    for row in &rows {
        let Some(codename) = row.codename.as_deref() else {
            continue;
        };
        if !known.contains(codename) {
            return Err(format!(
                "devices table codename {codename} is not in the compatibility table"
            ));
        }
    }
    Ok(())
}

fn parse_device_table(text: &str) -> Result<Vec<DeviceRow>, String> {
    let mut rows = Vec::new();
    let mut current: Option<DeviceRow> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[device]]" {
            if let Some(row) = current.take() {
                rows.push(row);
            }
            current = Some(DeviceRow {
                codename: None,
                model: None,
                has_init_boot: None,
            });
            continue;
        }
        let Some(row) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("devices table has an unreadable line: {line}"));
        };
        match key.trim() {
            "codename" => row.codename = Some(unquote(value.trim())?),
            "model" => row.model = Some(unquote(value.trim())?),
            "has_init_boot" => row.has_init_boot = Some(parse_bool(value.trim())?),
            _ => {}
        }
    }
    if let Some(row) = current {
        rows.push(row);
    }
    Ok(rows)
}

fn validate_device_rows(rows: &[DeviceRow]) -> Result<(), String> {
    if rows.is_empty() {
        return Err("devices table has no rows".into());
    }
    for row in rows {
        let Some(codename) = row.codename.as_deref() else {
            return Err("a device row is missing codename".into());
        };
        if !codename_ok(codename) {
            return Err(format!(
                "codename {codename} is not a lowercase device name"
            ));
        }
        if row.model.as_deref().is_none_or(str::is_empty) {
            return Err(format!("device {codename} is missing model"));
        }
        if row.has_init_boot.is_none() {
            return Err(format!("device {codename} is missing has_init_boot"));
        }
    }
    Ok(())
}

fn codename_ok(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_lowercase())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn unquote(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches(',');
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return Ok(value[1..value.len() - 1].to_string());
    }
    Err(format!("expected a quoted string, got {value}"))
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.trim().trim_end_matches(',') {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("expected true or false, got {other}")),
    }
}

fn walk_source(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    walk_ext(
        dir,
        &[
            "rs", "md", "toml", "json", "ts", "tsx", "js", "mjs", "cjs", "svelte", "html", "yml",
            "yaml",
        ],
        out,
    )
}

fn walk_ext(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    let mut queue = VecDeque::from([dir.to_path_buf()]);
    while let Some(current) = queue.pop_front() {
        for entry in fs::read_dir(&current).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                if matches!(name, "target" | "node_modules" | "dist" | ".git") {
                    continue;
                }
                queue.push_back(path);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| exts.contains(&ext))
            {
                out.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
struct MutationFixture {
    name: &'static str,
    path: &'static str,
    source: &'static str,
    javascript: bool,
}

#[cfg(test)]
fn mutation_fixtures() -> Vec<MutationFixture> {
    vec![
        MutationFixture {
            name: "std process call",
            path: "crates/flashwright-core/src/bad.rs",
            source: "fn main() {\n    std::process::Command::new(\"x\");\n}\n",
            javascript: false,
        },
        MutationFixture {
            name: "tokio process call",
            path: "crates/flashwright-core/src/bad.rs",
            source: "fn main() {\n    tokio::process::Command::new(\"x\");\n}\n",
            javascript: false,
        },
        MutationFixture {
            name: "imported Command",
            path: "crates/flashwright-core/src/bad.rs",
            source: "use std::process::Command;\nfn main() {\n    Command::new(\"x\");\n}\n",
            javascript: false,
        },
        MutationFixture {
            name: "function-local alias under allow",
            path: "crates/flashwright-core/src/bad.rs",
            source: "#[allow(clippy::style)]\nfn sneak() {\n    use std::process::Command as Tool;\n    let _child = Tool::new(\"x\");\n}\n",
            javascript: false,
        },
        MutationFixture {
            name: "path to Command::new",
            path: "crates/flashwright-core/src/bad.rs",
            source: "fn main() {\n    let make = std::process::Command::new;\n    let _ = make;\n}\n",
            javascript: false,
        },
        MutationFixture {
            name: "allow disallowed_methods",
            path: "crates/flashwright-core/src/bad.rs",
            source: "#[allow(clippy::disallowed_methods)]\nfn sneak() {}\n",
            javascript: false,
        },
        MutationFixture {
            name: "expect disallowed_methods",
            path: "crates/flashwright-core/src/bad.rs",
            source: "#[expect(clippy::disallowed_methods)]\nfn sneak() {}\n",
            javascript: false,
        },
        MutationFixture {
            name: "allow on a function that spawns",
            path: "crates/flashwright-core/src/bad.rs",
            source: "#[allow(unused)]\nfn sneak() {\n    std::process::Command::new(\"x\");\n}\n",
            javascript: false,
        },
        MutationFixture {
            name: "Win32 CreateProcessW",
            path: "crates/flashwright-core/src/bad.rs",
            source: "fn main() {\n    CreateProcessW();\n}\n",
            javascript: false,
        },
        MutationFixture {
            name: "type alias of tokio Command",
            path: "crates/flashwright-core/src/bad.rs",
            source: "type Tool = tokio::process::Command;\nfn main() {\n    Tool::new(\"x\");\n}\n",
            javascript: false,
        },
        MutationFixture {
            name: "javascript child_process",
            path: "apps/flashwright-gui/ui/src/sneak.ts",
            source: "import { spawn } from \"node:child_process\";\nspawn(\"adb\");\n",
            javascript: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_passes_static_checks() {
        check(&workspace_root()).unwrap();
    }

    #[test]
    fn mutation_fixtures_are_refused() {
        let fixtures = mutation_fixtures();
        assert_eq!(fixtures.len(), 11);
        for fixture in fixtures {
            let err = if fixture.javascript {
                js_snippet_error(fixture.path, fixture.source)
            } else {
                snippet_spawn_error(fixture.path, fixture.source)
            };
            assert!(err.is_err(), "{} was accepted", fixture.name);
        }
    }

    #[test]
    fn a_ui_codename_literal_is_refused() {
        let names =
            compatibility_codenames("codename = \"shiba\"\nbootloader_codename = \"akita\"\n");
        assert!(first_codename_literal(&names, "const name = \"harbor\";").is_none());
        assert_eq!(
            first_codename_literal(&names, "const name = \"shiba\";").as_deref(),
            Some("shiba")
        );
    }

    #[test]
    fn a_devices_row_must_match_the_schema() {
        let bad =
            "[[device]]\ncodename = \"Komodo\"\nmodel = \"Pixel 9 Pro XL\"\nhas_init_boot = true\n";
        let rows = parse_device_table(bad).unwrap();
        assert!(validate_device_rows(&rows).is_err());
    }

    #[test]
    fn the_reuse_log_must_name_the_upstream_commit() {
        assert!(reuse_commit_present("no commit", "no commit").is_err());
        let text = format!("commit {REUSE_COMMIT}");
        assert!(reuse_commit_present(&text, &text).is_ok());
        assert!(named_data_files_exist(Path::new("/no/such"), "see data/not_real.toml").is_err());
    }

    #[test]
    fn private_data_is_refused() {
        let email = format!("{}@example.{}", "person", "com");
        assert!(private_data_hit(Path::new("src/lib.rs"), &email).is_some());
        assert!(private_data_hit(Path::new("src/lib.rs"), r"C:\Users\someone\phone").is_some());
        assert!(private_data_hit(Path::new("src/lib.rs"), "/Users/someone/phone").is_some());
        let fingerprint = "google/shiba/shiba:14/UD1A.231105.004/1:user/release-keys";
        assert!(private_data_hit(Path::new("src/lib.rs"), fingerprint).is_some());
        assert!(private_data_hit(
            Path::new("crates/flashwright-device/tests/t1_3_props.rs"),
            fingerprint
        )
        .is_none());
        assert!(private_data_hit(
            Path::new("crates/flashwright-firmware/src/tests.rs"),
            fingerprint
        )
        .is_none());
        assert!(private_data_hit(
            Path::new("src/bootimg.rs"),
            "google/komodo/komodo:17/TEST/1:user/release-keys"
        )
        .is_none());
        assert!(private_data_hit(Path::new("tauri.conf.json"), "icons/128x128@2x.png").is_none());
    }
}
