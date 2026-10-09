// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

fn main() {
    #[cfg(feature = "gui")]
    tauri_build::build();
}
