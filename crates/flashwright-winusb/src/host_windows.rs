// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Present-device enumeration through SetupAPI.
//!
//! The mapping from problem code and service name onto driver status is
//! unverified until a capture from a test phone is recorded.

use std::mem::size_of;

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_Status, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
    SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceRegistryPropertyW,
    CM_DEVNODE_STATUS_FLAGS, CM_PROB, CR_SUCCESS, DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO,
    SPDRP_SERVICE, SP_DEVINFO_DATA,
};

use crate::classify::{ids_from_instance, RawUsbDevice};
use crate::UsbError;

struct DevInfoGuard(HDEVINFO);

impl Drop for DevInfoGuard {
    fn drop(&mut self) {
        // SAFETY: this guard owns the list returned by SetupDiGetClassDevsW.
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

pub(crate) fn enumerate_present() -> Result<Vec<RawUsbDevice>, UsbError> {
    // SAFETY: SetupAPI calls use a list we destroy in DevInfoGuard, and
    // buffers sized for the instance id and service name.
    unsafe { enumerate_present_inner() }
}

unsafe fn enumerate_present_inner() -> Result<Vec<RawUsbDevice>, UsbError> {
    let flags = DIGCF_PRESENT | DIGCF_ALLCLASSES;
    let set = SetupDiGetClassDevsW(None, PCWSTR::null(), None, flags)
        .map_err(|err| UsbError::SetupApi(err.to_string()))?;
    let _guard = DevInfoGuard(set);
    let mut out = Vec::new();
    let mut index = 0u32;
    loop {
        let mut info = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ClassGuid: GUID::zeroed(),
            DevInst: 0,
            Reserved: 0,
        };
        if SetupDiEnumDeviceInfo(set, index, &mut info).is_err() {
            break;
        }
        index += 1;
        let Some(instance_id) = instance_id(set, &info) else {
            continue;
        };
        let Some((vid, pid)) = ids_from_instance(&instance_id) else {
            continue;
        };
        let service = service_name(set, &info);
        let problem_code = problem_code(info.DevInst);
        out.push(RawUsbDevice {
            instance_id,
            vid,
            pid,
            service,
            problem_code,
        });
    }
    Ok(out)
}

unsafe fn instance_id(set: HDEVINFO, info: &SP_DEVINFO_DATA) -> Option<String> {
    let mut buf = vec![0u16; 512];
    let mut required = 0u32;
    SetupDiGetDeviceInstanceIdW(set, info, Some(&mut buf), Some(&mut required)).ok()?;
    Some(wide_to_string(&buf))
}

unsafe fn service_name(set: HDEVINFO, info: &SP_DEVINFO_DATA) -> Option<String> {
    let mut buf = vec![0u8; 512];
    let mut reg_type = 0u32;
    let mut required = 0u32;
    SetupDiGetDeviceRegistryPropertyW(
        set,
        info,
        SPDRP_SERVICE,
        Some(&mut reg_type),
        Some(&mut buf),
        Some(&mut required),
    )
    .ok()?;
    let wide = bytes_as_wide(&buf);
    let text = wide_to_string(&wide);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

unsafe fn problem_code(devinst: u32) -> u32 {
    let mut status = CM_DEVNODE_STATUS_FLAGS(0);
    let mut problem = CM_PROB(0);
    let result = CM_Get_DevNode_Status(&mut status, &mut problem, devinst, 0);
    if result == CR_SUCCESS {
        problem.0
    } else {
        0
    }
}

fn bytes_as_wide(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .filter(|chunk| chunk.len() == 2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|unit| *unit != 0)
        .collect()
}

fn wide_to_string(units: &[u16]) -> String {
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}
