// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Package gates. Messages name the gate and omit checksums and build ids.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::bootimg::{self, BootInfo};
use crate::catalog::AliasTable;
use crate::error::FirmwareError;
use crate::facts::DeviceFacts;
use crate::hashutil::{self, hex_encode, hex_eq};
use crate::metadata::PackageMeta;
use crate::region::is_restricted_region;
use crate::select::StockPartition;

pub const MAX_IMAGE: u64 = 256 * 1024 * 1024;
pub const MAX_INNER_ZIP: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_METADATA: u64 = 1024 * 1024;

pub const G07_ACK: &str = "The security patch level could not be read from the boot image.";
pub const G08_ACK: &str = "The build date could not be read from the image.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateAck {
    pub gate: &'static str,
    pub message: &'static str,
}

pub struct ImageCheck {
    pub image_sha256: String,
    pub image_sha1: String,
    pub security_patch: Option<String>,
    pub fingerprint: Option<String>,
    pub image_security_patch: Option<String>,
    pub image_build_date_utc: Option<u64>,
    pub acks: Vec<GateAck>,
}

pub fn extension_ok(name: &str) -> Result<(), FirmwareError> {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".tar") {
        return Err(FirmwareError::UnsupportedArchive);
    }
    if !name.ends_with(".zip") {
        return Err(FirmwareError::NotZip);
    }
    Ok(())
}

pub fn reject_region(text: &str) -> Result<(), FirmwareError> {
    if is_restricted_region(text) {
        Err(FirmwareError::RestrictedRegion)
    } else {
        Ok(())
    }
}

pub fn filename_fragment_ok(name: &str, package_sha256: &str) -> Result<(), FirmwareError> {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
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

/// A missing, empty, or blank published checksum is a G05 block.
pub fn published_ok(published: Option<&str>, package_sha256: &str) -> Result<(), FirmwareError> {
    let Some(published) = published.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(FirmwareError::PublishedHash);
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

pub fn codename_from_filename(name: &str) -> Result<String, FirmwareError> {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
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

pub fn filename_matches_device(
    aliases: &AliasTable,
    filename_codename: &str,
    device_codename: &str,
) -> Result<(), FirmwareError> {
    if device_codename.trim().is_empty() || !aliases.same(filename_codename, device_codename) {
        Err(FirmwareError::CodenameMismatch)
    } else {
        Ok(())
    }
}

pub fn agree_codename(
    aliases: &AliasTable,
    filename_codename: &str,
    stated: &[String],
    device_codename: &str,
) -> Result<String, FirmwareError> {
    if device_codename.trim().is_empty()
        || stated.is_empty()
        || !stated
            .iter()
            .any(|name| aliases.same(filename_codename, name))
        || !stated
            .iter()
            .any(|name| aliases.same(device_codename, name))
    {
        return Err(FirmwareError::CodenameMismatch);
    }
    let matched = stated
        .iter()
        .find(|name| aliases.same(filename_codename, name))
        .ok_or(FirmwareError::CodenameMismatch)?;
    Ok(aliases.canonical(matched).into_owned())
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

pub fn check_image_file<R: Read + Seek>(
    file: &mut R,
    partition: StockPartition,
    meta: &PackageMeta,
    expected_sha256: Option<&[u8]>,
    expected_size: Option<u64>,
    device: &DeviceFacts,
) -> Result<ImageCheck, FirmwareError> {
    let length = file.seek(SeekFrom::End(0)).map_err(FirmwareError::io)?;
    if length > MAX_IMAGE {
        return Err(FirmwareError::Archive(
            "the image is larger than Flashwright will extract".into(),
        ));
    }
    if let Some(expected) = expected_size {
        if length != expected {
            return Err(FirmwareError::PartitionHash);
        }
    }
    file.seek(SeekFrom::Start(0)).map_err(FirmwareError::io)?;
    let digests = hashutil::hash_reader(file, None)?;
    if let Some(expected) = expected_sha256 {
        if !hex_eq(&digests.sha256, &hex_encode(expected)) {
            return Err(FirmwareError::PartitionHash);
        }
    }
    file.seek(SeekFrom::Start(0)).map_err(FirmwareError::io)?;
    let info = bootimg::read_boot(file, partition.as_str())?;
    avb_ok(&info, meta)?;
    let acks = image_gates(&info, device)?;
    Ok(ImageCheck {
        image_sha256: digests.sha256,
        image_sha1: digests.sha1,
        security_patch: info.security_patch,
        fingerprint: info.fingerprint,
        image_security_patch: info.os_patch,
        image_build_date_utc: info.build_date_utc,
        acks,
    })
}

fn image_gates(info: &BootInfo, device: &DeviceFacts) -> Result<Vec<GateAck>, FirmwareError> {
    let mut acks = Vec::new();
    match info.os_patch.as_deref() {
        Some(patch) => {
            if let Some(device_patch) = device
                .security_patch
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if patch_is_older(patch, device_patch) == Some(true) {
                    return Err(FirmwareError::Downgrade);
                }
            }
        }
        None => acks.push(GateAck {
            gate: "G07",
            message: G07_ACK,
        }),
    }
    match info.build_date_utc {
        Some(date) => {
            if let Some(device_date) = device.build_date_utc {
                if date < device_date {
                    return Err(FirmwareError::OlderBuild);
                }
            }
        }
        None => acks.push(GateAck {
            gate: "G08",
            message: G08_ACK,
        }),
    }
    Ok(acks)
}

/// Compare `YYYY-MM` only, so a day suffix does not make a patch look older.
pub fn patch_is_older(firmware: &str, device: &str) -> Option<bool> {
    let firmware = year_month(firmware)?;
    let device = year_month(device)?;
    Some(firmware < device)
}

fn year_month(value: &str) -> Option<(u32, u32)> {
    let mut parts = value.trim().split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    if (1..=12).contains(&month) {
        Some((year, month))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_published_hash_is_a_block() {
        let hash = "ab".repeat(32);
        assert!(matches!(
            published_ok(None, &hash),
            Err(FirmwareError::PublishedHash)
        ));
        assert!(matches!(
            published_ok(Some("   "), &hash),
            Err(FirmwareError::PublishedHash)
        ));
        assert!(matches!(
            published_ok(Some(""), &hash),
            Err(FirmwareError::PublishedHash)
        ));
    }

    #[test]
    fn eos_and_aurora_name_the_same_phone() {
        let aliases = AliasTable::embedded().unwrap();
        let stated = vec!["eos".to_string()];
        let name = agree_codename(&aliases, "aurora", &stated, "aurora").unwrap();
        assert_eq!(name, "aurora");
    }

    #[test]
    fn the_day_does_not_make_a_patch_look_older() {
        assert_eq!(patch_is_older("2026-10", "2026-10-01"), Some(false));
        assert_eq!(patch_is_older("2026-10-05", "2026-10"), Some(false));
        assert_eq!(patch_is_older("2026-10", "2026-11-01"), Some(true));
    }
}
