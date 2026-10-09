// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Map a scanned phone onto the facts the firmware opener compares.
//! The image patch and build date are not fields here.

use crate::device::{BootTarget, DeviceInfo, InitBootPresence, LockState, Mode, RootState};
use crate::firmware::DeviceFacts;
use crate::CoreError;

pub fn facts_from_device(info: &DeviceInfo) -> Result<DeviceFacts, CoreError> {
    let codename = info
        .codename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreError::Rejected {
            reason: "the phone codename was not read".into(),
        })?;
    let build_date_utc = match info
        .build_date_utc
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => Some(value.parse::<u64>().map_err(|_| CoreError::Rejected {
            reason: "the phone build date could not be read".into(),
        })?),
        None => None,
    };
    let security_patch = info
        .spl
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok(DeviceFacts {
        codename: codename.to_string(),
        build_date_utc,
        security_patch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phone(codename: Option<&str>, date: Option<&str>, spl: Option<&str>) -> DeviceInfo {
        DeviceInfo {
            serial: "serial".to_string(),
            mode: Mode::Adb,
            model: None,
            codename: codename.map(str::to_string),
            codename_raw: codename.map(str::to_string),
            build_id: None,
            fingerprint: None,
            build_date_utc: date.map(str::to_string),
            sdk: None,
            spl: spl.map(str::to_string),
            active_slot: None,
            lock: LockState::Unknown,
            bootloader_version: None,
            root: RootState::RootUnknown {
                reason: "not checked".to_string(),
            },
            magisk_version: None,
            magisk_code: None,
            magisk_app_version: None,
            magisk_app_code: None,
            init_boot: InitBootPresence::Unknown,
            boot_target: BootTarget::Unknown,
            battery: None,
            transport_id: None,
        }
    }

    #[test]
    fn device_facts_follow_the_scan() {
        let facts = facts_from_device(&phone(
            Some("komodo"),
            Some("1750000000"),
            Some("2026-10-01"),
        ))
        .unwrap();
        assert_eq!(facts.codename, "komodo");
        assert_eq!(facts.build_date_utc, Some(1_750_000_000));
        assert_eq!(facts.security_patch.as_deref(), Some("2026-10-01"));
    }

    #[test]
    fn a_missing_codename_is_refused() {
        let err = facts_from_device(&phone(None, None, None)).unwrap_err();
        assert!(err.to_string().contains("codename"));
    }
}
