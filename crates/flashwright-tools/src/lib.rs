// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Managed, pinned platform-tools.
//!
//! Flashwright runs the user's `adb` and `fastboot`. It never redistributes
//! them. Writes require an allow-list entry whose file hashes match.

mod discover;
mod error;
mod hashutil;
mod import;
mod policy;
mod server;
mod version;

pub use discover::{
    adb_version, candidate_directories, directories_with_tools, evaluate_installation,
    DiscoverRequest, ToolBinaryNames, ToolsReport,
};
pub use error::ToolsError;
pub use hashutil::{sha1_bytes, sha256_bytes, sha256_file};
pub use import::{import_platform_tools_zip, safe_relative, ImportReport};
pub use policy::{
    classify, AllowEntry, HostKind, PlatformToolsPolicy, ToolFiles, ToolsVerdict,
    CANDIDATE_37_0_1_ZIP_SHA1,
};
pub use server::{
    assess_server, parse_host_version, probe_adb_server, restart_adb_server, ServerObservation,
    ServerStatus, DEFAULT_ADB_PORT,
};
pub use version::{parse_adb_version_output, SdkVersion};
