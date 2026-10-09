// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Resolved executables. Only this module constructs a [`VerifiedExe`].

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::proc::ProcError;

/// How a path became a [`VerifiedExe`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trust {
    /// Hashed regular file whose name is adb or fastboot. Writes stay off
    /// until an allow-list entry matches.
    Measured,
    /// Digest matches an allow-list entry.
    AllowListed,
    /// The path is not on disk. Only the scripted runner accepts this.
    Scripted,
}

/// Absolute path plus the SHA-256 measured for it.
#[derive(Clone, Debug)]
pub struct VerifiedExe {
    path: PathBuf,
    sha256: String,
    trust: Trust,
    file_id: Option<FileId>,
}

/// Identity of the opened file. A later run must see the same inode, one link,
/// and the allow-list digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileId {
    dev: u64,
    ino: u64,
    nlink: u64,
}

impl VerifiedExe {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub(crate) fn trust(&self) -> Trust {
        self.trust
    }

    /// Real spawn accepts only an allow-listed adb or fastboot.
    pub(crate) fn admits_system_spawn(&self) -> bool {
        self.trust == Trust::AllowListed && is_platform_tool(&file_name(&self.path))
    }
}

#[derive(Clone, Debug)]
pub struct ListenerImage {
    pub path: PathBuf,
    pub sha256: String,
}

/// Hash a regular adb or fastboot file without consulting the allow list.
///
/// Symlinks fail `O_NOFOLLOW`. Hard links and any other name, including a
/// helper that points at a shell or an interpreter, are refused. A measured
/// file still cannot run until [`platform_tool`] matches the allow list.
pub(crate) fn measure_platform_tool(path: &Path) -> Result<VerifiedExe, ProcError> {
    reject_path(path)?;
    let name = file_name(path);
    if !is_platform_tool(&name) {
        return Err(ProcError::Unverified {
            detail: format!("{name} is not an allow-listed platform-tools path"),
        });
    }
    let mut file = open_share_read(path)?;
    let file_id = file_identity(&file)?;
    if file_id.nlink != 1 {
        return Err(ProcError::Unverified {
            detail: format!("{name} is a hard link"),
        });
    }
    let sha256 = hash_file(&mut file)?;
    Ok(VerifiedExe {
        path: path.to_path_buf(),
        sha256,
        trust: Trust::Measured,
        file_id: Some(file_id),
    })
}

/// Re-open the file and compare it with the allow-list digest.
///
/// Scans and writes both use this. A measured file, a hard link, a replaced
/// copy, or a hash that no longer matches the allow list is refused.
pub(crate) fn recheck_allow_list(exe: &VerifiedExe) -> Result<(), ProcError> {
    if !exe.admits_system_spawn() {
        return Err(ProcError::Unverified {
            detail: "only an allow-listed adb or fastboot may run".into(),
        });
    }
    let Some(expected) = exe.file_id else {
        return Err(ProcError::Unverified {
            detail: "only an allow-listed adb or fastboot may run".into(),
        });
    };
    let mut file = open_share_read(exe.path())?;
    let file_id = file_identity(&file)?;
    if file_id.nlink != 1 {
        return Err(ProcError::Unverified {
            detail: "the platform-tools file is a hard link".into(),
        });
    }
    if file_id != expected {
        return Err(ProcError::Unverified {
            detail: "a copy replaced the allow-listed platform-tools file".into(),
        });
    }
    let sha256 = hash_file(&mut file)?;
    if !sha256.eq_ignore_ascii_case(exe.sha256()) {
        return Err(ProcError::Unverified {
            detail: "platform-tools file hash does not match the allow list".into(),
        });
    }
    Ok(())
}

/// Managed adb or fastboot whose digest matches the allow-list entry.
pub(crate) fn platform_tool(path: &Path, expected_sha256: &str) -> Result<VerifiedExe, ProcError> {
    let measured = measure_platform_tool(path)?;
    if !measured.sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(ProcError::Unverified {
            detail: "platform-tools file hash does not match the allow list".into(),
        });
    }
    Ok(VerifiedExe {
        path: measured.path,
        sha256: measured.sha256,
        trust: Trust::AllowListed,
        file_id: measured.file_id,
    })
}

/// Placeholder for a scripted path that is not on disk.
pub(crate) fn scripted_tool(path: &Path) -> Result<VerifiedExe, ProcError> {
    if !path.is_absolute() {
        return Err(ProcError::ProgramNotAbsolute);
    }
    let name = file_name(path);
    if is_forbidden_program_name(&name) {
        return Err(ProcError::ShellForbidden { name });
    }
    Ok(VerifiedExe {
        path: path.to_path_buf(),
        sha256: String::new(),
        trust: Trust::Scripted,
        file_id: None,
    })
}

/// Installed allow-listed tool, else a measured platform-tools file, else a
/// scripted placeholder when the path is absent.
pub(crate) fn resolve_tool(path: &Path) -> Result<VerifiedExe, ProcError> {
    if path.exists() {
        measure_platform_tool(path)
    } else {
        scripted_tool(path)
    }
}

fn is_forbidden_program_name(name: &str) -> bool {
    crate::proc::is_forbidden_program(name)
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

pub(crate) fn hash_regular_file(path: &Path) -> Result<String, ProcError> {
    let mut file = open_share_read(path)?;
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
        // FILE_SHARE_READ, and open the reparse point itself so a symlink or
        // junction is visible instead of its target.
        options.share_mode(1);
        options.custom_flags(0x0020_0000);
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
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE: u32 = 0x400;
        let meta = file.metadata()?;
        if meta.file_attributes() & REPARSE != 0 {
            return Err(ProcError::Unverified {
                detail: "reparse points are not used".into(),
            });
        }
    }
    Ok(file)
}

fn file_identity(file: &File) -> Result<FileId, ProcError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        Ok(FileId {
            dev: meta.dev(),
            ino: meta.ino(),
            nlink: meta.nlink(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        // SAFETY: the handle is the open file, and the struct is written only
        // on success before it is read.
        let info = unsafe {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info).map_err(|_| {
                ProcError::Unverified {
                    detail: "the platform-tools file could not be identified".into(),
                }
            })?;
            info
        };
        let ino = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Ok(FileId {
            dev: u64::from(info.dwVolumeSerialNumber),
            ino,
            nlink: u64::from(info.nNumberOfLinks),
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(ProcError::Unverified {
            detail: "the platform-tools file could not be identified".into(),
        })
    }
}
