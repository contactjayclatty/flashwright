// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! PC check of a patched init_boot image.
//!
//! magiskboot is resolved by absolute path and SHA-256. It is not bundled.

use std::path::Path;

use flashwright_core::magiskboot::{self, BootInspection};
use flashwright_core::proc::CommandRunner;

use crate::gates::check_patched_sha1;
use crate::MagiskError;

/// Paths and hashes for one PC check of a patched init_boot image.
pub struct PatchedCheck<'a> {
    pub tool_path: &'a Path,
    pub expected_sha256: &'a str,
    pub image: &'a Path,
    pub work: &'a Path,
    pub stock_sha1: &'a str,
    pub stock_sha256: &'a str,
    pub patched_sha256: &'a str,
}

/// Unpack the patched init_boot and require Magisk's stock SHA-1.
pub async fn validate_patched_init_boot<R: CommandRunner>(
    runner: &R,
    check: PatchedCheck<'_>,
) -> Result<BootInspection, MagiskError> {
    let tool = magiskboot::resolve(check.tool_path, check.expected_sha256)
        .map_err(|err| MagiskError::Message(err.to_string()))?;
    let report = magiskboot::inspect_patched_init_boot(runner, &tool, check.image, check.work)
        .await
        .map_err(|err| MagiskError::Message(err.to_string()))?;
    check_patched_sha1(
        check.stock_sha1,
        check.stock_sha256,
        check.patched_sha256,
        &report.config_sha1,
    )?;
    Ok(report)
}
