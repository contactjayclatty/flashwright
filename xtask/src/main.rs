// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Static checks for the device layer.
//!
//! Product source must not name a shell or a batch file. Write-class helpers
//! stay in the device crate until a plan crate exists.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match check(&workspace_root()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
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
    scan_sources(root)?;
    let license = fs::read_to_string(root.join("LICENSE")).map_err(|err| err.to_string())?;
    if !license.contains("GNU AFFERO GENERAL PUBLIC LICENSE") {
        return Err("LICENSE is missing the GNU AGPL heading".into());
    }
    Ok(())
}

fn scan_sources(root: &Path) -> Result<(), String> {
    let crates = root.join("crates");
    let mut files = Vec::new();
    walk(&crates, &mut files)?;
    for path in files {
        if skip_file(&path) {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let device_crate = relative.starts_with(Path::new("crates/flashwright-device"))
            || relative.starts_with(Path::new("crates/flashwright-plan"));
        for (number, line) in text.lines().enumerate() {
            if let Some(needle) = shell_needle(line) {
                return Err(format!(
                    "{}:{} contains shell needle {needle}",
                    relative.display(),
                    number + 1
                ));
            }
            if !device_crate && write_call(line) {
                return Err(format!(
                    "{}:{} calls a write-class helper outside the device crate",
                    relative.display(),
                    number + 1
                ));
            }
        }
    }
    Ok(())
}

fn skip_file(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.ends_with("forbid.rs") || text.contains("/tests/") || text.contains("\\tests\\")
}

fn shell_needle(line: &str) -> Option<&'static str> {
    const NEEDLES: &[&str] = &["cmd.exe", "cmd /c", "powershell", ".bat", "pwsh"];
    NEEDLES
        .iter()
        .copied()
        .find(|needle| contains_token(line, needle))
}

/// `.bat` is a file extension. It must not match a longer identifier such as `.battery`.
fn contains_token(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(index, _)| {
        let after = index + needle.len();
        !line[after..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

fn write_call(line: &str) -> bool {
    const UNIQUE: &[&str] = &[
        "WriteToken::mint",
        "fn fastboot_flash(",
        ".fastboot_flash(",
        "fn fastboot_set_active(",
        ".fastboot_set_active(",
        "fn fastboot_update(",
        ".fastboot_update(",
        "fn shell_write(",
        ".shell_write(",
        "fn sideload(",
        ".sideload(",
    ];
    if UNIQUE.iter().any(|needle| line.contains(needle)) {
        return true;
    }
    line.contains(".reboot(")
        || line.contains("fn reboot(")
        || (line.contains(".push(") && line.contains("token"))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_dir() {
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
