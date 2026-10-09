// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Allow-list, block-list, and scan-only classification.
//!
//! A block-listed build is refused even for a scan. An allow-listed build
//! whose file hashes match, and which has been marked device-tested, may
//! be used for writes. Everything else is scan and read only.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::hashutil::hex_eq;
use crate::version::SdkVersion;
use crate::ToolsError;

const EMBEDDED: &str = include_str!("../../../data/platform_tools.toml");

/// Published SHA-1 of `platform-tools_r37.0.1-win.zip`.
pub const CANDIDATE_37_0_1_ZIP_SHA1: &str = "e03e78b1d80b396f1c3358e31251cb31740e1110";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKind {
    Windows,
    Other,
}

impl HostKind {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

/// Digests of the binaries in one platform-tools directory.
#[derive(Clone, Debug, Default)]
pub struct ToolFiles {
    pub adb_sha256: String,
    pub fastboot_sha256: String,
    pub dll_sha256s: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct AllowEntry {
    pub version: SdkVersion,
    pub zip_sha1: String,
    pub device_tested: bool,
    pub adb_sha256: String,
    pub fastboot_sha256: String,
    pub dll_sha256s: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct PlatformToolsPolicy {
    pub below: SdkVersion,
    pub ranges: Vec<(SdkVersion, SdkVersion)>,
    pub exact: Vec<SdkVersion>,
    pub allow: Vec<AllowEntry>,
    pub candidates: Vec<AllowEntry>,
}

impl PlatformToolsPolicy {
    pub fn embedded() -> Result<Self, ToolsError> {
        Self::from_toml(EMBEDDED)
    }

    pub fn from_toml(text: &str) -> Result<Self, ToolsError> {
        let file: PolicyFile =
            toml::from_str(text).map_err(|err| ToolsError::Policy(err.to_string()))?;
        let below = SdkVersion::parse(&file.block.below)?;
        let mut ranges = Vec::new();
        for range in file.block.ranges {
            ranges.push((
                SdkVersion::parse(&range.from)?,
                SdkVersion::parse(&range.to)?,
            ));
        }
        let mut exact = Vec::new();
        for item in file.block.exact {
            exact.push(SdkVersion::parse(&item.version)?);
        }
        Ok(Self {
            below,
            ranges,
            exact,
            allow: file
                .allow
                .into_iter()
                .map(AllowEntry::from_file)
                .collect::<Result<_, _>>()?,
            candidates: file
                .candidate
                .into_iter()
                .map(AllowEntry::from_file)
                .collect::<Result<_, _>>()?,
        })
    }

    pub fn zip_sha1_owner(&self, digest: &str) -> Option<&AllowEntry> {
        self.allow
            .iter()
            .chain(self.candidates.iter())
            .find(|entry| hex_eq(&entry.zip_sha1, digest))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolsVerdict {
    Allowed {
        version: SdkVersion,
    },
    Blocked {
        version: SdkVersion,
        message: String,
    },
    ScanOnly {
        version: SdkVersion,
        notice: String,
    },
}

impl ToolsVerdict {
    pub fn allows_scan(&self) -> bool {
        !matches!(self, Self::Blocked { .. })
    }

    pub fn allows_writes(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Allowed { .. } => "",
            Self::Blocked { message, .. } => message,
            Self::ScanOnly { notice, .. } => notice,
        }
    }

    pub fn version(&self) -> &SdkVersion {
        match self {
            Self::Allowed { version }
            | Self::Blocked { version, .. }
            | Self::ScanOnly { version, .. } => version,
        }
    }
}

pub fn classify(
    version: &SdkVersion,
    files: &ToolFiles,
    policy: &PlatformToolsPolicy,
    host: HostKind,
) -> ToolsVerdict {
    if let Some(message) = block_message(version, policy) {
        return ToolsVerdict::Blocked {
            version: version.clone(),
            message,
        };
    }
    if let Some(entry) = policy
        .allow
        .iter()
        .find(|entry| entry.version.triple() == version.triple() && entry.device_tested)
    {
        if hashes_match(entry, files, host) {
            return ToolsVerdict::Allowed {
                version: version.clone(),
            };
        }
        return ToolsVerdict::ScanOnly {
            version: version.clone(),
            notice: format!(
                "Platform-tools {version} is on the allow list but the file hashes differ. Scan and read only."
            ),
        };
    }
    if policy
        .candidates
        .iter()
        .any(|entry| entry.version.triple() == version.triple())
    {
        return ToolsVerdict::ScanOnly {
            version: version.clone(),
            notice: format!(
                "Platform-tools {version} matches a candidate that has not passed a device test. Scan and read only."
            ),
        };
    }
    ToolsVerdict::ScanOnly {
        version: version.clone(),
        notice: format!(
            "Untested platform-tools {version}. Scan and read only; flashing stays disabled until an allow-listed build is selected."
        ),
    }
}

fn block_message(version: &SdkVersion, policy: &PlatformToolsPolicy) -> Option<String> {
    let triple = version.triple();
    if triple < policy.below.triple() {
        return Some(format!(
            "Platform-tools {version} is older than {} and is on the block list. Flashwright will not use it.",
            policy.below
        ));
    }
    for (from, to) in &policy.ranges {
        if triple >= from.triple() && triple <= to.triple() {
            return Some(format!(
                "Platform-tools {version} is on the block list ({from} through {to}). Flashwright will not use it."
            ));
        }
    }
    for exact in &policy.exact {
        if triple == exact.triple() {
            return Some(format!(
                "Platform-tools {exact} is on the block list (all build ids). Flashwright will not use it."
            ));
        }
    }
    None
}

fn hashes_match(entry: &AllowEntry, files: &ToolFiles, host: HostKind) -> bool {
    if !hex_eq(&entry.adb_sha256, &files.adb_sha256)
        || !hex_eq(&entry.fastboot_sha256, &files.fastboot_sha256)
    {
        return false;
    }
    if host != HostKind::Windows {
        return true;
    }
    if entry.dll_sha256s.is_empty() {
        return false;
    }
    entry.dll_sha256s.iter().all(|(name, expected)| {
        files
            .dll_sha256s
            .get(name)
            .is_some_and(|actual| hex_eq(expected, actual))
    })
}

impl AllowEntry {
    fn from_file(file: AllowFile) -> Result<Self, ToolsError> {
        Ok(Self {
            version: SdkVersion::parse(&file.version)?,
            zip_sha1: file.zip_sha1,
            device_tested: file.device_tested,
            adb_sha256: file.adb_sha256,
            fastboot_sha256: file.fastboot_sha256,
            dll_sha256s: file.dll_sha256s,
        })
    }
}

#[derive(Debug, Deserialize)]
struct PolicyFile {
    block: BlockFile,
    #[serde(default)]
    allow: Vec<AllowFile>,
    #[serde(default)]
    candidate: Vec<AllowFile>,
}

#[derive(Debug, Deserialize)]
struct BlockFile {
    below: String,
    #[serde(default)]
    ranges: Vec<RangeFile>,
    #[serde(default)]
    exact: Vec<ExactFile>,
}

#[derive(Debug, Deserialize)]
struct RangeFile {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct ExactFile {
    version: String,
    #[serde(default = "yes")]
    #[allow(dead_code)]
    any_build: bool,
}

#[derive(Debug, Deserialize)]
struct AllowFile {
    version: String,
    zip_sha1: String,
    #[serde(default)]
    device_tested: bool,
    #[serde(default)]
    adb_sha256: String,
    #[serde(default)]
    fastboot_sha256: String,
    #[serde(default)]
    dll_sha256s: BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}
