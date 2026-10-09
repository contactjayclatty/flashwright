// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Prost bindings for the vendored AOSP `update_metadata.proto`.
//! The generated text is `proto_gen.rs`, checked in so the crate does not
//! invoke a compiler. `xtask check` regenerates it and fails on any difference.

#[allow(clippy::all, dead_code, unused_imports)]
pub mod chromeos_update_engine {
    include!("proto_gen.rs");
}
