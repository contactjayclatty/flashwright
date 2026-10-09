// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Static checks for the device layer.
//!
//! One process spawn site, no shell program names in product source, and the
//! AGPL heading in LICENSE.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use std::collections::HashSet;

use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ExprCall, Item, Meta, Type, UseTree};

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
    let license = fs::read_to_string(root.join("LICENSE")).map_err(|err| err.to_string())?;
    if !license.contains("GNU AFFERO GENERAL PUBLIC LICENSE") {
        return Err("LICENSE is missing the GNU AGPL heading".into());
    }
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

fn lint_spawn(root: &Path, files: &[PathBuf]) -> Result<(), String> {
    let mut allows = Vec::new();
    let mut calls = Vec::new();
    for path in files {
        let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
        let parsed =
            syn::parse_file(&text).map_err(|err| format!("{}: {err}", display(root, path)))?;
        let mut aliases = HashSet::new();
        collect_aliases(&parsed, &mut aliases);
        let mut visitor = SpawnVisitor {
            path: path.clone(),
            aliases: &aliases,
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
    aliases: &'a HashSet<String>,
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
        if is_command_new(expr, self.aliases) {
            self.calls.push(self.path.clone());
        }
        visit::visit_expr(self, expr);
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

fn is_command_new(expr: &Expr, aliases: &HashSet<String>) -> bool {
    let Expr::Call(ExprCall { func, .. }) = expr else {
        return false;
    };
    let Expr::Path(path) = func.as_ref() else {
        return false;
    };
    let names: Vec<String> = path
        .path
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

fn collect_aliases(file: &syn::File, aliases: &mut HashSet<String>) {
    for item in &file.items {
        match item {
            Item::Use(item_use) => record_use(&item_use.tree, &[], aliases),
            Item::Type(item_type) if type_is_command(&item_type.ty) => {
                aliases.insert(item_type.ident.to_string());
            }
            _ => {}
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_passes_static_checks() {
        check(&workspace_root()).unwrap();
    }
}
