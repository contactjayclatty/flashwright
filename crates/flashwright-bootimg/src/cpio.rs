// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Newc cpio walk. Only Magisk's `init` and `.backup/.magisk` are kept.

use crate::error::BootError;

const HEADER: usize = 110;
const MAX_ENTRIES: usize = 4096;
const MAX_NAME: usize = 256;
const MAX_FILE: usize = 8 * 1024 * 1024;
const MAX_CONFIG: usize = 4096;
const REGULAR: u32 = 0o100000;
const TYPE_MASK: u32 = 0o170000;
const EXECUTABLE: u32 = 0o111;

pub(crate) struct MagiskRamdisk {
    pub config: Vec<u8>,
    pub init: Vec<u8>,
}

pub(crate) fn walk(archive: &[u8]) -> Result<MagiskRamdisk, BootError> {
    let mut pos = 0usize;
    let mut entries = 0usize;
    let mut init = Vec::new();
    let mut saw_init = false;
    let mut saw_config = false;
    let mut config = None;
    let mut saw_trailer = false;
    while pos < archive.len() {
        if entries >= MAX_ENTRIES {
            return Err(BootError::TooLarge);
        }
        let header_end = pos.checked_add(HEADER).ok_or(BootError::Header)?;
        let header = archive.get(pos..header_end).ok_or(BootError::Header)?;
        if header.get(..6) != Some(b"070701".as_slice()) {
            return Err(BootError::Header);
        }
        let mode = hex8(header, 14)?;
        let file_size = hex8(header, 54)? as usize;
        let name_size = hex8(header, 94)? as usize;
        if name_size == 0 || name_size > MAX_NAME || file_size > MAX_FILE {
            return Err(BootError::TooLarge);
        }
        let name_at = header_end;
        let name_end = name_at.checked_add(name_size).ok_or(BootError::Header)?;
        let name_bytes = archive.get(name_at..name_end).ok_or(BootError::Header)?;
        if name_bytes.last() != Some(&0) {
            return Err(BootError::Header);
        }
        let name_text = name_bytes
            .get(..name_size.saturating_sub(1))
            .ok_or(BootError::Header)?;
        let name = std::str::from_utf8(name_text).map_err(|_| BootError::Header)?;
        let bare = name.strip_prefix("./").unwrap_or(name);
        if name_rejected(bare) {
            return Err(BootError::Header);
        }
        let data_at = align4(name_end).ok_or(BootError::Header)?;
        let data_end = data_at.checked_add(file_size).ok_or(BootError::Header)?;
        let data = archive.get(data_at..data_end).ok_or(BootError::Header)?;
        let next = align4(data_end).ok_or(BootError::Header)?;
        if next < data_end {
            return Err(BootError::Header);
        }
        entries += 1;
        if bare == "TRAILER!!!" {
            saw_trailer = true;
            break;
        }
        if bare == "overlay.d" || bare.starts_with("overlay.d/") {
            return Err(BootError::Header);
        }
        if bare == "init" {
            if saw_init {
                return Err(BootError::Header);
            }
            saw_init = true;
            let regular = mode & TYPE_MASK == REGULAR;
            let executable = mode & EXECUTABLE != 0;
            if regular && executable && !data.is_empty() {
                init = data.to_vec();
            }
        }
        if bare == ".backup/.magisk" {
            if saw_config {
                return Err(BootError::Header);
            }
            saw_config = true;
            if file_size > MAX_CONFIG {
                return Err(BootError::TooLarge);
            }
            config = Some(data.to_vec());
        }
        pos = next;
    }
    if !saw_trailer {
        return Err(BootError::Header);
    }
    if init.is_empty() {
        return Err(BootError::NoMagiskInit);
    }
    let config = config.ok_or(BootError::MissingSha1)?;
    Ok(MagiskRamdisk { config, init })
}

fn name_rejected(name: &str) -> bool {
    if name == "TRAILER!!!" {
        return false;
    }
    name.starts_with('/')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

fn hex8(header: &[u8], at: usize) -> Result<u32, BootError> {
    let bytes = header.get(at..at + 8).ok_or(BootError::Header)?;
    let text = std::str::from_utf8(bytes).map_err(|_| BootError::Header)?;
    u32::from_str_radix(text, 16).map_err(|_| BootError::Header)
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|sum| sum & !3)
}

pub(crate) fn config_sha1(config: &[u8]) -> Result<String, BootError> {
    let text = std::str::from_utf8(config).map_err(|_| BootError::MissingSha1)?;
    let mut found = None;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("SHA1=") else {
            continue;
        };
        let rest = rest.trim();
        if rest.len() == 40 && rest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            let lower = rest.to_ascii_lowercase();
            if found.as_ref().is_some_and(|prior| prior != &lower) {
                return Err(BootError::MissingSha1);
            }
            found = Some(lower);
        }
    }
    found.ok_or(BootError::MissingSha1)
}
