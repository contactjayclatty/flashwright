// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Reuse a patched image when the same stock image and Magisk code already passed.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

use crate::gates::check_patched_sha1;
use crate::image::is_hex;
use crate::MagiskError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchCacheMeta {
    pub stock_sha1: String,
    pub stock_sha256: String,
    pub magisk_version: String,
    pub magisk_code: u32,
    pub method: String,
    pub patched_sha1: String,
    pub patched_sha256: String,
    pub config_sha1: String,
    pub created_utc: String,
}

/// Store one patched image. A later call can offer it again after G09.
pub fn store(root: &Path, meta: &PatchCacheMeta, image: &[u8]) -> Result<PathBuf, MagiskError> {
    if !is_hex(&meta.stock_sha1, 40) || !is_hex(&meta.patched_sha256, 64) {
        return Err(MagiskError::PatchedSha1);
    }
    check_patched_sha1(
        &meta.stock_sha1,
        &meta.stock_sha256,
        &meta.patched_sha256,
        &meta.config_sha1,
    )?;
    if !sha256_hex(image).eq_ignore_ascii_case(&meta.patched_sha256) {
        return Err(MagiskError::PatchedSha1);
    }
    let dir = root
        .join(&meta.stock_sha1)
        .join(meta.magisk_code.to_string());
    fs::create_dir_all(&dir).map_err(|err| MagiskError::Message(err.to_string()))?;
    let image_path = dir.join(format!("{}.img", meta.patched_sha256));
    let meta_path = dir.join("meta.json");
    fs::write(&image_path, image).map_err(|err| MagiskError::Message(err.to_string()))?;
    let body =
        serde_json::to_vec_pretty(meta).map_err(|err| MagiskError::Message(err.to_string()))?;
    fs::write(meta_path, body).map_err(|err| MagiskError::Message(err.to_string()))?;
    Ok(image_path)
}

/// Return the cached image when it still matches the stock SHA-1.
pub fn offer(
    root: &Path,
    stock_sha1: &str,
    stock_sha256: &str,
    magisk_code: u32,
) -> Result<Option<PathBuf>, MagiskError> {
    if !is_hex(stock_sha1, 40) {
        return Err(MagiskError::PatchedSha1);
    }
    let dir = root.join(stock_sha1).join(magisk_code.to_string());
    let meta_path = dir.join("meta.json");
    if !meta_path.is_file() {
        return Ok(None);
    }
    let body = fs::read(&meta_path).map_err(|err| MagiskError::Message(err.to_string()))?;
    let meta: PatchCacheMeta =
        serde_json::from_slice(&body).map_err(|err| MagiskError::Message(err.to_string()))?;
    if meta.stock_sha1 != stock_sha1 || meta.magisk_code != magisk_code {
        return Ok(None);
    }
    let image_path = dir.join(format!("{}.img", meta.patched_sha256));
    if !image_path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&image_path).map_err(|err| MagiskError::Message(err.to_string()))?;
    if !sha256_hex(&bytes).eq_ignore_ascii_case(&meta.patched_sha256) {
        return Ok(None);
    }
    check_patched_sha1(
        stock_sha1,
        stock_sha256,
        &meta.patched_sha256,
        &meta.config_sha1,
    )?;
    Ok(Some(image_path))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
