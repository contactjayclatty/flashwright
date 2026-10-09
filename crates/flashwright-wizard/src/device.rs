// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use serde::Serialize;

use crate::error::CoreError;

/// Boot slot. There is no "all" variant, so a write cannot name every slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }
}

/// The inactive slot is the other one. An unknown active slot is an error.
pub fn inactive_slot(active: Option<Slot>) -> Result<Slot, CoreError> {
    active
        .map(Slot::other)
        .ok_or_else(|| CoreError::message("The active slot is unknown."))
}

/// String compare used for the unlock check. `"0"` means unlocked.
pub fn unlocked_from_props(flash_locked: &str, verified_boot_state: &str) -> bool {
    flash_locked == "0" || verified_boot_state == "orange"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Adb,
    Recovery,
    Sideload,
    Rescue,
    Fastboot,
    Fastbootd,
    Unauthorized,
    NoPermissions,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Partition {
    Boot,
    InitBoot,
    Vbmeta,
    Bootloader,
    Radio,
}

impl Partition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::InitBoot => "init_boot",
            Self::Vbmeta => "vbmeta",
            Self::Bootloader => "bootloader",
            Self::Radio => "radio",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceSummary {
    pub serial: String,
    pub mode: Mode,
    pub model: String,
    pub codename: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceInfo {
    pub serial: String,
    pub mode: Mode,
    pub model: String,
    pub codename: String,
    pub build_id: String,
    pub fingerprint: String,
    pub spl: String,
    pub active_slot: Option<Slot>,
    pub bootloader_unlocked: bool,
    pub bootloader_version: String,
    pub root_present: bool,
    pub root_tool_version: String,
    pub root_tool_code: u32,
    pub uses_init_boot: bool,
    pub battery_percent: u8,
    pub battery_charging: bool,
}

/// Read-class device operations. A later build can supply a real transport.
/// This build keeps every phone behind the sample transport.
pub trait DeviceTransport: Send + Sync {
    fn list(&self) -> Result<Vec<DeviceSummary>, CoreError>;
    fn info(&self, serial: &str) -> Result<DeviceInfo, CoreError>;
}

/// Write-class device operations. Only a later plan executor may call these.
/// The sample transport refuses every call.
pub trait DeviceWrite: Send + Sync {
    fn flash(
        &self,
        serial: &str,
        slot: Slot,
        partition: Partition,
        image: &str,
    ) -> Result<(), CoreError>;
}
