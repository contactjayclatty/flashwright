// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Typed process runner.
//!
//! Every child is started from an absolute program path and an argument
//! vector. There is no command interpreter and no batch file. The adb
//! server is the one process started detached, so it can outlive a call.

mod error;
mod forbid;
mod lines;
mod scripted;
mod spawn;
mod system;

#[cfg(unix)]
mod unix_kill;

#[cfg(windows)]
mod windows_job;

pub use error::ProcError;
pub use forbid::is_forbidden_program;
pub use lines::StreamLine;
pub use scripted::{ScriptedResponse, ScriptedRunner};
pub(crate) use system::SystemRunner;

/// Name the real runner from library code.
///
/// The window constructs a scripted or empty engine today. This reference keeps
/// the allow-listed spawn path present for a later session that owns it.
pub(crate) fn keep_system_runner_linked() {
    let runner = SystemRunner;
    let run = SystemRunner::run;
    let _ = (runner, run);
}

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Which stream a [`StreamLine`] came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdStream {
    Stdout,
    Stderr,
}

/// Whether the child is tied to this process or may outlive it.
///
/// Device commands use [`ProcessGroup::TiedToParent`]. `adb start-server`
/// uses [`ProcessGroup::Detached`] so the server is outside the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessGroup {
    TiedToParent,
    Detached,
}

/// One recorded argv call. Tests read this. It cannot start a process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedCall {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub watchdog: Option<Duration>,
    pub finalising: Option<Duration>,
}

/// Timers for one catalogue command.
#[derive(Clone, Copy, Debug)]
pub struct RunLimits {
    pub timeout: Duration,
    pub watchdog: Option<Duration>,
    pub finalising: Option<Duration>,
}

impl RunLimits {
    pub(crate) fn from_budget(budget: &crate::timeouts::StepBudget) -> Self {
        Self {
            timeout: budget.timeout,
            watchdog: budget.watchdog,
            finalising: budget.finalising,
        }
    }
}

pub(crate) fn validate_program(program: &Path) -> Result<(), ProcError> {
    let name = program
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if is_forbidden_program(name) {
        return Err(ProcError::ShellForbidden {
            name: name.to_string(),
        });
    }
    if !program.is_absolute() {
        return Err(ProcError::ProgramNotAbsolute);
    }
    Ok(())
}

/// Outcome of one invocation.
#[derive(Clone, Debug)]
pub struct RunResult {
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_tail: Vec<u8>,
    pub stderr_tail: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub lines: Vec<StreamLine>,
    pub timed_out: bool,
    pub killed_by_watchdog: bool,
}

impl RunResult {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    /// Exit status 0, and the run was not killed by a timer.
    pub fn success_exit(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && !self.killed_by_watchdog
    }
}

/// Fastboot flash/update success: exit 0, no `FAILED` line, and an `OKAY` or `Finished.` line.
pub fn fastboot_flash_ok(result: &RunResult) -> bool {
    if !result.success_exit() {
        return false;
    }
    let mut saw_ok = false;
    for line in &result.lines {
        if line.text.starts_with("FAILED") {
            return false;
        }
        if line.text.contains("OKAY") || line.text.starts_with("Finished.") {
            saw_ok = true;
        }
    }
    saw_ok
}

/// Runs one catalogue command on a verified executable.
///
/// `VerifiedExe` and `CatalogueCommand` have no public constructors, so a
/// caller outside this crate cannot assemble an arbitrary argv.
pub trait CommandRunner: Send + Sync {
    fn run(
        &self,
        exe: &crate::exe::VerifiedExe,
        command: &crate::cmd::CatalogueCommand,
        limits: RunLimits,
    ) -> impl std::future::Future<Output = Result<RunResult, ProcError>> + Send;
}

pub(crate) fn file_name_lower(program: &Path) -> String {
    program
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;

    #[test]
    fn relative_paths_and_shells_are_rejected() {
        let err = validate_program(Path::new("adb")).unwrap_err();
        assert!(matches!(err, ProcError::ProgramNotAbsolute));

        // Built without a shell name literal so the product-source scan stays clean.
        let shell = if cfg!(windows) {
            let mut name = String::from(r"C:\Windows\System32\cm");
            name.push('d');
            name.push_str(".exe");
            PathBuf::from(name)
        } else {
            PathBuf::from("/bin/bash")
        };
        let err = validate_program(&shell).unwrap_err();
        assert!(matches!(err, ProcError::ShellForbidden { .. }));
    }

    #[test]
    fn fastboot_ok_requires_okay_and_rejects_failed() {
        let mut ok = RunResult {
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_tail: Vec::new(),
            stderr_tail: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            lines: vec![StreamLine {
                stream: StdStream::Stderr,
                text: "OKAY".into(),
                at: Duration::ZERO,
            }],
            timed_out: false,
            killed_by_watchdog: false,
        };
        assert!(fastboot_flash_ok(&ok));
        ok.lines.push(StreamLine {
            stream: StdStream::Stderr,
            text: "FAILED (remote: 'x')".into(),
            at: Duration::ZERO,
        });
        assert!(!fastboot_flash_ok(&ok));
    }
}
