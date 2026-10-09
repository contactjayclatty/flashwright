// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Resolved executables. Only this module constructs a [`VerifiedExe`].

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::proc::ProcError;

/// Absolute path plus the SHA-256 measured for it.
pub struct VerifiedExe {
    path: PathBuf,
    sha256: String,
}

impl VerifiedExe {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Debug)]
pub struct ListenerImage {
    pub path: PathBuf,
    pub sha256: String,
}

/// Host utility such as `echo` or `where.exe`. Platform-tools names are refused.
pub(crate) fn host_utility(path: &Path) -> Result<VerifiedExe, ProcError> {
    reject_path(path)?;
    let name = file_name(path);
    if is_platform_tool(&name) {
        return Err(ProcError::Unverified {
            detail: format!("{name} must be a verified platform-tools binary"),
        });
    }
    let sha256 = hash_path(path)?;
    Ok(VerifiedExe {
        path: path.to_path_buf(),
        sha256,
    })
}

/// A user-supplied tool, checked by absolute path and SHA-256.
///
/// Platform-tools names are refused here. Those stay on the allow list.
/// The file is not searched for on `PATH`.
pub fn managed_tool(path: &Path, expected_sha256: &str) -> Result<VerifiedExe, ProcError> {
    reject_path(path)?;
    let name = file_name(path);
    if is_platform_tool(&name) {
        return Err(ProcError::Unverified {
            detail: format!("{name} is resolved through the platform-tools allow list"),
        });
    }
    if expected_sha256.len() != 64 || !expected_sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(ProcError::Unverified {
            detail: "expected SHA-256 must be 64 hex digits".into(),
        });
    }
    let sha256 = hash_path(path)?;
    if !sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(ProcError::Unverified {
            detail: "file hash does not match".into(),
        });
    }
    Ok(VerifiedExe {
        path: path.to_path_buf(),
        sha256,
    })
}

/// Managed adb or fastboot whose digest matches the allow-list entry.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn platform_tool(path: &Path, expected_sha256: &str) -> Result<VerifiedExe, ProcError> {
    reject_path(path)?;
    let sha256 = hash_path(path)?;
    if !sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(ProcError::Unverified {
            detail: "platform-tools file hash does not match the allow list".into(),
        });
    }
    Ok(VerifiedExe {
        path: path.to_path_buf(),
        sha256,
    })
}

/// Open each path read-only and shared for read, then hash through the handle.
pub struct SharedReadLocks {
    files: Vec<File>,
}

impl SharedReadLocks {
    pub fn hold(paths: &[PathBuf]) -> Result<Self, ProcError> {
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            reject_path(path)?;
            files.push(open_share_read(path)?);
        }
        Ok(Self { files })
    }

    pub fn hashes(&mut self) -> Result<Vec<String>, ProcError> {
        let mut out = Vec::with_capacity(self.files.len());
        for file in &mut self.files {
            out.push(hash_file(file)?);
        }
        Ok(out)
    }
}

pub fn adb_server_matches(
    expected: &VerifiedExe,
    listener: Option<&ListenerImage>,
) -> Result<(), String> {
    let Some(listener) = listener else {
        return Err("no adb server is listening on 127.0.0.1:5037".into());
    };
    if listener.path != expected.path() || !listener.sha256.eq_ignore_ascii_case(expected.sha256())
    {
        return Err("the adb server on port 5037 is not Flashwright's verified adb".into());
    }
    Ok(())
}

fn reject_path(path: &Path) -> Result<(), ProcError> {
    if !path.is_absolute() {
        return Err(ProcError::ProgramNotAbsolute);
    }
    let text = path.to_string_lossy();
    if text.starts_with(r"\\") || text.starts_with("//") {
        return Err(ProcError::Unverified {
            detail: "UNC paths are not used".into(),
        });
    }
    if has_ads(&text) {
        return Err(ProcError::Unverified {
            detail: "alternate data streams are not used".into(),
        });
    }
    let name = file_name(path);
    if crate::proc::is_forbidden_program(&name) {
        return Err(ProcError::ShellForbidden { name });
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string()
}

fn is_platform_tool(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "adb" | "adb.exe" | "fastboot" | "fastboot.exe"
    )
}

fn has_ads(text: &str) -> bool {
    let rest = text.strip_prefix(r"\\?\").unwrap_or(text);
    let rest = if rest.len() >= 2
        && rest.as_bytes()[0].is_ascii_alphabetic()
        && rest.as_bytes()[1] == b':'
    {
        &rest[2..]
    } else {
        rest
    };
    rest.contains(':')
}

fn hash_path(path: &Path) -> Result<String, ProcError> {
    let mut file = File::open(path).map_err(|source| ProcError::spawn(path, source))?;
    hash_file(&mut file)
}

fn hash_file(file: &mut File) -> Result<String, ProcError> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buf)?;
        if count == 0 {
            break;
        }
        hasher.update(&buf[..count]);
    }
    Ok(hex(hasher.finalize()))
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn open_share_read(path: &Path) -> Result<File, ProcError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let file = options
        .open(path)
        .map_err(|source| ProcError::spawn(path, source))?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH) };
        if rc != 0 {
            return Err(ProcError::Io(std::io::Error::last_os_error()));
        }
    }
    Ok(file)
}
