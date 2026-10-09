// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::sync::Arc;

use flashwright_core::proc::{ScriptedResponse, ScriptedRunner};
use flashwright_core::{CoreError, Session};
use flashwright_tools::HostKind;

fn tool_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

#[tokio::test]
async fn blocked_tools_are_refused_before_a_scan() {
    let dir = std::env::temp_dir().join(format!("flashwright-session-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let adb = tool_name("adb");
    let fastboot = tool_name("fastboot");
    std::fs::write(dir.join(&adb), b"adb").unwrap();
    std::fs::write(dir.join(&fastboot), b"fastboot").unwrap();

    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        &adb,
        &["version"],
        ScriptedResponse::ok("Android Debug Bridge version 1.0.41\nVersion 34.0.4-10449696\n"),
    );
    runner.on(
        &fastboot,
        &["--version"],
        ScriptedResponse::ok("fastboot version 34.0.4-10449696\n"),
    );
    let mut session = Session::new(Arc::clone(&runner)).unwrap();
    let report = session
        .locate_tools(&dir, HostKind::current())
        .await
        .unwrap();
    assert!(!report.verdict.allows_scan());
    let err = session.scan().await.unwrap_err();
    assert!(matches!(err, CoreError::ToolsBlocked(_)));
    let calls = runner.calls();
    assert!(calls.iter().any(|call| call.args == ["version"]));
    assert!(calls.iter().any(|call| call.args == ["--version"]));
    assert_eq!(calls.len(), 2);
    assert!(!session.allows_writes());
    let _ = std::fs::remove_dir_all(&dir);
}
