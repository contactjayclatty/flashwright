// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.4 — unknown slot is an error, and unlock is a string compare.

mod common;

use std::sync::Arc;

use flashwright_device::{
    lock_from_adb, lock_from_fastboot, parse_getprop, parse_getvar, parse_slot, DeviceError,
    LockState, Slot, TransportConfig,
};
use flashwright_proc::ScriptedRunner;

use common::{catalogues, script_adb_probe, script_lists, transport};

#[test]
fn empty_or_unknown_slot_is_not_slot_a() {
    assert_eq!(parse_slot(""), None);
    assert_eq!(parse_slot("   "), None);
    assert_eq!(parse_slot("all"), None);
    assert_eq!(parse_slot("_a"), Some(Slot::A));
    assert_eq!(parse_slot("b"), Some(Slot::B));
    assert_ne!(parse_slot(""), Some(Slot::A));
}

#[test]
fn lock_compares_strings() {
    let unlocked =
        parse_getprop("[ro.boot.flash.locked]: [0]\n[ro.boot.verifiedbootstate]: [green]\n");
    assert_eq!(lock_from_adb(&unlocked), LockState::Unlocked);
    let locked =
        parse_getprop("[ro.boot.flash.locked]: [1]\n[ro.boot.verifiedbootstate]: [green]\n");
    assert_eq!(lock_from_adb(&locked), LockState::Locked);
    let orange =
        parse_getprop("[ro.boot.flash.locked]: [1]\n[ro.boot.verifiedbootstate]: [orange]\n");
    assert_eq!(lock_from_adb(&orange), LockState::Unlocked);
    let red = parse_getprop("[ro.boot.verifiedbootstate]: [red]\n");
    assert_eq!(lock_from_adb(&red), LockState::Unknown);
    let fastboot = parse_getvar("(bootloader) unlocked: yes\n", "");
    assert_eq!(lock_from_fastboot(&fastboot), LockState::Unlocked);
    let no = parse_getvar("(bootloader) unlocked: no\n", "");
    assert_eq!(lock_from_fastboot(&no), LockState::Locked);
}

#[tokio::test]
async fn unknown_slot_errors_and_alias_maps_to_aurora() {
    let runner = Arc::new(ScriptedRunner::new());
    let props = "\
[ro.product.device]: [eos]
[ro.product.model]: [Pixel Watch]
[ro.boot.slot_suffix]: []
[ro.boot.flash.locked]: [0]
";
    script_lists(&runner, "watch1 device\n", "");
    script_adb_probe(
        &runner,
        "watch1",
        props,
        "uid=2000\n",
        1,
        "",
        "No such file\n",
    );
    let transport = transport(runner, TransportConfig::for_tests());
    let (aliases, devices) = catalogues();
    let info = transport
        .device_info("watch1", &aliases, &devices)
        .await
        .unwrap();
    assert_eq!(info.codename.as_deref(), Some("aurora"));
    assert_eq!(info.codename_raw.as_deref(), Some("eos"));
    assert_eq!(info.active_slot, None);
    assert_eq!(info.lock, LockState::Unlocked);
    let err = info.inactive_slot().unwrap_err();
    assert!(matches!(err, DeviceError::UnknownSlot));
}
