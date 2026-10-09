// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! PC check of a patched init_boot image.
//!
//! The check is a read-only parse. It does not run magiskboot.

use flashwright_bootimg::{BootError, BoundInspection};

use crate::gates::check_patched_sha1;
use crate::MagiskError;

/// Bytes and hashes for one PC check of a patched init_boot image.
pub struct PatchedCheck<'a> {
    pub patched: &'a [u8],
    pub stock_sha1: &'a str,
    pub stock_sha256: &'a str,
    pub plan_hash: &'a str,
}

/// Read the patched init_boot and require Magisk's stock SHA-1.
pub fn validate_patched_init_boot(check: PatchedCheck<'_>) -> Result<BoundInspection, MagiskError> {
    let report = flashwright_bootimg::inspect_patched_init_boot(
        check.patched,
        check.stock_sha1,
        check.stock_sha256,
        check.plan_hash,
    )
    .map_err(map_boot)?;
    check_patched_sha1(
        check.stock_sha1,
        check.stock_sha256,
        &report.patched_sha256,
        &report.config_sha1,
    )?;
    Ok(report)
}

fn map_boot(err: BootError) -> MagiskError {
    match err {
        BootError::NoMagiskInit | BootError::MissingSha1 | BootError::StockMismatch => {
            MagiskError::PatchedSha1
        }
        BootError::UnsupportedRamdisk => MagiskError::Message("unsupported ramdisk format".into()),
        other => MagiskError::Message(other.to_string()),
    }
}
