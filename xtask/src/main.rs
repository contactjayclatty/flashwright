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
    let license = fs::read_to_string(root.join("LICENSE")).map_err(|err| err.to_string())?;
    if !license.contains("GNU AFFERO GENERAL PUBLIC LICENSE") {
        return Err("LICENSE is missing the GNU AGPL heading".into());
    }
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
