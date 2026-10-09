// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Unpack a platform-tools zip the user downloaded from Google.
//!
//! The zip SHA-1 must match an allow-list or candidate entry. Flashwright
//! does not download the zip and does not accept Google's terms.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use zip::ZipArchive;

use crate::hashutil::sha1_bytes;
use crate::policy::PlatformToolsPolicy;
use crate::version::SdkVersion;
use crate::ToolsError;

const MAX_ENTRY: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ImportReport {
    pub version: SdkVersion,
    pub directory: PathBuf,
    pub sha1: String,
    pub device_tested: bool,
}

pub fn import_platform_tools_zip(
    bytes: &[u8],
    dest_root: &Path,
    policy: &PlatformToolsPolicy,
) -> Result<ImportReport, ToolsError> {
    let digest = sha1_bytes(bytes);
    let entry = policy
        .zip_sha1_owner(&digest)
        .ok_or_else(|| ToolsError::ZipChecksum {
            got: digest.clone(),
        })?;
    let version = entry.version.clone();
    let device_tested = entry.device_tested;
    let directory = dest_root.join(version.to_string());
    extract(bytes, &directory)?;
    Ok(ImportReport {
        version,
        directory,
        sha1: digest,
        device_tested,
    })
}

fn extract(bytes: &[u8], directory: &Path) -> Result<(), ToolsError> {
    if directory.exists() {
        fs::remove_dir_all(directory).map_err(|err| ToolsError::io(directory, err))?;
    }
    fs::create_dir_all(directory).map_err(|err| ToolsError::io(directory, err))?;
    let cursor = io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|err| ToolsError::Zip(err.to_string()))?;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| ToolsError::Zip(err.to_string()))?;
        let raw_name = entry
            .name()
            .map(|value| value.into_owned())
            .unwrap_or_default();
        let name = entry
            .enclosed_name()
            .ok_or(ToolsError::ZipPath { name: raw_name })?;
        let relative = safe_relative(&name)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = directory.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|err| ToolsError::io(&target, err))?;
            continue;
        }
        let size = entry.size();
        if size > MAX_ENTRY || total.saturating_add(size) > MAX_TOTAL {
            return Err(ToolsError::ZipTooLarge {
                name: relative.display().to_string(),
            });
        }
        total = total.saturating_add(size);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| ToolsError::io(parent, err))?;
        }
        let mut output = File::create(&target).map_err(|err| ToolsError::io(&target, err))?;
        io::copy(&mut entry, &mut output).map_err(|err| ToolsError::io(&target, err))?;
        output.flush().map_err(|err| ToolsError::io(&target, err))?;
    }
    Ok(())
}

pub fn safe_relative(path: &Path) -> Result<PathBuf, ToolsError> {
    if path.is_absolute() {
        return Err(ToolsError::ZipPath {
            name: path.display().to_string(),
        });
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => {
                return Err(ToolsError::ZipPath {
                    name: path.display().to_string(),
                });
            }
        }
    }
    Ok(out)
}
