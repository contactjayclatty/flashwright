// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A/B slot. There is no "all" variant, so `--slot all` cannot be expressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn suffix(self) -> &'static str {
        match self {
            Self::A => "_a",
            Self::B => "_b",
        }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Partitions Phase 1 knows about. `Vbmeta` is read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Partition {
    Boot,
    InitBoot,
    Vbmeta,
    Bootloader,
    Radio,
}

impl Partition {
    pub fn fastboot_name(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::InitBoot => "init_boot",
            Self::Vbmeta => "vbmeta",
            Self::Bootloader => "bootloader",
            Self::Radio => "radio",
        }
    }

    pub fn is_writable(self) -> bool {
        !matches!(self, Self::Vbmeta)
    }
}

impl fmt::Display for Partition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.fastboot_name())
    }
}

/// Connection mode. `unauthorized` and `offline` are first-class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    Unrecognized,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Adb => "device",
            Self::Recovery => "recovery",
            Self::Sideload => "sideload",
            Self::Rescue => "rescue",
            Self::Fastboot => "fastboot",
            Self::Fastbootd => "fastbootd",
            Self::Unauthorized => "unauthorized",
            Self::NoPermissions => "no permissions",
            Self::Offline => "offline",
            Self::Unrecognized => "unrecognized",
        }
    }

    pub fn is_fastboot_family(self) -> bool {
        matches!(self, Self::Fastboot | Self::Fastbootd)
    }

    pub fn guidance(self) -> Option<&'static str> {
        match self {
            Self::Unauthorized => Some(
                "This phone is unauthorised. Unlock it and tap Allow on the USB debugging prompt, then scan again.",
            ),
            Self::Offline => Some("This phone is offline. Reconnect the cable and scan again."),
            Self::NoPermissions => Some(
                "The OS did not grant USB access to this phone. Reconnect the cable and scan again.",
            ),
            _ => None,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One row from `adb devices -l` or `fastboot devices -l`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanEntry {
    pub serial: String,
    pub mode: Mode,
    pub transport_id: Option<String>,
    pub raw_state: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockState {
    Unlocked,
    Locked,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootState {
    Rooted,
    RootUnknown { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitBootPresence {
    Present,
    Absent,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootTarget {
    InitBoot,
    Boot,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Battery {
    pub level: Option<u32>,
    pub charging: Option<bool>,
}

/// Fields for the device-info panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub serial: String,
    pub mode: Mode,
    pub model: Option<String>,
    pub codename: Option<String>,
    pub codename_raw: Option<String>,
    pub build_id: Option<String>,
    pub fingerprint: Option<String>,
    pub build_date_utc: Option<String>,
    pub sdk: Option<String>,
    pub spl: Option<String>,
    pub active_slot: Option<Slot>,
    pub lock: LockState,
    pub bootloader_version: Option<String>,
    pub root: RootState,
    pub magisk_version: Option<String>,
    pub magisk_code: Option<u32>,
    pub magisk_app_version: Option<String>,
    pub magisk_app_code: Option<u32>,
    pub init_boot: InitBootPresence,
    pub boot_target: BootTarget,
    pub battery: Option<Battery>,
    pub transport_id: Option<String>,
}

impl DeviceInfo {
    /// The other slot. A missing active slot is an error, never slot A.
    pub fn inactive_slot(&self) -> Result<Slot, crate::DeviceError> {
        self.active_slot
            .map(Slot::other)
            .ok_or(crate::DeviceError::UnknownSlot)
    }
}

/// Capability for write-class calls.
///
/// `mint` is public so tests can exercise write argv. Phase 1 plans are the
/// only production caller; `xtask` rejects other call sites.
#[derive(Debug)]
pub struct WriteToken {
    _private: (),
}

impl WriteToken {
    pub fn mint() -> Self {
        Self { _private: () }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebootTarget {
    Bootloader,
    Sideload,
    System,
}

impl fmt::Display for RebootTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bootloader => "bootloader",
            Self::Sideload => "sideload",
            Self::System => "system",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitTarget {
    /// Serial shows up in `fastboot devices` (bootloader or fastbootd).
    FastbootFamily,
    Mode(Mode),
    /// `adb get-state` is `device` and `sys.boot_completed` is `1`.
    SystemBooted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WaitOutcome {
    Reached,
    TimedOut,
    WrongMode { actual: Mode },
    Disappeared,
}

/// Poll interval and the §3.5 timeouts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportConfig {
    pub poll_interval: Duration,
    pub adb_to_bootloader: Duration,
    pub adb_to_sideload: Duration,
    pub bootloader_to_bootloader: Duration,
    pub bootloader_to_system: Duration,
    pub sideload_to_bootloader: Duration,
    pub prop_timeout: Duration,
    pub su_timeout: Duration,
    pub command_timeout: Duration,
}

impl TransportConfig {
    pub fn production() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            adb_to_bootloader: Duration::from_secs(90),
            adb_to_sideload: Duration::from_secs(120),
            bootloader_to_bootloader: Duration::from_secs(90),
            bootloader_to_system: Duration::from_secs(900),
            sideload_to_bootloader: Duration::from_secs(120),
            prop_timeout: Duration::from_secs(10),
            su_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(30),
        }
    }

    /// Same transitions, short enough for fake devices.
    pub fn for_tests() -> Self {
        Self {
            poll_interval: Duration::from_millis(5),
            adb_to_bootloader: Duration::from_millis(200),
            adb_to_sideload: Duration::from_millis(200),
            bootloader_to_bootloader: Duration::from_millis(200),
            bootloader_to_system: Duration::from_millis(200),
            sideload_to_bootloader: Duration::from_millis(200),
            prop_timeout: Duration::from_secs(10),
            su_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(2),
        }
    }
}

pub const MAX_PARALLEL_PROBES: usize = 4;
