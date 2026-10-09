// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Device transport for Flashwright.
//!
//! Scanning, property reads, slot and lock checks, and mode waits live here.
//! Write-class calls require a [`WriteToken`]. Production plans mint that
//! token; this crate's tests mint it so the argv can be checked without a phone.

#[cfg(test)]
mod write_tests;

mod argv;
mod catalog;
mod derive;
mod error;
mod facts;
mod info;
mod parse;
mod transport;
mod types;

pub use crate::token::WriteToken;
pub use catalog::{AliasTable, DeviceRow, DeviceTable, KnownBadMagisk, MinBootloaderTable};
pub use derive::{
    init_boot_from_ls, init_boot_from_size, interpret_su, lock_from_adb, lock_from_fastboot,
    parse_slot,
};
pub use error::DeviceError;
pub use facts::facts_from_device;
pub use parse::{
    merge_scans, parse_adb_devices, parse_battery, parse_dumpsys_package, parse_fastboot_devices,
    parse_getprop, parse_getvar, parse_mode_token, PropMap,
};
pub use transport::{probe_many, DeviceTransport, PlatformToolsTransport};
pub use types::*;
