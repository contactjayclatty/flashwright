// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Typed device command catalogue.
//!
//! There is no free-form shell string. Read variants do not change device
//! state. Write variants are executed only with a write token.

mod newtypes;
mod render;

pub(crate) use newtypes::MAX_BLOCK_LEN;
pub use newtypes::{
    AssetRef, ByNameRoot, ByteLen, CmdError, DeviceSerial, DumpsysService, FastbootVar, HostRef,
    ImageRef, PackageName, PropName, PullName, ValidatedDevicePath, VerifiedHostFile, WorkFile,
    MAGISK_PACKAGE,
};
pub use render::{cleanup_argv, read_argv, sh_quote, write_argv, Rendered, Tool};

use serde::{Deserialize, Serialize};

use crate::device::{Partition, RebootTarget, Slot};

/// Phone reboot mode. There is no "all" and no wipe.
pub type RebootMode = RebootTarget;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadCmd {
    AdbHost(AdbHostRead),
    AdbShell(AdbShellRead),
    Su(SuRead),
    ExecOutSu(ExecOutSuRead),
    Fastboot(FastbootRead),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdbHostRead {
    Version,
    Devices,
    GetState {
        serial: DeviceSerial,
    },
    Pull {
        serial: DeviceSerial,
        remote: PullRemote,
        dst_name: PullName,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PullRemote {
    Validated(ValidatedDevicePath),
    Work(WorkFile),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdbShellRead {
    GetpropAll {
        serial: DeviceSerial,
    },
    Getprop {
        serial: DeviceSerial,
        name: PropName,
    },
    LsBlockByName {
        serial: DeviceSerial,
        root: ByNameRoot,
        partition: Partition,
        slot: Slot,
    },
    LsWorkDir {
        serial: DeviceSerial,
    },
    DumpsysBattery {
        serial: DeviceSerial,
    },
    DumpsysDiskstats {
        serial: DeviceSerial,
    },
    DumpsysPackage {
        serial: DeviceSerial,
        package: PackageName,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuRead {
    Id {
        serial: DeviceSerial,
    },
    MagiskVersion {
        serial: DeviceSerial,
    },
    MagiskVersionCode {
        serial: DeviceSerial,
    },
    LsMagiskDir {
        serial: DeviceSerial,
    },
    Sha256Block {
        serial: DeviceSerial,
        root: ByNameRoot,
        partition: Partition,
        slot: Slot,
    },
    Sha256BlockPrefix {
        serial: DeviceSerial,
        root: ByNameRoot,
        partition: Partition,
        slot: Slot,
        len: ByteLen,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecOutSuRead {
    CatBlock {
        serial: DeviceSerial,
        root: ByNameRoot,
        partition: Partition,
        slot: Slot,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FastbootRead {
    Devices,
    GetvarAll {
        serial: DeviceSerial,
    },
    Getvar {
        serial: DeviceSerial,
        var: FastbootVar,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteCmd {
    AdbHost(AdbHostWrite),
    AdbShell(AdbShellWrite),
    Su(SuWrite),
    Fastboot(FastbootWrite),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdbHostWrite {
    Push {
        serial: DeviceSerial,
        src: HostRef,
        dst: WorkFile,
    },
    Reboot {
        serial: DeviceSerial,
        mode: RebootMode,
    },
    Sideload {
        serial: DeviceSerial,
        package: ImageRef,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdbShellWrite {
    MakeWorkDir { serial: DeviceSerial },
    RunPatchScript { serial: DeviceSerial },
}

/// Fixed cleanup. This is not a write: it only removes the work directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CleanupCmd {
    RemoveWorkDir { serial: DeviceSerial },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuWrite {
    RunPatchScript { serial: DeviceSerial },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FastbootWrite {
    Flash {
        serial: DeviceSerial,
        slot: Slot,
        partition: Partition,
        image: ImageRef,
    },
    SetActive {
        serial: DeviceSerial,
        slot: Slot,
    },
    Update {
        serial: DeviceSerial,
        slot: Slot,
        package: ImageRef,
    },
    Reboot {
        serial: DeviceSerial,
        mode: RebootMode,
    },
}

impl ReadCmd {
    pub fn serial(&self) -> Option<&DeviceSerial> {
        match self {
            Self::AdbHost(AdbHostRead::Version | AdbHostRead::Devices) => None,
            Self::AdbHost(AdbHostRead::GetState { serial } | AdbHostRead::Pull { serial, .. }) => {
                Some(serial)
            }
            Self::AdbShell(cmd) => Some(cmd.serial()),
            Self::Su(cmd) => Some(cmd.serial()),
            Self::ExecOutSu(ExecOutSuRead::CatBlock { serial, .. }) => Some(serial),
            Self::Fastboot(FastbootRead::Devices) => None,
            Self::Fastboot(
                FastbootRead::GetvarAll { serial } | FastbootRead::Getvar { serial, .. },
            ) => Some(serial),
        }
    }

    pub fn uses_fastboot(&self) -> bool {
        matches!(self, Self::Fastboot(_))
    }

    pub fn is_su(&self) -> bool {
        matches!(self, Self::Su(SuRead::Id { .. }))
    }
}

impl AdbShellRead {
    fn serial(&self) -> &DeviceSerial {
        match self {
            Self::GetpropAll { serial }
            | Self::Getprop { serial, .. }
            | Self::LsBlockByName { serial, .. }
            | Self::LsWorkDir { serial }
            | Self::DumpsysBattery { serial }
            | Self::DumpsysDiskstats { serial }
            | Self::DumpsysPackage { serial, .. } => serial,
        }
    }
}

impl SuRead {
    fn serial(&self) -> &DeviceSerial {
        match self {
            Self::Id { serial }
            | Self::MagiskVersion { serial }
            | Self::MagiskVersionCode { serial }
            | Self::LsMagiskDir { serial }
            | Self::Sha256Block { serial, .. }
            | Self::Sha256BlockPrefix { serial, .. } => serial,
        }
    }
}

impl WriteCmd {
    pub fn serial(&self) -> &DeviceSerial {
        match self {
            Self::AdbHost(
                AdbHostWrite::Push { serial, .. }
                | AdbHostWrite::Reboot { serial, .. }
                | AdbHostWrite::Sideload { serial, .. },
            ) => serial,
            Self::AdbShell(
                AdbShellWrite::MakeWorkDir { serial } | AdbShellWrite::RunPatchScript { serial },
            ) => serial,
            Self::Su(SuWrite::RunPatchScript { serial }) => serial,
            Self::Fastboot(
                FastbootWrite::Flash { serial, .. }
                | FastbootWrite::SetActive { serial, .. }
                | FastbootWrite::Update { serial, .. }
                | FastbootWrite::Reboot { serial, .. },
            ) => serial,
        }
    }

    pub fn uses_fastboot(&self) -> bool {
        matches!(self, Self::Fastboot(_))
    }

    pub fn uses_adb(&self) -> bool {
        !self.uses_fastboot()
    }
}

impl CleanupCmd {
    pub fn serial(&self) -> &DeviceSerial {
        match self {
            Self::RemoveWorkDir { serial } => serial,
        }
    }
}

/// Verbs that must never appear in a rendered read command.
pub const WRITE_VERBS: &[&str] = &[
    "reboot",
    "dd",
    "setprop",
    "rm",
    "mkdir",
    "push",
    "flash",
    "erase",
    "set_active",
    "sideload",
    ">",
];

pub fn read_contains_write_verb(argv: &[String]) -> Option<&'static str> {
    let joined = argv.join(" ");
    WRITE_VERBS
        .iter()
        .copied()
        .find(|verb| contains_verb(&joined, verb))
}

fn contains_verb(haystack: &str, verb: &str) -> bool {
    if verb == ">" {
        return haystack.contains('>');
    }
    haystack
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .any(|word| word == verb || word == "set-active" && verb == "set_active")
}
