// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Package gates. Messages name the gate and omit checksums and build ids.

use std::path::Path;

use crate::bootimg::{self, BootInfo};
use crate::catalog::AliasTable;
use crate::error::FirmwareError;
use crate::hashutil::{self, hex_encode, hex_eq, sha1_file, sha256_file};
use crate::metadata::PackageMeta;
use crate::region::is_restricted_region;
use crate::select::StockPartition;

pub const MAX_IMAGE: u64 = 256 * 1024 * 1024;
pub const MAX_INNER_ZIP: u64 = 16 * 1024 * 1024 * 1024;

pub struct ImageCheck {
    pub image_sha256: String,
    pub image_sha1: String,
    pub security_patch: Option<String>,
    pub fingerprint: Option<String>,
}

pub fn extension_ok(path: &Path) -> Result<(), FirmwareError> {
    let name = file_label(path).to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".tar") {
        return Err(FirmwareError::UnsupportedArchive);
    }
    if !name.ends_with(".zip") {
        return Err(FirmwareError::NotZip);
    }
    Ok(())
}

pub fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

pub fn reject_region(text: &str) -> Result<(), FirmwareError> {
    if is_restricted_region(text) {
        Err(FirmwareError::RestrictedRegion)
    } else {
        Ok(())
    }
}

pub fn filename_fragment_ok(path: &Path, package_sha256: &str) -> Result<(), FirmwareError> {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or(FirmwareError::FilenameFragment)?;
    let token = stem
        .rsplit('-')
        .next()
        .ok_or(FirmwareError::FilenameFragment)?;
    let fragment_ok = token.len() == 8 && token.chars().all(|ch| ch.is_ascii_hexdigit());
    if fragment_ok
        && package_sha256
            .to_ascii_lowercase()
            .starts_with(&token.to_ascii_lowercase())
    {
        Ok(())
    } else {
        Err(FirmwareError::FilenameFragment)
    }
}

pub fn published_ok(published: Option<&str>, package_sha256: &str) -> Result<(), FirmwareError> {
    let Some(published) = published.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if published.len() == 64
        && published.chars().all(|ch| ch.is_ascii_hexdigit())
        && hex_eq(published, package_sha256)
    {
        Ok(())
    } else {
        Err(FirmwareError::PublishedHash)
    }
}

pub fn codename_from_filename(path: &Path) -> Result<String, FirmwareError> {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or(FirmwareError::CodenameMismatch)?;
    let mut parts = stem.split('-').filter(|part| !part.is_empty());
    let first = parts.next().ok_or(FirmwareError::CodenameMismatch)?;
    if first.eq_ignore_ascii_case("image") {
        return parts
            .next()
            .map(ToString::to_string)
            .ok_or(FirmwareError::CodenameMismatch);
    }
    Ok(first.to_string())
}

pub fn agree_codename(
    aliases: &AliasTable,
    filename_codename: &str,
    stated: &[String],
    expected: Option<&str>,
) -> Result<String, FirmwareError> {
    if stated.is_empty()
        || !stated
            .iter()
            .any(|name| aliases.same(filename_codename, name))
    {
        return Err(FirmwareError::CodenameMismatch);
    }
    if let Some(expected) = expected.map(str::trim).filter(|value| !value.is_empty()) {
        if !stated.iter().any(|name| aliases.same(expected, name)) {
            return Err(FirmwareError::CodenameMismatch);
        }
    }
    let matched = stated
        .iter()
        .find(|name| aliases.same(filename_codename, name))
        .ok_or(FirmwareError::CodenameMismatch)?;
    Ok(aliases.canonical(matched).into_owned())
}

pub fn downgrade_ok(
    meta: &PackageMeta,
    device_timestamp: Option<u64>,
    device_security_patch: Option<&str>,
) -> Result<(), FirmwareError> {
    if let (Some(device_timestamp), Some(post)) = (device_timestamp, meta.post_timestamp) {
        if post < device_timestamp {
            return Err(FirmwareError::Downgrade);
        }
    }
    if let Some(device_patch) = device_security_patch
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(firmware_patch) = meta.post_security_patch.as_deref() {
            if firmware_patch < device_patch {
                return Err(FirmwareError::Downgrade);
            }
        }
    }
    Ok(())
}

pub fn full_ab_ota(meta: &PackageMeta) -> Result<(), FirmwareError> {
    if meta
        .pre_build
        .as_ref()
        .is_some_and(|value| !value.is_empty())
    {
        return Err(FirmwareError::IncrementalOta);
    }
    match meta.ota_type.as_deref() {
        Some(kind) if kind.eq_ignore_ascii_case("AB") => Ok(()),
        _ => Err(FirmwareError::NotFullOta),
    }
}

pub fn avb_ok(info: &BootInfo, meta: &PackageMeta) -> Result<(), FirmwareError> {
    if let Some(expected) = meta.post_security_patch.as_deref() {
        match info.security_patch.as_deref() {
            Some(got) if got == expected => {}
            _ => return Err(FirmwareError::SecurityPatchMismatch),
        }
    }
    if let Some(expected) = meta.post_build.as_deref() {
        match info.fingerprint.as_deref() {
            Some(got) if got == expected => {}
            _ => return Err(FirmwareError::FingerprintMismatch),
        }
    }
    Ok(())
}

pub fn check_extracted_image(
    path: &Path,
    partition: StockPartition,
    meta: &PackageMeta,
    expected_sha256: Option<&[u8]>,
    expected_size: Option<u64>,
) -> Result<ImageCheck, FirmwareError> {
    let length = std::fs::metadata(path).map_err(FirmwareError::io)?.len();
    if length > MAX_IMAGE {
        let _ = std::fs::remove_file(path);
        return Err(FirmwareError::Archive(
            "the image is larger than Flashwright will extract".into(),
        ));
    }
    if let Some(expected) = expected_size {
        if length != expected {
            let _ = std::fs::remove_file(path);
            return Err(FirmwareError::PartitionHash);
        }
    }
    let image_sha256 = sha256_file(path, None)?;
    if let Some(expected) = expected_sha256 {
        if !hex_eq(&image_sha256, &hex_encode(expected)) {
            let _ = std::fs::remove_file(path);
            return Err(FirmwareError::PartitionHash);
        }
    }
    let info = match bootimg::read_boot_image(path, partition.as_str()) {
        Ok(info) => info,
        Err(err) => {
            let _ = std::fs::remove_file(path);
            return Err(err);
        }
    };
    if let Err(err) = avb_ok(&info, meta) {
        let _ = std::fs::remove_file(path);
        return Err(err);
    }
    let image_sha1 = sha1_file(path)?;
    Ok(ImageCheck {
        image_sha256,
        image_sha1,
        security_patch: info.security_patch,
        fingerprint: info.fingerprint,
    })
}

pub fn hash_with(
    path: &Path,
    progress: &mut Option<Box<dyn FnMut(u64) + Send>>,
) -> Result<String, FirmwareError> {
    match progress {
        Some(callback) => hashutil::sha256_file(path, Some(callback.as_mut())),
        None => hashutil::sha256_file(path, None),
    }
}
