// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! The only place a process is started.
//!
//! Callers pass a [`crate::exe::VerifiedExe`]. The program path is that
//! value's absolute path.

use crate::exe::VerifiedExe;
use std::ffi::OsStr;
#[cfg(all(test, unix))]
use std::path::Path;

/// Build a child command. This is the single `Command::new` call site.
#[allow(clippy::disallowed_methods)]
pub(crate) fn command(program: &OsStr) -> tokio::process::Command {
    tokio::process::Command::new(program)
}

pub(crate) fn command_for(exe: &VerifiedExe) -> tokio::process::Command {
    debug_assert!(exe.path().is_absolute());
    command(exe.path().as_os_str())
}

/// Quote round-trip helper. Uses the same `command` function as production.
#[cfg(all(test, unix))]
pub(crate) fn command_path(program: &Path) -> tokio::process::Command {
    command(program.as_os_str())
}

#[cfg(all(test, unix))]
mod quote_tests {
    use super::command_path;

    #[tokio::test]
    async fn ten_thousand_strings_round_trip_through_sh() {
        let samples: Vec<String> = (0..10_000)
            .map(|index| {
                if index % 17 == 0 {
                    format!("a'b {index}")
                } else {
                    format!("tok{index}")
                }
            })
            .collect();
        for chunk in samples.chunks(250) {
            let script = chunk
                .iter()
                .map(|sample| format!("printf '%s\\n' {}", crate::cmd::sh_quote(sample)))
                .collect::<Vec<_>>()
                .join("; ");
            let mut child = command_path(std::path::Path::new("/bin/sh"));
            child.arg("-c").arg(script);
            let output = child.output().await.expect("sh");
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines, chunk);
        }
    }
}
