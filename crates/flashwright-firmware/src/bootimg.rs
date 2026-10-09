// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Android boot header (v0–v4) plus the AVB footer and vbmeta properties.
//! The footer layout follows AOSP `external/avb` (MIT). See `third_party/aosp/avb`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::FirmwareError;

const BOOT_MAGIC: &[u8; 8] = b"ANDROID!";
const FOOTER_LEN: usize = 64;
const VBMETA_HEADER: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootInfo {
    pub header_version: u32,
    pub security_patch: Option<String>,
    pub fingerprint: Option<String>,
}

pub fn read_boot_image(path: &Path, partition: &str) -> Result<BootInfo, FirmwareError> {
    let mut file = File::open(path).map_err(FirmwareError::io)?;
    let mut head = [0u8; 48];
    file.read_exact(&mut head).map_err(FirmwareError::io)?;
    if &head[0..8] != BOOT_MAGIC {
        return Err(FirmwareError::BootImage);
    }
    let header_version = u32::from_le_bytes(head[40..44].try_into().unwrap());
    if header_version > 4 {
        return Err(FirmwareError::BootImage);
    }
    let len = file.seek(SeekFrom::End(0)).map_err(FirmwareError::io)?;
    if len < FOOTER_LEN as u64 + VBMETA_HEADER as u64 {
        return Err(FirmwareError::BootImage);
    }
    file.seek(SeekFrom::End(-(FOOTER_LEN as i64)))
        .map_err(FirmwareError::io)?;
    let mut footer = [0u8; FOOTER_LEN];
    file.read_exact(&mut footer).map_err(FirmwareError::io)?;
    if &footer[0..4] != b"AVBf" {
        return Err(FirmwareError::BootImage);
    }
    let vbmeta_offset = u64::from_be_bytes(footer[16..24].try_into().unwrap());
    let vbmeta_size = u64::from_be_bytes(footer[24..32].try_into().unwrap());
    if vbmeta_size < VBMETA_HEADER as u64 || vbmeta_offset.saturating_add(vbmeta_size) > len {
        return Err(FirmwareError::BootImage);
    }
    if vbmeta_size > 1024 * 1024 {
        return Err(FirmwareError::BootImage);
    }
    file.seek(SeekFrom::Start(vbmeta_offset))
        .map_err(FirmwareError::io)?;
    let mut vbmeta = vec![0u8; vbmeta_size as usize];
    file.read_exact(&mut vbmeta).map_err(FirmwareError::io)?;
    let props = properties(&vbmeta)?;
    let patch_key = format!("com.android.build.{partition}.security_patch");
    let print_key = format!("com.android.build.{partition}.fingerprint");
    Ok(BootInfo {
        header_version,
        security_patch: props.get(&patch_key).cloned(),
        fingerprint: props.get(&print_key).cloned(),
    })
}

fn properties(vbmeta: &[u8]) -> Result<std::collections::BTreeMap<String, String>, FirmwareError> {
    if vbmeta.len() < VBMETA_HEADER || &vbmeta[0..4] != b"AVB0" {
        return Err(FirmwareError::BootImage);
    }
    let auth = u64::from_be_bytes(vbmeta[12..20].try_into().unwrap());
    let descriptors_offset = u64::from_be_bytes(vbmeta[96..104].try_into().unwrap());
    let descriptors_size = u64::from_be_bytes(vbmeta[104..112].try_into().unwrap());
    let aux_start = VBMETA_HEADER as u64 + auth;
    let start = aux_start.saturating_add(descriptors_offset) as usize;
    let end = start.saturating_add(descriptors_size as usize);
    if end > vbmeta.len() {
        return Err(FirmwareError::BootImage);
    }
    let mut out = std::collections::BTreeMap::new();
    let mut pos = start;
    while pos + 16 <= end {
        let tag = u64::from_be_bytes(vbmeta[pos..pos + 8].try_into().unwrap());
        let following = u64::from_be_bytes(vbmeta[pos + 8..pos + 16].try_into().unwrap()) as usize;
        let body = pos + 16;
        let next = body.saturating_add(following);
        if next > end {
            return Err(FirmwareError::BootImage);
        }
        if tag == 0 && following >= 16 {
            let key_len = u64::from_be_bytes(vbmeta[body..body + 8].try_into().unwrap()) as usize;
            let value_len =
                u64::from_be_bytes(vbmeta[body + 8..body + 16].try_into().unwrap()) as usize;
            let key_at = body + 16;
            let value_at = key_at.saturating_add(key_len).saturating_add(1);
            if value_at.saturating_add(value_len) <= next {
                if let (Ok(key), Ok(value)) = (
                    std::str::from_utf8(&vbmeta[key_at..key_at + key_len]),
                    std::str::from_utf8(&vbmeta[value_at..value_at + value_len]),
                ) {
                    out.insert(key.to_string(), value.to_string());
                }
            }
        }
        pos = next;
    }
    Ok(out)
}

/// A tiny boot image with an AVB footer. Test fixtures only.
#[cfg(test)]
pub fn synthetic_boot(partition: &str, security_patch: &str, fingerprint: &str) -> Vec<u8> {
    let mut image = vec![0u8; 4096];
    image[..8].copy_from_slice(BOOT_MAGIC);
    image[40..44].copy_from_slice(&4u32.to_le_bytes());
    let vbmeta = synthetic_vbmeta(partition, security_patch, fingerprint);
    let vbmeta_offset = image.len() as u64;
    let vbmeta_size = vbmeta.len() as u64;
    image.extend_from_slice(&vbmeta);
    image.extend_from_slice(&synthetic_footer(4096, vbmeta_offset, vbmeta_size));
    image
}

#[cfg(test)]
fn synthetic_vbmeta(partition: &str, security_patch: &str, fingerprint: &str) -> Vec<u8> {
    let descriptors = property_descriptor(
        &format!("com.android.build.{partition}.security_patch"),
        security_patch.as_bytes(),
    )
    .into_iter()
    .chain(property_descriptor(
        &format!("com.android.build.{partition}.fingerprint"),
        fingerprint.as_bytes(),
    ))
    .collect::<Vec<_>>();
    let mut header = vec![0u8; VBMETA_HEADER];
    header[0..4].copy_from_slice(b"AVB0");
    header[4..8].copy_from_slice(&1u32.to_be_bytes());
    put_u64(&mut header, 20, descriptors.len() as u64);
    put_u64(&mut header, 104, descriptors.len() as u64);
    let mut release = b"avbtool 1.2.0".to_vec();
    release.resize(47, 0);
    header[128..175].copy_from_slice(&release);
    header.extend_from_slice(&descriptors);
    header
}

#[cfg(test)]
fn property_descriptor(key: &str, value: &[u8]) -> Vec<u8> {
    let key_bytes = key.as_bytes();
    let num_following = 16 + key_bytes.len() + 1 + value.len() + 1;
    let padded = num_following.div_ceil(8) * 8;
    let mut out = Vec::with_capacity(16 + padded);
    out.extend_from_slice(&0u64.to_be_bytes());
    out.extend_from_slice(&(padded as u64).to_be_bytes());
    out.extend_from_slice(&(key_bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(key_bytes);
    out.push(0);
    out.extend_from_slice(value);
    out.push(0);
    out.resize(16 + padded, 0);
    out
}

#[cfg(test)]
fn synthetic_footer(original: u64, vbmeta_offset: u64, vbmeta_size: u64) -> [u8; FOOTER_LEN] {
    let mut footer = [0u8; FOOTER_LEN];
    footer[0..4].copy_from_slice(b"AVBf");
    footer[4..8].copy_from_slice(&1u32.to_be_bytes());
    put_u64_arr(&mut footer, 8, original);
    put_u64_arr(&mut footer, 16, vbmeta_offset);
    put_u64_arr(&mut footer, 24, vbmeta_size);
    footer
}

#[cfg(test)]
fn put_u64(buf: &mut [u8], at: usize, value: u64) {
    buf[at..at + 8].copy_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
fn put_u64_arr(buf: &mut [u8; FOOTER_LEN], at: usize, value: u64) {
    buf[at..at + 8].copy_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_properties() {
        let image = synthetic_boot("init_boot", "2026-10-01", "google/komodo/komodo:17/TEST/1:user/release-keys");
        let dir = std::env::temp_dir().join("fw-boot-roundtrip");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("init_boot.img");
        std::fs::write(&path, &image).unwrap();
        let info = read_boot_image(&path, "init_boot").unwrap();
        assert_eq!(info.header_version, 4);
        assert_eq!(info.security_patch.as_deref(), Some("2026-10-01"));
        assert!(info.fingerprint.unwrap().contains("komodo"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
