// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Flashwright window for the Windows update wizard.
//!
//! The `gui` feature links the window. Workspace checks leave it off so they
//! do not need a WebView.

#[cfg(feature = "gui")]
mod shell;

#[cfg(feature = "gui")]
pub use shell::run;

#[cfg(not(feature = "gui"))]
pub fn run() {
    eprintln!("The Flashwright window is built with the gui feature.");
}
