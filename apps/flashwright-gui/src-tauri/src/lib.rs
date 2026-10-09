// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Flashwright window for the Windows update wizard.
//!
//! The `gui` feature links the window. Workspace checks leave it off so they
//! do not need a WebView.

mod commands;

#[cfg(feature = "gui")]
mod shell;

#[cfg(feature = "gui")]
pub use shell::run;

#[cfg(not(feature = "gui"))]
pub fn run() {
    eprintln!("The Flashwright window is built with the gui feature.");
}

/// The command list the window is allowed to call.
pub fn phase1_commands() -> &'static [&'static str] {
    commands::PHASE1_COMMANDS
}

#[cfg(test)]
mod phase1 {
    use super::phase1_commands;

    #[test]
    fn capability_grants_only_the_phase1_commands() {
        let capability = include_str!("../capabilities/main-window.json");
        assert!(!capability.contains("core:default"));
        assert!(!capability.contains("dialog:"));
        assert!(!capability.contains("opener:"));
        assert!(!capability.contains("\"remote\""));
        for command in phase1_commands() {
            let allow = format!("\"allow-{}\"", command.replace('_', "-"));
            assert!(capability.contains(&allow), "{allow} missing");
        }
        let shell = include_str!("shell.rs");
        for command in phase1_commands() {
            assert!(
                shell.contains(&format!("fn {command}(")),
                "{command} is not a handler"
            );
        }
        for dropped in [
            "backup_verify",
            "backup_pin",
            "support_report",
            "engine_info",
            "pick_package",
            "open_firmware",
        ] {
            assert!(!shell.contains(&format!("fn {dropped}(")));
        }
    }
}
