// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! PC check of a patched init_boot image.
//!
//! The check is a read-only parse. It does not run magiskboot.

use flashwright_bootimg::{BootError, BoundInspection};
use flashwright_core::parse::{self, Verdict};
use sha2::{Digest, Sha256};

use crate::gates::check_patched_sha1;
use crate::MagiskError;

/// Bytes and hashes for one PC check of a patched init_boot image.
pub struct PatchedCheck<'a> {
    pub patched: &'a [u8],
    pub stock_sha1: &'a str,
    pub stock_sha256: &'a str,
    pub plan_hash: &'a str,
}

/// Script output plus the pulled patched image and the pulled APK.
pub struct PatchPull<'a> {
    pub script_text: &'a str,
    pub pull_text: &'a str,
    pub patched: &'a [u8],
    pub apk: &'a [u8],
    pub stock_sha1: &'a str,
    pub stock_sha256: &'a str,
    pub plan_hash: &'a str,
}

/// The pull must say one file arrived, and `FL_STOCK_SHA256` must be the plan's stock hash.
pub fn accept_patch_pull(pull: PatchPull<'_>) -> Result<PatchAcceptance, MagiskError> {
    if parse::parse_pull(pull.pull_text) != Verdict::Ok {
        return Err(MagiskError::Message(
            "adb did not report 1 file pulled".into(),
        ));
    }
    if pull.apk.is_empty() {
        return Err(MagiskError::Message("the pulled APK is empty".into()));
    }
    let reported = pull
        .script_text
        .lines()
        .find_map(|line| line.trim().strip_prefix("FL_STOCK_SHA256="))
        .unwrap_or("");
    if !reported.eq_ignore_ascii_case(pull.stock_sha256) {
        return Err(MagiskError::PatchedSha1);
    }
    let inspection = validate_patched_init_boot(PatchedCheck {
        patched: pull.patched,
        stock_sha1: pull.stock_sha1,
        stock_sha256: pull.stock_sha256,
        plan_hash: pull.plan_hash,
    })?;
    Ok(PatchAcceptance {
        inspection,
        apk_sha256: hex_sha256(pull.apk),
    })
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

/// Parser report plus the hash of the APK that was pulled.
pub struct PatchAcceptance {
    pub inspection: BoundInspection,
    pub apk_sha256: String,
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
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
