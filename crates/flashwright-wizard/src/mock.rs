// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::sync::atomic::{AtomicU32, Ordering};

use crate::device::{
    DeviceInfo, DeviceSummary, DeviceTransport, DeviceWrite, Mode, Partition, Slot,
};
use crate::error::CoreError;

pub const FIXTURE_SHA256: &str = "ab12cd34e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
pub const FIXTURE_OTA_NAME: &str = "harbor-ab12cd34-full.zip";
pub const FIXTURE_FACTORY_NAME: &str = "harbor-ab12cd34-factory.zip";
pub const PRIMARY_SERIAL: &str = "FWMOCK000001";
pub const LOCKED_OUT_SERIAL: &str = "FWMOCK000002";

/// In-memory sample phones. No USB, adb, or fastboot.
#[derive(Debug)]
pub struct MockTransport {
    write_calls: AtomicU32,
}

impl Default for MockTransport {
    fn default() -> Self {
        Self {
            write_calls: AtomicU32::new(0),
        }
    }
}

impl MockTransport {
    pub fn write_calls(&self) -> u32 {
        self.write_calls.load(Ordering::SeqCst)
    }

    fn primary() -> DeviceInfo {
        DeviceInfo {
            serial: PRIMARY_SERIAL.to_string(),
            mode: Mode::Adb,
            model: "Harbor".to_string(),
            codename: "harbor".to_string(),
            build_id: "HQ1A.MOCK.001".to_string(),
            fingerprint: "harbor/harbor/harbor:16/HQ1A.MOCK.001/100:user/release-keys".to_string(),
            spl: "2026-09-05".to_string(),
            active_slot: Some(Slot::A),
            bootloader_unlocked: true,
            bootloader_version: "harbor-1.0-100".to_string(),
            root_present: true,
            root_tool_version: "30.7".to_string(),
            root_tool_code: 30700,
            uses_init_boot: true,
            battery_percent: 82,
            battery_charging: false,
        }
    }

    fn summary(info: &DeviceInfo) -> DeviceSummary {
        DeviceSummary {
            serial: info.serial.clone(),
            mode: info.mode,
            model: info.model.clone(),
            codename: info.codename.clone(),
        }
    }
}

impl DeviceTransport for MockTransport {
    fn list(&self) -> Result<Vec<DeviceSummary>, CoreError> {
        let primary = Self::primary();
        Ok(vec![
            Self::summary(&primary),
            DeviceSummary {
                serial: LOCKED_OUT_SERIAL.to_string(),
                mode: Mode::Unauthorized,
                model: "Harbor".to_string(),
                codename: "harbor".to_string(),
            },
        ])
    }

    fn info(&self, serial: &str) -> Result<DeviceInfo, CoreError> {
        if serial == PRIMARY_SERIAL {
            return Ok(Self::primary());
        }
        if serial == LOCKED_OUT_SERIAL {
            return Ok(DeviceInfo {
                serial: LOCKED_OUT_SERIAL.to_string(),
                mode: Mode::Unauthorized,
                model: "Harbor".to_string(),
                codename: "harbor".to_string(),
                build_id: String::new(),
                fingerprint: String::new(),
                spl: String::new(),
                active_slot: None,
                bootloader_unlocked: false,
                bootloader_version: String::new(),
                root_present: false,
                root_tool_version: String::new(),
                root_tool_code: 0,
                uses_init_boot: false,
                battery_percent: 0,
                battery_charging: false,
            });
        }
        Err(CoreError::message("That phone is not in the scan."))
    }
}

impl DeviceWrite for MockTransport {
    fn flash(
        &self,
        _serial: &str,
        _slot: Slot,
        _partition: Partition,
        _image: &str,
    ) -> Result<(), CoreError> {
        self.write_calls.fetch_add(1, Ordering::SeqCst);
        Err(CoreError::DeviceLayerStub)
    }
}
