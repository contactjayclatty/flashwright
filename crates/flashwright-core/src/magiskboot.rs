// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! PC-side check of a patched init_boot image.
//!
//! `magiskboot` is not bundled. The caller passes an absolute path and the
//! SHA-256 of that file. This module runs `unpack`, then asks the ramdisk
//! whether Magisk's init is present and reads the stock SHA-1 Magisk stored.

use std::path::Path;
use std::time::Duration;

use thiserror::Error;

use crate::exe::{self, VerifiedExe};
use crate::proc::{CommandRunner, Invocation, ProcError};

const UNPACK_TIMEOUT: Duration = Duration::from_secs(60);
const KNOWN_FORMATS: &[&str] = &["gzip", "lz4", "lz4_legacy", "lz4_lg", "cpio"];

/// What `magiskboot` reported about a patched init_boot image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootInspection {
    pub ramdisk_format: String,
    pub magisk_init: bool,
    pub config_sha1: String,
}

#[derive(Debug, Error)]
pub enum MagiskbootError {
    #[error("{0}")]
    Tool(String),

    #[error("unsupported ramdisk format")]
    UnsupportedRamdisk,

    #[error("patched init_boot has no Magisk init")]
    NoMagiskInit,

    #[error("patched init_boot is missing the stock SHA-1")]
    MissingSha1,

    #[error("{0}")]
    Run(String),
}

impl From<ProcError> for MagiskbootError {
    fn from(err: ProcError) -> Self {
        Self::Tool(err.to_string())
    }
}

/// Resolve magiskboot by absolute path and SHA-256. A mismatch does not run it.
pub fn resolve(path: &Path, expected_sha256: &str) -> Result<VerifiedExe, MagiskbootError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let stem = name
        .strip_suffix(".exe")
        .unwrap_or(name)
        .to_ascii_lowercase();
    if stem != "magiskboot" {
        return Err(MagiskbootError::Tool(
            "the PC check only runs a file named magiskboot".into(),
        ));
    }
    Ok(exe::managed_tool(path, expected_sha256)?)
}

/// Unpack `image` with the resolved magiskboot and read the ramdisk report.
///
/// `work` is the directory the tool writes into. Tests can stand in for the
/// tool; a real magiskboot is only used when the caller points at one.
pub async fn inspect_patched_init_boot<R: CommandRunner>(
    runner: &R,
    tool: &VerifiedExe,
    image: &Path,
    work: &Path,
) -> Result<BootInspection, MagiskbootError> {
    if !image.is_absolute() {
        return Err(MagiskbootError::Tool(
            "the patched image path must be absolute".into(),
        ));
    }
    let image = image.to_path_buf();
    let unpack = run_tool(runner, tool, work, vec!["unpack".into(), path_arg(&image)]).await?;
    if !unpack.success_exit() {
        return Err(MagiskbootError::Run(tail(&unpack)));
    }
    let format = ramdisk_format(&combined(&unpack)).ok_or(MagiskbootError::UnsupportedRamdisk)?;
    if !known_format(&format) {
        return Err(MagiskbootError::UnsupportedRamdisk);
    }

    let test = run_tool(
        runner,
        tool,
        work,
        vec!["cpio".into(), "ramdisk.cpio".into(), "test".into()],
    )
    .await?;
    if !test.success_exit() || !combined(&test).contains("Magisk detected") {
        return Err(MagiskbootError::NoMagiskInit);
    }

    let exists = run_tool(
        runner,
        tool,
        work,
        vec![
            "cpio".into(),
            "ramdisk.cpio".into(),
            "exists".into(),
            "init".into(),
        ],
    )
    .await?;
    if !exists.success_exit() {
        return Err(MagiskbootError::NoMagiskInit);
    }

    let extracted = work.join("magisk-config");
    let extract = run_tool(
        runner,
        tool,
        work,
        vec![
            "cpio".into(),
            "ramdisk.cpio".into(),
            "extract".into(),
            ".backup/.magisk".into(),
            path_arg(&extracted),
        ],
    )
    .await?;
    if !extract.success_exit() {
        return Err(MagiskbootError::MissingSha1);
    }
    let config = std::fs::read_to_string(&extracted).unwrap_or_default();
    let sha1 = config_sha1(&config)
        .or_else(|| config_sha1(&combined(&extract)))
        .ok_or(MagiskbootError::MissingSha1)?;
    Ok(BootInspection {
        ramdisk_format: format,
        magisk_init: true,
        config_sha1: sha1,
    })
}

pub fn known_format(format: &str) -> bool {
    KNOWN_FORMATS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(format))
}

fn ramdisk_format(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(rest) = line.split("RAMDISK_FMT").nth(1) else {
            continue;
        };
        let start = rest.find('[')?;
        let end = rest[start + 1..].find(']')?;
        let format = rest[start + 1..start + 1 + end].trim();
        if !format.is_empty() {
            return Some(format.to_string());
        }
    }
    None
}

fn config_sha1(text: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("SHA1=") else {
            continue;
        };
        let rest = rest.trim();
        if rest.len() == 40 && rest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            if found.is_some() && found.as_deref() != Some(rest) {
                return None;
            }
            found = Some(rest.to_ascii_lowercase());
        }
    }
    found
}

async fn run_tool<R: CommandRunner>(
    runner: &R,
    tool: &VerifiedExe,
    work: &Path,
    args: Vec<String>,
) -> Result<crate::proc::RunResult, MagiskbootError> {
    let invocation = Invocation::tied(tool.path(), args, UNPACK_TIMEOUT)
        .with_watchdog(Duration::from_secs(30))
        .with_current_dir(work);
    runner
        .run(invocation)
        .await
        .map_err(|err| MagiskbootError::Run(err.to_string()))
}

fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

fn combined(result: &crate::proc::RunResult) -> String {
    format!("{}{}", result.stdout_text(), result.stderr_text())
}

fn tail(result: &crate::proc::RunResult) -> String {
    let text = combined(result);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        format!("magiskboot exited {:?}", result.exit_code)
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::proc::{ScriptedResponse, ScriptedRunner};
    use sha2::{Digest, Sha256};

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

    fn tool_file(dir: &Path, body: &[u8]) -> PathBuf {
        let path = dir.join(if cfg!(windows) {
            "magiskboot.exe"
        } else {
            "magiskboot"
        });
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn hash_mismatch_does_not_resolve() {
        let dir = std::env::temp_dir().join("flashwright-magiskboot-hash");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = tool_file(&dir, b"not-a-real-magiskboot");
        let digest = hex(Sha256::digest(b"not-a-real-magiskboot"));
        assert!(resolve(&path, &digest).is_ok());
        let mut wrong = digest.clone();
        wrong.replace_range(0..2, "00");
        if wrong == digest {
            wrong.replace_range(0..2, "ff");
        }
        assert!(resolve(&path, &wrong).is_err());
        assert!(resolve(Path::new("magiskboot"), &digest).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn inspects_a_scripted_unpack() {
        let dir = std::env::temp_dir().join("flashwright-magiskboot-inspect");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tool_path = tool_file(&dir, b"magiskboot-stand-in");
        let digest = hex(Sha256::digest(b"magiskboot-stand-in"));
        let tool = resolve(&tool_path, &digest).unwrap();
        let image = dir.join("init_boot.img");
        std::fs::write(&image, b"ANDROID!synthetic").unwrap();
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let sha1 = "0123456789abcdef0123456789abcdef01234567";
        let runner = ScriptedRunner::new();
        let name = tool_path.file_name().unwrap().to_str().unwrap();
        runner.on(
            name,
            &["unpack"],
            ScriptedResponse::ok("Parsing boot image: [init_boot.img]\nRAMDISK_FMT     [gzip]\n"),
        );
        runner.on(
            name,
            &["cpio", "ramdisk.cpio", "test"],
            ScriptedResponse::ok("Loading cpio: [ramdisk.cpio]\nMagisk detected\n"),
        );
        runner.on(
            name,
            &["cpio", "ramdisk.cpio", "exists", "init"],
            ScriptedResponse::ok(""),
        );
        runner.on(
            name,
            &["cpio", "ramdisk.cpio", "extract"],
            ScriptedResponse::ok(format!("SHA1={sha1}\n")),
        );
        let report = inspect_patched_init_boot(&runner, &tool, &image, &work)
            .await
            .unwrap();
        assert_eq!(report.ramdisk_format, "gzip");
        assert!(report.magisk_init);
        assert_eq!(report.config_sha1, sha1);
        let calls = runner.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].args[0], "unpack");
        assert!(calls[0].args[1].ends_with("init_boot.img"));
        assert_eq!(calls[2].args[3], "init");
        assert!(calls
            .iter()
            .all(|call| call.current_dir.as_deref() == Some(work.as_path())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn unknown_format_fails_closed() {
        let dir = std::env::temp_dir().join("flashwright-magiskboot-xz");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tool_path = tool_file(&dir, b"magiskboot-stand-in-xz");
        let digest = hex(Sha256::digest(b"magiskboot-stand-in-xz"));
        let tool = resolve(&tool_path, &digest).unwrap();
        let image = dir.join("init_boot.img");
        std::fs::write(&image, b"ANDROID!synthetic").unwrap();
        let runner = ScriptedRunner::new();
        let name = tool_path.file_name().unwrap().to_str().unwrap();
        runner.on(
            name,
            &["unpack"],
            ScriptedResponse::ok("RAMDISK_FMT     [xz]\n"),
        );
        let err = inspect_patched_init_boot(&runner, &tool, &image, &dir)
            .await
            .unwrap_err();
        assert!(matches!(err, MagiskbootError::UnsupportedRamdisk));
        assert_eq!(runner.calls().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
