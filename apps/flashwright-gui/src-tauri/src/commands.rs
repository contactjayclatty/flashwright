// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

// Phase 1 window commands. `build.rs` and the invoke handler both use this list.

pub const PHASE1_COMMANDS: &[&str] = &[
    "engine_snapshot",
    "subscribe",
    "tools_status",
    "tools_pick_folder",
    "tools_import_zip",
    "scan",
    "select_device",
    "continue_from_connect",
    "set_choice",
    "continue_from_choose",
    "back",
    "pick_firmware",
    "firmware_open",
    "prepare_patch",
    "build_plan",
    "ack_gate",
    "dry_run",
    "confirm_and_run",
    "cancel",
    "recovery_plan",
    "backups_list",
    "restore_plan",
    "open_external",
];
