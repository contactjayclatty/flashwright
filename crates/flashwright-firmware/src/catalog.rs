// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Codename aliases and the init_boot table. The alias pair aurora/eos is the
//! same table the device layer uses. komodo is a public Pixel 9 Pro XL fact.

use serde::Deserialize;

const ALIASES: &str = include_str!("../../../data/device_aliases.toml");
const DEVICES: &str = include_str!("../../../data/devices.toml");

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
    pub fn embedded() -> Result<Self, String> {
        let file: AliasFile = toml::from_str(ALIASES).map_err(|err| err.to_string())?;
        Ok(Self {
            groups: file
                .group
                .into_iter()
                .map(|group| AliasGroup {
                    canonical: group.canonical.to_ascii_lowercase(),
                    aliases: group
                        .aliases
                        .into_iter()
                        .map(|alias| alias.to_ascii_lowercase())
                        .collect(),
                })
                .collect(),
        })
    }

    pub fn canonical<'a>(&'a self, raw: &'a str) -> std::borrow::Cow<'a, str> {
        let folded = raw.to_ascii_lowercase();
        for group in &self.groups {
            if group.canonical == folded || group.aliases.iter().any(|alias| alias == &folded) {
                return std::borrow::Cow::Owned(group.canonical.clone());
            }
        }
        std::borrow::Cow::Owned(folded)
    }

    pub fn same(&self, left: &str, right: &str) -> bool {
        self.canonical(left) == self.canonical(right)
    }
}

#[derive(Clone, Debug)]
pub struct DeviceTable {
    rows: Vec<DeviceRow>,
}

#[derive(Clone, Debug)]
pub struct DeviceRow {
    pub codename: String,
    pub has_init_boot: bool,
}

impl DeviceTable {
    pub fn embedded() -> Result<Self, String> {
        let file: DeviceFile = toml::from_str(DEVICES).map_err(|err| err.to_string())?;
        Ok(Self {
            rows: file
                .device
                .into_iter()
                .map(|row| DeviceRow {
                    codename: row.codename.to_ascii_lowercase(),
                    has_init_boot: row.has_init_boot,
                })
                .collect(),
        })
    }

    pub fn has_init_boot(&self, codename: &str) -> Option<bool> {
        let folded = codename.to_ascii_lowercase();
        self.rows
            .iter()
            .find(|row| row.codename == folded)
            .map(|row| row.has_init_boot)
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
    has_init_boot: bool,
}
