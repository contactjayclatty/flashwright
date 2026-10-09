// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use serde::Deserialize;

use crate::device::{DeviceError, Partition};

const ALIASES: &str = include_str!("../../../../data/device_aliases.toml");
const DEVICES: &str = include_str!("../../../../data/devices.toml");
const KNOWN_BAD: &str = include_str!("../../../../data/known_bad_magisk.toml");
const MIN_BOOTLOADER: &str = include_str!("../../../../data/min_bootloader.toml");

#[derive(Clone, Debug)]
pub struct AliasTable {
    groups: Vec<AliasGroup>,
}

#[derive(Clone, Debug)]
struct AliasGroup {
    canonical: String,
    aliases: Vec<String>,
}

impl AliasTable {
    pub fn embedded() -> Result<Self, DeviceError> {
        Self::from_toml(ALIASES)
    }

    pub fn from_toml(text: &str) -> Result<Self, DeviceError> {
        let file: AliasFile =
            toml::from_str(text).map_err(|err| DeviceError::Catalogue(err.to_string()))?;
        Ok(Self {
            groups: file
                .group
                .into_iter()
                .map(|group| AliasGroup {
                    canonical: group.canonical,
                    aliases: group.aliases,
                })
                .collect(),
        })
    }

    /// Map a reported codename onto the canonical one. Unknown names pass through.
    pub fn canonical<'a>(&'a self, raw: &'a str) -> &'a str {
        for group in &self.groups {
            if group.canonical == raw || group.aliases.iter().any(|alias| alias == raw) {
                return &group.canonical;
            }
        }
        raw
    }
}

#[derive(Clone, Debug)]
pub struct DeviceRow {
    pub codename: String,
    pub model: Option<String>,
    pub has_init_boot: bool,
    pub patch_partition: Partition,
    /// Gate id from the device catalogue. Present when a boot-image mismatch is that phone's rule.
    pub boot_gate: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DeviceTable {
    rows: Vec<DeviceRow>,
}

impl DeviceTable {
    pub fn embedded() -> Result<Self, DeviceError> {
        Self::from_toml(DEVICES)
    }

    pub fn from_toml(text: &str) -> Result<Self, DeviceError> {
        let file: DeviceFile =
            toml::from_str(text).map_err(|err| DeviceError::Catalogue(err.to_string()))?;
        let rows = file
            .device
            .into_iter()
            .map(|row| {
                let patch_partition = parse_patch_partition(&row.patch_partition)?;
                Ok(DeviceRow {
                    codename: row.codename.to_ascii_lowercase(),
                    model: row.model,
                    has_init_boot: row.has_init_boot,
                    patch_partition,
                    boot_gate: row.boot_gate,
                })
            })
            .collect::<Result<Vec<_>, DeviceError>>()?;
        Ok(Self { rows })
    }

    pub fn get(&self, codename: &str) -> Option<&DeviceRow> {
        let folded = codename.to_ascii_lowercase();
        self.rows.iter().find(|row| row.codename == folded)
    }

    pub fn rows(&self) -> &[DeviceRow] {
        &self.rows
    }
}

fn parse_patch_partition(name: &str) -> Result<Partition, DeviceError> {
    match name {
        "boot" => Ok(Partition::Boot),
        "init_boot" => Ok(Partition::InitBoot),
        other => Err(DeviceError::Catalogue(format!(
            "unknown patch partition {other}"
        ))),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownBadMagisk {
    pub version_codes: Vec<u32>,
}

impl KnownBadMagisk {
    pub fn embedded() -> Result<Self, DeviceError> {
        let file: KnownBadFile =
            toml::from_str(KNOWN_BAD).map_err(|err| DeviceError::Catalogue(err.to_string()))?;
        Ok(Self {
            version_codes: file.version_codes,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinBootloaderEntry {
    pub codename: String,
    pub min: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinBootloaderTable {
    pub entries: Vec<MinBootloaderEntry>,
}

impl MinBootloaderTable {
    pub fn embedded() -> Result<Self, DeviceError> {
        let file: MinFile = toml::from_str(MIN_BOOTLOADER)
            .map_err(|err| DeviceError::Catalogue(err.to_string()))?;
        Ok(Self {
            entries: file
                .entries
                .into_iter()
                .map(|entry| MinBootloaderEntry {
                    codename: entry.codename,
                    min: entry.min,
                })
                .collect(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct AliasFile {
    #[serde(default)]
    group: Vec<AliasGroupFile>,
}

#[derive(Debug, Deserialize)]
struct AliasGroupFile {
    canonical: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceFile {
    #[serde(default)]
    device: Vec<DeviceRowFile>,
}

#[derive(Debug, Deserialize)]
struct DeviceRowFile {
    codename: String,
    #[serde(default)]
    model: Option<String>,
    has_init_boot: bool,
    patch_partition: String,
    #[serde(default)]
    boot_gate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KnownBadFile {
    #[serde(default)]
    version_codes: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct MinFile {
    #[serde(default)]
    entries: Vec<MinEntryFile>,
}

#[derive(Debug, Deserialize)]
struct MinEntryFile {
    codename: String,
    min: String,
}
