// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

include!("src/commands.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/commands.rs");
    #[cfg(feature = "gui")]
    {
        let attributes = tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(PHASE1_COMMANDS));
        if let Err(error) = tauri_build::try_build(attributes) {
            panic!("The window build failed: {error}");
        }
    }
}
