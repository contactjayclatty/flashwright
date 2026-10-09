// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use serde::Deserialize;

use crate::UsbInterface;

const EMBEDDED: &str = include_str!("../../../data/usb_ids.toml");

#[derive(Debug, thiserror::Error)]
pub enum UsbError {
    #[error("usb id table is invalid: {0}")]
    Table(String),

    #[error("setupapi enumeration failed: {0}")]
    SetupApi(String),
}

#[derive(Clone, Debug)]
pub struct UsbTable {
    vids: Vec<String>,
    pids: Vec<PidRow>,
}

#[derive(Clone, Debug)]
struct PidRow {
    vid: String,
    pid: String,
    interface: UsbInterface,
}

impl UsbTable {
    pub fn embedded() -> Result<Self, UsbError> {
        Self::from_toml(EMBEDDED)
    }

    pub fn from_toml(text: &str) -> Result<Self, UsbError> {
        let file: UsbFile = toml::from_str(text).map_err(|err| UsbError::Table(err.to_string()))?;
        let mut pids = Vec::new();
        for row in file.pid {
            pids.push(PidRow {
                vid: row.vid.to_ascii_uppercase(),
                pid: row.pid.to_ascii_uppercase(),
                interface: match row.interface.as_str() {
                    "adb" => UsbInterface::Adb,
                    "bootloader" => UsbInterface::Bootloader,
                    _ => UsbInterface::Unknown,
                },
            });
        }
        Ok(Self {
            vids: file
                .vid
                .into_iter()
                .map(|row| row.vid.to_ascii_uppercase())
                .collect(),
            pids,
        })
    }

    pub fn knows_vid(&self, vid: &str) -> bool {
        let vid = vid.to_ascii_uppercase();
        self.vids.iter().any(|known| known == &vid)
    }

    pub fn interface_for(&self, vid: &str, pid: &str) -> UsbInterface {
        let vid = vid.to_ascii_uppercase();
        let pid = pid.to_ascii_uppercase();
        self.pids
            .iter()
            .find(|row| row.vid == vid && row.pid == pid)
            .map(|row| row.interface)
            .unwrap_or(UsbInterface::Unknown)
    }
}

#[derive(Debug, Deserialize)]
struct UsbFile {
    #[serde(default)]
    vid: Vec<VidFile>,
    #[serde(default)]
    pid: Vec<PidFile>,
}

#[derive(Debug, Deserialize)]
struct VidFile {
    vid: String,
}

#[derive(Debug, Deserialize)]
struct PidFile {
    vid: String,
    pid: String,
    interface: String,
}
