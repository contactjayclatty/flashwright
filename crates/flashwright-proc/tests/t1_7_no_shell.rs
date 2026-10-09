// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.7 — the runner refuses a shell, and a real argv child still works.
//!
//! A ProcMon trace is Windows-only and is not collected in this environment.
//! The static half of T1.7 lives in `xtask`.

use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use flashwright_proc::{CommandRunner, Invocation, ProcError, ScriptedRunner, SystemRunner};

#[tokio::test]
async fn scripted_runner_rejects_a_shell() {
    let runner = ScriptedRunner::new();
    let err = runner
        .run(Invocation::tied(
            "/bin/bash",
            vec!["-c".into(), "echo hi".into()],
            Duration::from_secs(1),
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, ProcError::ShellForbidden { .. }));
    assert!(runner.calls().is_empty());
}

#[tokio::test]
#[cfg(unix)]
async fn system_runner_echo_timeout_and_watchdog() {
    let runner = SystemRunner;
    let echoed = runner
        .run(Invocation::tied(
            "/bin/echo",
            vec!["hello".into()],
            Duration::from_secs(5),
        ))
        .await
        .unwrap();
    assert!(echoed.success_exit());
    assert_eq!(echoed.stdout_text().trim(), "hello");

    let started = Instant::now();
    let timed = runner
        .run(Invocation::tied(
            "/bin/sleep",
            vec!["30".into()],
            Duration::from_millis(200),
        ))
        .await
        .unwrap();
    assert!(timed.timed_out);
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert!(started.elapsed() < Duration::from_secs(3));

    let started = Instant::now();
    let watched = runner
        .run(
            Invocation::tied("/bin/sleep", vec!["30".into()], Duration::from_secs(5))
                .with_watchdog(Duration::from_millis(150)),
        )
        .await
        .unwrap();
    assert!(watched.killed_by_watchdog);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
#[cfg(windows)]
async fn system_runner_where() {
    let root = std::env::var("SystemRoot").unwrap();
    let program = std::path::PathBuf::from(root)
        .join("System32")
        .join("where.exe");
    let runner = SystemRunner;
    let result = runner
        .run(Invocation::tied(
            program,
            vec!["where".into()],
            Duration::from_secs(10),
        ))
        .await
        .unwrap();
    assert!(result.success_exit());
}
