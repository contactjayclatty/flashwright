// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Reject shells and batch files before anything is spawned.
//!
//! The needles live in this module so the rest of the workspace can stay
//! free of those program names. `xtask` skips this file when it scans.

/// `true` when `file_name` is a command interpreter or a batch script.
pub fn is_forbidden_program(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(lower.as_str());
    matches!(
        stem,
        "cmd"
            | "command"
            | "powershell"
            | "pwsh"
            | "sh"
            | "bash"
            | "dash"
            | "zsh"
            | "fish"
            | "wscript"
            | "cscript"
            | "mshta"
            | "ksh"
            | "busybox"
            | "toybox"
            | "env"
            | "wsl"
    ) || lower.ends_with(".bat")
        || lower.ends_with(".cmd")
        || lower.ends_with(".ps1")
        || lower.ends_with(".vbs")
}

#[cfg(test)]
mod tests {
    use super::is_forbidden_program;

    #[test]
    fn flags_interpreters_and_batch_files() {
        for name in [
            "cmd",
            "cmd.exe",
            "CMD.EXE",
            "powershell",
            "powershell.exe",
            "pwsh",
            "pwsh.exe",
            "flash.bat",
            "FLASH.BAT",
            "tool.cmd",
            "sh",
            "bash",
        ] {
            assert!(is_forbidden_program(name), "{name} should be refused");
        }
    }

    #[test]
    fn allows_platform_tools() {
        for name in [
            "adb",
            "adb.exe",
            "fastboot",
            "fastboot.exe",
            "AdbWinApi.dll",
        ] {
            assert!(!is_forbidden_program(name), "{name} should be allowed");
        }
    }
}
