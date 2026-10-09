// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Android boot header v3 and v4, for an init_boot image.
//!
//! init_boot carries the generic ramdisk and no kernel. The page size is 4096.

use crate::error::BootError;

const MAGIC: &[u8; 8] = b"ANDROID!";
const PAGE: u64 = 4096;
const V3_HEADER: u32 = 1580;
const V4_HEADER: u32 = 1584;
const MAX_HEADER: u32 = 4096;
pub(crate) const MAX_IMAGE: usize = 32 * 1024 * 1024;
pub(crate) const MAX_RAMDISK_IN: usize = 16 * 1024 * 1024;

pub(crate) struct BootLayout {
    pub header_version: u32,
    pub ramdisk: usize,
    pub ramdisk_len: usize,
}

pub(crate) fn layout(image: &[u8]) -> Result<BootLayout, BootError> {
    if image.len() > MAX_IMAGE {
        return Err(BootError::TooLarge);
    }
    let magic = image.get(..8).ok_or(BootError::Header)?;
    if image.len() < V3_HEADER as usize || magic != MAGIC {
        return Err(BootError::Header);
    }
    let kernel_size = read_u32(image, 8)?;
    let ramdisk_size = read_u32(image, 12)?;
    let header_size = read_u32(image, 20)?;
    let header_version = read_u32(image, 40)?;
    if kernel_size != 0 || (header_version != 3 && header_version != 4) {
        return Err(BootError::Header);
    }
    let min_header = if header_version == 3 {
        V3_HEADER
    } else {
        V4_HEADER
    };
    if header_size < min_header || header_size > MAX_HEADER || image.len() < header_size as usize {
        return Err(BootError::Header);
    }
    if header_version == 4 {
        let signature = read_u32(image, 1580)?;
        if signature as usize > MAX_IMAGE {
            return Err(BootError::Header);
        }
    }
    let ramdisk_len = ramdisk_size as usize;
    if ramdisk_len == 0 || ramdisk_len > MAX_RAMDISK_IN {
        return Err(BootError::TooLarge);
    }
    let header_pages = align_page(u64::from(header_size)).ok_or(BootError::Header)?;
    let ramdisk_at = usize::try_from(header_pages).map_err(|_| BootError::Header)?;
    let ramdisk_end = ramdisk_at
        .checked_add(ramdisk_len)
        .ok_or(BootError::Header)?;
    if ramdisk_end > image.len() {
        return Err(BootError::Header);
    }
    Ok(BootLayout {
        header_version,
        ramdisk: ramdisk_at,
        ramdisk_len,
    })
}

fn read_u32(image: &[u8], at: usize) -> Result<u32, BootError> {
    let end = at.checked_add(4).ok_or(BootError::Header)?;
    let bytes: [u8; 4] = image
        .get(at..end)
        .ok_or(BootError::Header)?
        .try_into()
        .map_err(|_| BootError::Header)?;
    Ok(u32::from_le_bytes(bytes))
}

fn align_page(size: u64) -> Option<u64> {
    let pages = size.checked_add(PAGE - 1)?.checked_div(PAGE)?;
    pages.checked_mul(PAGE)
}
