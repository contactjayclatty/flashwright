// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Safety tables compiled into the binary.
//!
//! Each table names the PixelFlasher file it was ported from. The gate path
//! reads them through [`tables`].

use std::sync::OnceLock;

use serde::Deserialize;

use crate::device::AliasTable;

const DEVICES: &str = include_str!("../../../../data/device_compatibility.toml");
const KNOWN_BAD: &str = include_str!("../../../../data/known_bad_magisk.toml");
const MIN_BOOTLOADER: &str = include_str!("../../../../data/min_bootloader.toml");
const KERNELS: &str = include_str!("../../../../data/banned_kernels.toml");
const OFF_LIMITS: &str = include_str!("../../../../data/off_limits.toml");
const SLOT_RULES: &str = include_str!("../../../../data/slot_rules.toml");
const TENSOR: &str = include_str!("../../../../data/tensor_arb.toml");
const ALIASES: &str = include_str!("../../../../data/device_aliases.toml");

pub const UPSTREAM_COMMIT: &str = "081286d";

/// One ported item. The disclaimer lists the same rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortedItem {
    pub id: &'static str,
    pub upstream_path: &'static str,
    pub lines: &'static str,
    pub commit: &'static str,
}

pub fn ported_items() -> &'static [PortedItem] {
    const ITEMS: &[PortedItem] = &[
        PortedItem {
            id: "device-compatibility",
            upstream_path: "android_devices.json",
            lines: "entire file",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "init-boot-lookup",
            upstream_path: "runtime.py",
            lines: "1140-1147",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "build-spl",
            upstream_path: "runtime.py",
            lines: "11791-11856",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "model-match",
            upstream_path: "pf_modules.py",
            lines: "4224-4258 and 5274-5303",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "known-bad-magisk",
            upstream_path: "constants.py",
            lines: "49",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "banned-kernels",
            upstream_path: "constants.py",
            lines: "74-101",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "unofficial-magisk",
            upstream_path: "constants.py",
            lines: "59-60",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "min-bootloader",
            upstream_path: "constants.py",
            lines: "135-146",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "bootloader-compare",
            upstream_path: "runtime.py",
            lines: "11536-11563 and 11698-11724",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "tensor-arb",
            upstream_path: "pf_modules.py",
            lines: "6322-6367",
            commit: UPSTREAM_COMMIT,
        },
        PortedItem {
            id: "slot-rules",
            upstream_path: "pf_modules.py",
            lines: "6283-6293 and 6422",
            commit: UPSTREAM_COMMIT,
        },
    ];
    ITEMS
}

#[derive(Clone, Debug)]
pub struct DeviceCompat {
    pub codename: String,
    pub model: String,
    pub first_api_level: u32,
    pub bootloader_codename: String,
    pub has_init_boot: bool,
    pub is_pixel_watch: bool,
    pub ab: bool,
}

#[derive(Clone, Debug)]
pub struct MagiskCombo {
    pub label: String,
    pub version_code: u32,
}

#[derive(Clone, Debug)]
pub struct BootloaderMin {
    pub codename: String,
    pub min: String,
}

#[derive(Clone, Debug)]
pub struct TensorArb {
    pub codename: String,
    pub min_api: u32,
}

#[derive(Clone, Debug)]
pub struct SlotRules {
    pub both_slots: String,
    pub slot_all: String,
    pub target: String,
    pub explicit_slot: bool,
}

#[derive(Clone, Debug)]
pub struct OffLimits {
    pub regions: Vec<String>,
    pub argv: Vec<String>,
    pub shell: Vec<String>,
    pub packages: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SafetyTables {
    pub devices: Vec<DeviceCompat>,
    pub aliases: AliasTable,
    pub combos: Vec<MagiskCombo>,
    pub kernels: Vec<String>,
    pub bootloaders: Vec<BootloaderMin>,
    pub tensor: Vec<TensorArb>,
    pub slots: SlotRules,
    pub off_limits: OffLimits,
}

impl SafetyTables {
    fn load() -> Result<Self, String> {
        let devices: DeviceFile = toml::from_str(DEVICES).map_err(|err| err.to_string())?;
        let known: KnownFile = toml::from_str(KNOWN_BAD).map_err(|err| err.to_string())?;
        let mins: MinFile = toml::from_str(MIN_BOOTLOADER).map_err(|err| err.to_string())?;
        let kernels: KernelFile = toml::from_str(KERNELS).map_err(|err| err.to_string())?;
        let off: OffFile = toml::from_str(OFF_LIMITS).map_err(|err| err.to_string())?;
        let slots: SlotFile = toml::from_str(SLOT_RULES).map_err(|err| err.to_string())?;
        let tensor: TensorFile = toml::from_str(TENSOR).map_err(|err| err.to_string())?;
        let aliases = AliasTable::from_toml(ALIASES).map_err(|err| err.to_string())?;
        Ok(Self {
            devices: devices
                .device
                .into_iter()
                .map(|row| DeviceCompat {
                    codename: row.codename,
                    model: row.model,
                    first_api_level: row.first_api_level,
                    bootloader_codename: row.bootloader_codename,
                    has_init_boot: row.has_init_boot,
                    is_pixel_watch: row.is_pixel_watch,
                    ab: row.ab,
                })
                .collect(),
            aliases,
            combos: known
                .combo
                .into_iter()
                .map(|row| MagiskCombo {
                    label: row.label,
                    version_code: row.version_code,
                })
                .collect(),
            kernels: kernels.fragments,
            bootloaders: mins
                .entries
                .into_iter()
                .map(|row| BootloaderMin {
                    codename: row.codename,
                    min: row.min,
                })
                .collect(),
            tensor: tensor
                .device
                .into_iter()
                .map(|row| TensorArb {
                    codename: row.codename,
                    min_api: row.min_api,
                })
                .collect(),
            slots: SlotRules {
                both_slots: slots.both_slots,
                slot_all: slots.slot_all,
                target: slots.target,
                explicit_slot: slots.explicit_slot,
            },
            off_limits: OffLimits {
                regions: off.regions,
                argv: off.argv,
                shell: off.shell,
                packages: off.packages,
            },
        })
    }
}

pub fn tables() -> &'static SafetyTables {
    static TABLES: OnceLock<SafetyTables> = OnceLock::new();
    TABLES.get_or_init(|| SafetyTables::load().expect("safety tables parse"))
}

pub fn log_ported_items() {
    static LOGGED: OnceLock<()> = OnceLock::new();
    LOGGED.get_or_init(|| {
        for item in ported_items() {
            tracing::info!(
                item = item.id,
                upstream = item.upstream_path,
                lines = item.lines,
                commit = item.commit,
                "ported safety table"
            );
        }
    });
}

#[derive(Debug, Deserialize)]
struct DeviceFile {
    device: Vec<DeviceRow>,
}

#[derive(Debug, Deserialize)]
struct DeviceRow {
    codename: String,
    model: String,
    first_api_level: u32,
    bootloader_codename: String,
    has_init_boot: bool,
    is_pixel_watch: bool,
    ab: bool,
}

#[derive(Debug, Deserialize)]
struct KnownFile {
    #[serde(default)]
    combo: Vec<ComboRow>,
}

#[derive(Debug, Deserialize)]
struct ComboRow {
    label: String,
    version_code: u32,
}

#[derive(Debug, Deserialize)]
struct MinFile {
    entries: Vec<MinRow>,
}

#[derive(Debug, Deserialize)]
struct MinRow {
    codename: String,
    min: String,
}

#[derive(Debug, Deserialize)]
struct KernelFile {
    fragments: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OffFile {
    regions: Vec<String>,
    argv: Vec<String>,
    shell: Vec<String>,
    packages: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SlotFile {
    both_slots: String,
    slot_all: String,
    target: String,
    explicit_slot: bool,
}

#[derive(Debug, Deserialize)]
struct TensorFile {
    device: Vec<TensorRow>,
}

#[derive(Debug, Deserialize)]
struct TensorRow {
    codename: String,
    min_api: u32,
}
