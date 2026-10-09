// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.7 — product code has one spawn site.
//!
//! The real child, the shell refusal, and the watchdog live in
//! `flashwright-core`. This crate no longer exposes a free-form runner.

use flashwright_proc::ScriptedRunner;

#[test]
fn scripted_runner_starts_with_no_calls() {
    let runner = ScriptedRunner::new();
    assert!(runner.calls().is_empty());
    assert_eq!(runner.max_in_flight(), 0);
}
