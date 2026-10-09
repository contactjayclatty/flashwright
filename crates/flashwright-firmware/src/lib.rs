// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Open a Pixel factory zip or a full OTA zip and extract init_boot or boot.
//! Callers choose the output directory. This crate does not write to a device.

mod bootimg;
mod catalog;
mod check;
mod error;
mod factory;
mod hashutil;
mod metadata;
mod open;
mod payload;
mod proto;
mod region;
mod select;
mod space;
mod ziputil;

#[cfg(test)]
mod tests;

pub use bootimg::{read_boot_image, synthetic_boot, BootInfo};
pub use error::FirmwareError;
pub use open::{extraction_workers, open_package, OpenRequest, OpenedPackage, PackageKind};
pub use select::StockPartition;
pub use ziputil::windows_flash_name;
