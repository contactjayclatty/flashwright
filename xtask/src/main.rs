// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Static checks for the device layer.
//!
//! One process spawn site, no shell program names in product source, and the
//! AGPL heading in LICENSE.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ExprCall, Meta};

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
    let mut files = Vec::new();
    walk(&root.join("crates"), &mut files)?;
    walk(&root.join("xtask"), &mut files)?;
    lint_spawn(root, &files)?;
    lint_shell_names(root, &files)?;
    lint_window(root)?;
    let license = fs::read_to_string(root.join("LICENSE")).map_err(|err| err.to_string())?;
    if !license.contains("GNU AFFERO GENERAL PUBLIC LICENSE") {
        return Err("LICENSE is missing the GNU AGPL heading".into());
    }
    check_update_metadata(root)?;
    Ok(())
}

fn lint_spawn(root: &Path, files: &[PathBuf]) -> Result<(), String> {
    let mut allows = Vec::new();
    let mut calls = Vec::new();
    for path in files {
        let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
        let parsed =
            syn::parse_file(&text).map_err(|err| format!("{}: {err}", display(root, path)))?;
        let mut visitor = SpawnVisitor {
            path: path.clone(),
            allows: &mut allows,
            calls: &mut calls,
        };
        visitor.visit_file(&parsed);
    }
    if allows.len() != 1 {
        return Err(format!(
            "expected exactly one clippy::disallowed_methods allow, found {}",
            allows.len()
        ));
    }
    let allow_path = display(root, &allows[0]);
    if !allow_path
        .replace('\\', "/")
        .ends_with("flashwright-core/src/proc/spawn.rs")
    {
        return Err(format!("spawn allow is in {allow_path}"));
    }
    for call in &calls {
        let shown = display(root, call);
        if !shown
            .replace('\\', "/")
            .ends_with("flashwright-core/src/proc/spawn.rs")
        {
            return Err(format!("{shown} calls Command::new"));
        }
    }
    if calls.is_empty() {
        return Err("spawn.rs does not call Command::new".into());
    }
    Ok(())
}

struct SpawnVisitor<'a> {
    path: PathBuf,
    allows: &'a mut Vec<PathBuf>,
    calls: &'a mut Vec<PathBuf>,
}

impl Visit<'_> for SpawnVisitor<'_> {
    fn visit_attribute(&mut self, attr: &Attribute) {
        if allow_disallowed(attr) {
            self.allows.push(self.path.clone());
        }
        visit::visit_attribute(self, attr);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if is_command_new(expr) {
            self.calls.push(self.path.clone());
        }
        visit::visit_expr(self, expr);
    }
}

fn allow_disallowed(attr: &Attribute) -> bool {
    let Meta::List(list) = &attr.meta else {
        return false;
    };
    list.path.is_ident("allow") && list.tokens.to_string().contains("disallowed_methods")
}

fn is_command_new(expr: &Expr) -> bool {
    let Expr::Call(ExprCall { func, .. }) = expr else {
        return false;
    };
    let Expr::Path(path) = func.as_ref() else {
        return false;
    };
    let mut names = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string());
    let last = names.next_back();
    let previous = names.next_back();
    previous.as_deref() == Some("Command") && last.as_deref() == Some("new")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_passes_static_checks() {
        check(&workspace_root()).unwrap();
    }
}
