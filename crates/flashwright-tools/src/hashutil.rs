// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha1::Sha1;
use sha2::Digest;
use sha2::Sha256;

use crate::ToolsError;

pub fn sha256_bytes(bytes: &[u8]) -> String {
    encode(&Sha256::digest(bytes))
}

pub fn sha1_bytes(bytes: &[u8]) -> String {
    encode(&Sha1::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String, ToolsError> {
    let mut file = File::open(path).map_err(|err| ToolsError::io(path, err))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|err| ToolsError::io(path, err))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(encode(&hasher.finalize()))
}

pub fn hex_eq(left: &str, right: &str) -> bool {
    !left.is_empty() && left.eq_ignore_ascii_case(right)
}

fn encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}
