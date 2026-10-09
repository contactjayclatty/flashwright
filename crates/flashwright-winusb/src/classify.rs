// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use crate::UsbTable;

/// Google's USB driver page, plus the cable guidance Phase 1 shows.
pub const DRIVER_GUIDANCE: &str = "Install Google's USB driver from https://developer.android.com/studio/run/win-usb. Use a USB 2.0 port and a known-good cable.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverStatus {
    Ok,
    NoDriver,
    Problem { code: u32 },
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbInterface {
    Adb,
    Bootloader,
    Unknown,
}

/// One present device node, already parsed out of a SetupAPI instance id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawUsbDevice {
    pub instance_id: String,
    pub vid: String,
    pub pid: String,
    pub service: Option<String>,
    pub problem_code: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbReport {
    pub instance_id: String,
    pub vid: String,
    pub pid: String,
    pub status: DriverStatus,
    pub interface: UsbInterface,
    pub guidance: Option<&'static str>,
}

/// `None` when the VID is not a Google id Flashwright knows about.
pub fn classify(raw: &RawUsbDevice, table: &UsbTable) -> Option<UsbReport> {
    if !table.knows_vid(&raw.vid) {
        return None;
    }
    let status = driver_status(raw.service.as_deref(), raw.problem_code);
    let guidance = if status == DriverStatus::Ok {
        None
    } else {
        Some(DRIVER_GUIDANCE)
    };
    Some(UsbReport {
        instance_id: raw.instance_id.clone(),
        vid: raw.vid.to_ascii_uppercase(),
        pid: raw.pid.to_ascii_uppercase(),
        status,
        interface: table.interface_for(&raw.vid, &raw.pid),
        guidance,
    })
}

fn driver_status(service: Option<&str>, problem_code: u32) -> DriverStatus {
    let service = service.map(str::trim).filter(|value| !value.is_empty());
    if problem_code != 0 {
        return match service {
            None => DriverStatus::NoDriver,
            Some(_) => DriverStatus::Problem { code: problem_code },
        };
    }
    match service {
        None => DriverStatus::NoDriver,
        Some(name) if is_android_driver(name) => DriverStatus::Ok,
        Some(_) => DriverStatus::Unknown,
    }
}

fn is_android_driver(service: &str) -> bool {
    matches!(
        service.to_ascii_lowercase().as_str(),
        "winusb" | "androidusb" | "android_winusb"
    )
}

/// Read `VID_XXXX` and `PID_XXXX` out of a device instance id.
pub fn ids_from_instance(instance_id: &str) -> Option<(String, String)> {
    let upper = instance_id.to_ascii_uppercase();
    let vid = four_hex_after(&upper, "VID_")?;
    let pid = four_hex_after(&upper, "PID_")?;
    Some((vid, pid))
}

fn four_hex_after(text: &str, marker: &str) -> Option<String> {
    let index = text.find(marker)?;
    let start = index + marker.len();
    let hex = text.get(start..start + 4)?;
    if hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(hex.to_string())
    } else {
        None
    }
}
