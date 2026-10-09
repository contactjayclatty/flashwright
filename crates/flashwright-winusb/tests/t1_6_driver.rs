// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.6 — driver status from fixtures, not a live SetupAPI capture.

use flashwright_winusb::{
    classify, probe_host, DriverStatus, HostProbe, RawUsbDevice, UsbInterface, UsbTable,
    DRIVER_GUIDANCE,
};

fn raw(vid: &str, pid: &str, service: Option<&str>, problem: u32) -> RawUsbDevice {
    RawUsbDevice {
        instance_id: format!("USB\\VID_{vid}&PID_{pid}\\ABC"),
        vid: vid.into(),
        pid: pid.into(),
        service: service.map(str::to_string),
        problem_code: problem,
    }
}

#[test]
fn t1_6_driver_fixtures() {
    let table = UsbTable::embedded().unwrap();

    let ok = classify(&raw("18D1", "4EE7", Some("WinUSB"), 0), &table).unwrap();
    assert_eq!(ok.status, DriverStatus::Ok);
    assert_eq!(ok.interface, UsbInterface::Adb);
    assert_eq!(ok.guidance, None);

    let missing = classify(&raw("18d1", "4ee0", None, 28), &table).unwrap();
    assert_eq!(missing.status, DriverStatus::NoDriver);
    assert_eq!(missing.interface, UsbInterface::Bootloader);
    let guidance = missing.guidance.unwrap();
    assert!(guidance.contains("https://developer.android.com/studio/run/win-usb"));
    assert!(guidance.contains("USB 2.0"));
    assert!(guidance.contains("known-good cable"));
    assert_eq!(guidance, DRIVER_GUIDANCE);

    let problem = classify(&raw("18D1", "4EE7", Some("usbccgp"), 1), &table).unwrap();
    assert_eq!(problem.status, DriverStatus::Problem { code: 1 });
    assert!(problem.guidance.is_some());

    assert!(classify(&raw("1234", "0001", Some("WinUSB"), 0), &table).is_none());

    let unknown = classify(&raw("18D1", "4EE2", Some("SomeOther"), 0), &table).unwrap();
    assert_eq!(unknown.status, DriverStatus::Unknown);
    assert_eq!(unknown.interface, UsbInterface::Adb);
    assert!(unknown.guidance.is_some());
}

#[test]
fn non_windows_probe_does_not_touch_hardware() {
    let table = UsbTable::embedded().unwrap();
    if cfg!(windows) {
        let probe = probe_host(&table).unwrap();
        assert!(matches!(probe, HostProbe::Reports(_)));
    } else {
        assert_eq!(probe_host(&table).unwrap(), HostProbe::UnsupportedHost);
    }
}
