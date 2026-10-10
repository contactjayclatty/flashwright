// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Patch gates that can be decided before a plan is confirmed.

use flashwright_core::device::{DeviceTable, KnownBadMagisk, Partition};

use crate::image::{is_hex, ExtractedBootImage};
use crate::MagiskError;

pub const KOMODO: &str = "komodo";
pub const MIN_CODE_FOR_LATE_SPL: u32 = 30_600;
pub const LATE_SPL: &str = "2025-12-01";
const HEADROOM: u64 = 64 * 1024 * 1024;

/// The partition named in `devices.toml` is the one this phone patches.
pub fn patch_partition(
    codename: &str,
    image: &dyn ExtractedBootImage,
) -> Result<Partition, MagiskError> {
    let devices = DeviceTable::embedded().map_err(|err| MagiskError::Message(err.to_string()))?;
    let Some(row) = devices.get(codename) else {
        return Err(MagiskError::Message(format!("unknown device {codename}")));
    };
    if image.partition() != row.patch_partition {
        if codename.eq_ignore_ascii_case(KOMODO) {
            return Err(MagiskError::KomodoBoot);
        }
        let label = row.model.clone().unwrap_or_else(|| row.codename.clone());
        return Err(MagiskError::Message(format!(
            "{label} patches {}, not {}.",
            row.patch_partition.fastboot_name(),
            image.partition().fastboot_name()
        )));
    }
    Ok(row.patch_partition)
}

/// The LU0 / FIPS region is a hard block. Other region labels pass.
pub fn check_region(region: &str) -> Result<(), MagiskError> {
    let folded = region.trim().to_ascii_uppercase();
    if folded == "LU0" || folded == "FIPS" || folded == "LU0 / FIPS" || folded == "LU0/FIPS" {
        return Err(MagiskError::Lu0Fips);
    }
    Ok(())
}

/// Strict stock SHA-1. The ramdisk value must be the full 40 hex digits.
///
/// The patched file's SHA-256 must differ from the stock file.
pub fn check_patched_sha1(
    stock_sha1: &str,
    stock_sha256: &str,
    patched_sha256: &str,
    config_sha1: &str,
) -> Result<(), MagiskError> {
    if !is_hex(stock_sha1, 40) || !is_hex(stock_sha256, 64) || !is_hex(patched_sha256, 64) {
        return Err(MagiskError::PatchedSha1);
    }
    if !config_sha1.eq_ignore_ascii_case(stock_sha1) || !is_hex(config_sha1, 40) {
        return Err(MagiskError::PatchedSha1);
    }
    if patched_sha256.eq_ignore_ascii_case(stock_sha256) {
        return Err(MagiskError::PatchedSha1);
    }
    Ok(())
}

/// Known-bad codes come from the caller. The embedded table is the safety list.
pub fn check_magisk_version(
    version_code: u32,
    security_patch: &str,
    known_bad: &[u32],
) -> Result<(), MagiskError> {
    if known_bad.contains(&version_code) {
        return Err(MagiskError::KnownBad);
    }
    if !is_date(security_patch) {
        return Err(MagiskError::Message("security patch is not a date".into()));
    }
    if security_patch >= LATE_SPL && version_code < MIN_CODE_FOR_LATE_SPL {
        return Err(MagiskError::MagiskTooOld);
    }
    Ok(())
}

/// `/data` free space from a `dumpsys diskstats` fixture, in bytes.
pub fn data_free_bytes(diskstats: &str) -> Option<u64> {
    for line in diskstats.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("Data-Free:") else {
            continue;
        };
        let token = rest.split_whitespace().next()?.trim();
        let (digits, scale) = scale_of(token)?;
        let value: u64 = digits.parse().ok()?;
        return value.checked_mul(scale);
    }
    None
}

pub fn space_needed(image_size: u64) -> Option<u64> {
    image_size.checked_mul(3)?.checked_add(HEADROOM)
}

/// Block when `/data` cannot hold three copies of the image plus 64 MiB.
pub fn check_device_space(diskstats: &str, image_size: u64) -> Result<(), MagiskError> {
    let free = data_free_bytes(diskstats)
        .ok_or_else(|| MagiskError::Message("dumpsys diskstats did not report Data-Free".into()))?;
    let need = space_needed(image_size)
        .ok_or_else(|| MagiskError::Message("image size is too large".into()))?;
    if free < need {
        Err(MagiskError::DeviceSpace)
    } else {
        Ok(())
    }
}

pub fn embedded_known_bad() -> Result<Vec<u32>, MagiskError> {
    let table = KnownBadMagisk::embedded().map_err(|err| MagiskError::Message(err.to_string()))?;
    Ok(table.version_codes)
}

pub fn komodo_has_init_boot() -> Result<bool, MagiskError> {
    let devices = DeviceTable::embedded().map_err(|err| MagiskError::Message(err.to_string()))?;
    Ok(devices.get(KOMODO).is_some_and(|row| row.has_init_boot))
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

fn scale_of(token: &str) -> Option<(&str, u64)> {
    if let Some(digits) = token.strip_suffix('K').or_else(|| token.strip_suffix('k')) {
        Some((digits, 1024))
    } else if let Some(digits) = token.strip_suffix('M').or_else(|| token.strip_suffix('m')) {
        Some((digits, 1024 * 1024))
    } else if token.chars().all(|ch| ch.is_ascii_digit()) {
        Some((token, 1))
    } else {
        None
    }
}
