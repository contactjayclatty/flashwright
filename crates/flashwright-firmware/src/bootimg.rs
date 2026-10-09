// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Android boot header (v0–v4) plus the AVB footer and vbmeta properties.
//! The footer layout follows AOSP `external/avb` (MIT). See `third_party/aosp/avb`.
//!
//! The security patch used for G07 is the header `os_patch_level`. The build
//! date used for G08 is `ro.build.date.utc` in `system/build.prop` or
//! `vendor/build.prop` inside the ramdisk, when that file is present.

use std::io::{Read, Seek, SeekFrom};

use crate::error::FirmwareError;

const BOOT_MAGIC: &[u8; 8] = b"ANDROID!";
const FOOTER_LEN: usize = 64;
const VBMETA_HEADER: usize = 256;
const PAGE: u64 = 4096;
const RAMDISK_CAP: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootInfo {
    pub header_version: u32,
    pub security_patch: Option<String>,
    pub fingerprint: Option<String>,
    pub os_patch: Option<String>,
    pub build_date_utc: Option<u64>,
}

pub fn read_boot<R: Read + Seek>(file: &mut R, partition: &str) -> Result<BootInfo, FirmwareError> {
    file.seek(SeekFrom::Start(0)).map_err(FirmwareError::io)?;
    let mut head = [0u8; 48];
    file.read_exact(&mut head).map_err(FirmwareError::io)?;
    if &head[0..8] != BOOT_MAGIC {
        return Err(FirmwareError::BootImage);
    }
    let header_version = u32_le(&head, 40);
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
    let vbmeta_offset = u64_be(&footer, 20);
    let vbmeta_size = u64_be(&footer, 28);
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
    let os_word = if header_version >= 3 {
        u32_le(&head, 16)
    } else {
        u32_le(&head, 44)
    };
    Ok(BootInfo {
        header_version,
        security_patch: props.get(&patch_key).cloned(),
        fingerprint: props.get(&print_key).cloned(),
        os_patch: decode_os_patch(os_word),
        build_date_utc: read_build_date(file, &head, header_version, len)?,
    })
}

fn read_build_date<R: Read + Seek>(
    file: &mut R,
    head: &[u8; 48],
    header_version: u32,
    len: u64,
) -> Result<Option<u64>, FirmwareError> {
    let Some((offset, size)) = ramdisk_range(head, header_version) else {
        return Ok(None);
    };
    if size == 0 || size > RAMDISK_CAP || offset.saturating_add(size) > len {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(FirmwareError::io)?;
    let mut ramdisk = vec![0u8; size as usize];
    file.read_exact(&mut ramdisk).map_err(FirmwareError::io)?;
    Ok(cpio_build_date(&ramdisk))
}

fn ramdisk_range(head: &[u8; 48], header_version: u32) -> Option<(u64, u64)> {
    if header_version >= 3 {
        let kernel = u64::from(u32_le(head, 8));
        let ramdisk = u64::from(u32_le(head, 12));
        let header = u64::from(u32_le(head, 20));
        if header == 0 || header > 1024 * 1024 {
            return None;
        }
        let kernel_at = page_align(header, PAGE);
        let ramdisk_at = kernel_at.saturating_add(page_align(kernel, PAGE));
        Some((ramdisk_at, ramdisk))
    } else {
        let page = u64::from(u32_le(head, 36));
        if page == 0 || page > 1024 * 1024 {
            return None;
        }
        let kernel = u64::from(u32_le(head, 8));
        let ramdisk = u64::from(u32_le(head, 16));
        let ramdisk_at = page.saturating_add(page_align(kernel, page));
        Some((ramdisk_at, ramdisk))
    }
}

fn page_align(value: u64, page: u64) -> u64 {
    if page == 0 {
        return value;
    }
    value.div_ceil(page).saturating_mul(page)
}

pub fn decode_os_patch(word: u32) -> Option<String> {
    if word == 0 {
        return None;
    }
    let month = word & 0x0f;
    let year = (word >> 4) & 0x7f;
    if month == 0 || year == 0 || month > 12 {
        return None;
    }
    Some(format!("{:04}-{:02}", 2000 + year, month))
}

fn cpio_build_date(ramdisk: &[u8]) -> Option<u64> {
    let mut system = None;
    let mut vendor = None;
    let mut pos = 0usize;
    while pos + 110 <= ramdisk.len() {
        let magic = &ramdisk[pos..pos + 6];
        if magic != b"070701" && magic != b"070702" {
            break;
        }
        let filesize = parse_hex_u32(&ramdisk[pos + 54..pos + 62])? as usize;
        let namesize = parse_hex_u32(&ramdisk[pos + 94..pos + 102])? as usize;
        if namesize == 0 || namesize > 4096 {
            break;
        }
        let name_at = pos + 110;
        let name_end = name_at.checked_add(namesize)?;
        if name_end > ramdisk.len() {
            break;
        }
        let name = std::str::from_utf8(&ramdisk[name_at..name_end])
            .ok()?
            .trim_end_matches('\0');
        let data_at = align4(name_end);
        let data_end = data_at.checked_add(filesize)?;
        if data_end > ramdisk.len() {
            break;
        }
        if name == "TRAILER!!!" {
            break;
        }
        let data = &ramdisk[data_at..data_end];
        if is_prop(name, "system/build.prop") {
            if let Some(date) = prop_date(data) {
                system = Some(date);
            }
        } else if is_prop(name, "vendor/build.prop") {
            if let Some(date) = prop_date(data) {
                vendor = Some(date);
            }
        }
        pos = align4(data_end);
    }
    system.or(vendor)
}

fn is_prop(name: &str, suffix: &str) -> bool {
    name == suffix
        || name.strip_prefix("./") == Some(suffix)
        || name.ends_with(&format!("/{suffix}"))
}

fn prop_date(data: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(data).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("ro.build.date.utc=") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn parse_hex_u32(bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(bytes).ok()?;
    u32::from_str_radix(text, 16).ok()
}

fn align4(value: usize) -> usize {
    value.saturating_add(3) & !3
}

fn properties(vbmeta: &[u8]) -> Result<std::collections::BTreeMap<String, String>, FirmwareError> {
    if vbmeta.len() < VBMETA_HEADER || &vbmeta[0..4] != b"AVB0" {
        return Err(FirmwareError::BootImage);
    }
    let auth = u64_be(vbmeta, 12);
    let descriptors_offset = u64_be(vbmeta, 96);
    let descriptors_size = u64_be(vbmeta, 104);
    let aux_start = VBMETA_HEADER as u64 + auth;
    let start = aux_start.saturating_add(descriptors_offset) as usize;
    let end = start.saturating_add(descriptors_size as usize);
    if end > vbmeta.len() {
        return Err(FirmwareError::BootImage);
    }
    let mut out = std::collections::BTreeMap::new();
    let mut pos = start;
    while pos + 16 <= end {
        let tag = u64_be(vbmeta, pos);
        let following = u64_be(vbmeta, pos + 8) as usize;
        let body = pos + 16;
        let next = body.saturating_add(following);
        if next > end {
            return Err(FirmwareError::BootImage);
        }
        if tag == 0 && following >= 16 {
            let key_len = u64_be(vbmeta, body) as usize;
            let value_len = u64_be(vbmeta, body + 8) as usize;
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

fn u32_le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn u64_be(bytes: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(bytes[at..at + 8].try_into().unwrap())
}

/// A tiny boot image with an AVB footer. Test fixtures only.
#[cfg(test)]
pub fn synthetic_boot(partition: &str, security_patch: &str, fingerprint: &str) -> Vec<u8> {
    let os_patch = year_month(security_patch);
    synthetic_boot_custom(
        partition,
        security_patch,
        fingerprint,
        os_patch,
        Some(1_750_000_000),
    )
}

/// `os_patch` is the header security patch `(year, month)`. `None` leaves it unreadable.
#[cfg(test)]
pub fn synthetic_boot_custom(
    partition: &str,
    security_patch: &str,
    fingerprint: &str,
    os_patch: Option<(u32, u32)>,
    build_date_utc: Option<u64>,
) -> Vec<u8> {
    let cpio = match build_date_utc {
        Some(date) => cpio_with_date(date),
        None => Vec::new(),
    };
    let mut image = vec![0u8; 4096];
    image[..8].copy_from_slice(BOOT_MAGIC);
    image[12..16].copy_from_slice(&(cpio.len() as u32).to_le_bytes());
    if let Some((year, month)) = os_patch {
        image[16..20].copy_from_slice(&pack_patch(year, month).to_le_bytes());
    }
    image[20..24].copy_from_slice(&4096u32.to_le_bytes());
    image[40..44].copy_from_slice(&4u32.to_le_bytes());
    image.extend_from_slice(&cpio);
    let original = image.len() as u64;
    let vbmeta = synthetic_vbmeta(partition, security_patch, fingerprint);
    let vbmeta_offset = image.len() as u64;
    let vbmeta_size = vbmeta.len() as u64;
    image.extend_from_slice(&vbmeta);
    image.extend_from_slice(&synthetic_footer(original, vbmeta_offset, vbmeta_size));
    image
}

#[cfg(test)]
fn year_month(patch: &str) -> Option<(u32, u32)> {
    let mut parts = patch.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    if year < 2000 || !(1..=12).contains(&month) {
        None
    } else {
        Some((year, month))
    }
}

#[cfg(test)]
fn pack_patch(year: u32, month: u32) -> u32 {
    let packed_year = (year.saturating_sub(2000)) & 0x7f;
    let packed_month = month & 0x0f;
    (packed_year << 4) | packed_month
}

#[cfg(test)]
fn cpio_with_date(date: u64) -> Vec<u8> {
    let body = format!("ro.build.date.utc={date}\n");
    let mut out = newc_file("system/build.prop", body.as_bytes());
    out.extend_from_slice(&newc_file("TRAILER!!!", b""));
    out
}

#[cfg(test)]
fn newc_file(path: &str, data: &[u8]) -> Vec<u8> {
    let mut name = path.as_bytes().to_vec();
    name.push(0);
    let mut header = Vec::with_capacity(110 + name.len() + data.len() + 8);
    header.extend_from_slice(b"070701");
    for value in [
        1u32,
        0o100644,
        0,
        0,
        1,
        0,
        data.len() as u32,
        0,
        0,
        0,
        0,
        name.len() as u32,
        0,
    ] {
        header.extend_from_slice(format!("{value:08x}").as_bytes());
    }
    header.extend_from_slice(&name);
    while header.len() % 4 != 0 {
        header.push(0);
    }
    header.extend_from_slice(data);
    while header.len() % 4 != 0 {
        header.push(0);
    }
    header
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
    put_u64_arr(&mut footer, 12, original);
    put_u64_arr(&mut footer, 20, vbmeta_offset);
    put_u64_arr(&mut footer, 28, vbmeta_size);
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
    use std::io::Cursor;

    #[test]
    fn round_trip_header_patch_and_build_date() {
        let image = synthetic_boot(
            "init_boot",
            "2026-10-01",
            "google/komodo/komodo:17/TEST/1:user/release-keys",
        );
        let mut cursor = Cursor::new(image);
        let info = read_boot(&mut cursor, "init_boot").unwrap();
        assert_eq!(info.header_version, 4);
        assert_eq!(info.security_patch.as_deref(), Some("2026-10-01"));
        assert_eq!(info.os_patch.as_deref(), Some("2026-10"));
        assert_eq!(info.build_date_utc, Some(1_750_000_000));
        assert!(info.fingerprint.unwrap().contains("komodo"));
    }

    #[test]
    fn a_zero_patch_and_a_ramdisk_without_build_prop_are_unreadable() {
        let image = synthetic_boot_custom(
            "init_boot",
            "2026-10-01",
            "google/komodo/komodo:17/TEST/1:user/release-keys",
            None,
            None,
        );
        let mut cursor = Cursor::new(image);
        let info = read_boot(&mut cursor, "init_boot").unwrap();
        assert_eq!(info.os_patch, None);
        assert_eq!(info.build_date_utc, None);
        assert_eq!(info.security_patch.as_deref(), Some("2026-10-01"));
    }
}
