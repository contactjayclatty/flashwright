// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Prost bindings for the vendored AOSP `update_metadata.proto`.

#[allow(clippy::all, dead_code, unused_imports)]
pub mod chromeos_update_engine {
    include!(concat!(env!("OUT_DIR"), "/chromeos_update_engine.rs"));
}
