// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Chunked SHA-256. The behaviour follows PixelFlasher `sha256` in
//! `runtime.py` (commit 081286d). The read size is 1 MiB, and a progress
//! mark is due every 64 MiB.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::error::FirmwareError;

pub const HASH_CHUNK: usize = 1024 * 1024;
pub const HASH_PROGRESS_EVERY: u64 = 64 * 1024 * 1024;

pub fn hash_chunk_len() -> usize {
    HASH_CHUNK
}

pub fn hash_progress_interval() -> u64 {
    HASH_PROGRESS_EVERY
}

pub fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn hex_eq(left: &str, right: &str) -> bool {
    !left.is_empty() && left.eq_ignore_ascii_case(right)
}

/// Report when `bytes` crosses another `interval` boundary, and always when done.
pub fn progress_due(bytes: u64, last_mark: &mut u64, interval: u64, done: bool) -> bool {
    if done {
        return true;
    }
    if interval == 0 {
        return false;
    }
    if bytes / interval > *last_mark / interval {
        *last_mark = bytes;
        true
    } else {
        false
    }
}

pub fn sha256_file(path: &Path, mut progress: Option<&mut dyn FnMut(u64)>) -> Result<String, FirmwareError> {
    let mut file = File::open(path).map_err(FirmwareError::io)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK];
    let mut total = 0u64;
    let mut last_mark = 0u64;
    loop {
        let read = file.read(&mut buf).map_err(FirmwareError::io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        total += read as u64;
        if progress_due(total, &mut last_mark, HASH_PROGRESS_EVERY, false) {
            if let Some(callback) = progress.as_mut() {
                callback(total);
            }
        }
    }
    if let Some(callback) = progress.as_mut() {
        callback(total);
    }
    Ok(hex_encode(&hasher.finalize()))
}

pub fn sha1_file(path: &Path) -> Result<String, FirmwareError> {
    let mut file = File::open(path).map_err(FirmwareError::io)?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; HASH_CHUNK];
    loop {
        let read = file.read(&mut buf).map_err(FirmwareError::io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_marks_every_interval() {
        let interval = 64;
        let mut last = 0u64;
        assert!(!progress_due(63, &mut last, interval, false));
        assert!(progress_due(64, &mut last, interval, false));
        assert!(!progress_due(127, &mut last, interval, false));
        assert!(progress_due(128, &mut last, interval, false));
        assert!(progress_due(128, &mut last, interval, true));
    }

    #[test]
    fn chunk_and_interval_match_the_bounds() {
        assert_eq!(hash_chunk_len(), 1024 * 1024);
        assert_eq!(hash_progress_interval(), 64 * 1024 * 1024);
    }
}
