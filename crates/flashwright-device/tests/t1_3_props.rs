// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.3 — device info matches the scripted getprop and getvar fixtures.

mod common;

use std::sync::Arc;
use std::time::Duration;

use flashwright_device::{
    BootTarget, InitBootPresence, LockState, Mode, RootState, TransportConfig,
};
use flashwright_proc::{ScriptedResponse, ScriptedRunner};

use common::{
    adb_name, catalogues, fastboot_name, script_adb_probe, script_lists, transport, ORIOLE_PROPS,
    SHIBA_PROPS,
};

#[tokio::test]
async fn t1_3_adb_props_match_fixtures() {
    let runner = Arc::new(ScriptedRunner::new());
    script_lists(
        &runner,
        "dib123 device transport_id:9\ndb456 device transport_id:2\n",
        "",
    );
    script_adb_probe(
        &runner,
        "dib123",
        SHIBA_PROPS,
        "uid=0(root) gid=0(root)\n",
        0,
        "/dev/block/by-name/init_boot_a\n",
        "",
    );
    script_adb_probe(
        &runner,
        "db456",
        ORIOLE_PROPS,
        "uid=0(root) gid=0(root)\n",
        1,
        "",
        "ls: /dev/block/by-name/init_boot_a: No such file or directory\n",
    );
    let transport = transport(Arc::clone(&runner), TransportConfig::production());
    let (aliases, devices) = catalogues();

    let shiba = transport
        .device_info("dib123", &aliases, &devices)
        .await
        .unwrap();
    assert_eq!(shiba.serial, "dib123");
    assert_eq!(shiba.mode, Mode::Adb);
    assert_eq!(shiba.model.as_deref(), Some("Pixel 8"));
    assert_eq!(shiba.codename.as_deref(), Some("shiba"));
    assert_eq!(shiba.codename_raw.as_deref(), Some("shiba"));
    assert_eq!(shiba.build_id.as_deref(), Some("UD1A.231105.004"));
    assert_eq!(
        shiba.fingerprint.as_deref(),
        Some("google/shiba/shiba:14/UD1A.231105.004/11010374:user/release-keys")
    );
    assert_eq!(shiba.build_date_utc.as_deref(), Some("1699000000"));
    assert_eq!(shiba.sdk.as_deref(), Some("34"));
    assert_eq!(shiba.spl.as_deref(), Some("2023-11-05"));
    assert_eq!(shiba.active_slot, Some(flashwright_device::Slot::B));
    assert_eq!(shiba.inactive_slot().unwrap(), flashwright_device::Slot::A);
    assert_eq!(shiba.lock, LockState::Unlocked);
    assert_eq!(
        shiba.bootloader_version.as_deref(),
        Some("ripcurrent-14.0-11010374")
    );
    assert_eq!(shiba.root, RootState::Rooted);
    assert_eq!(shiba.magisk_version.as_deref(), Some("27.0"));
    assert_eq!(shiba.magisk_code, Some(27000));
    assert_eq!(shiba.magisk_app_version.as_deref(), Some("27.0"));
    assert_eq!(shiba.magisk_app_code, Some(27000));
    assert_eq!(shiba.init_boot, InitBootPresence::Present);
    assert_eq!(shiba.boot_target, BootTarget::InitBoot);
    let battery = shiba.battery.unwrap();
    assert_eq!(battery.level, Some(80));
    assert_eq!(battery.charging, Some(true));
    assert_eq!(shiba.transport_id.as_deref(), Some("9"));

    let oriole = transport
        .device_info("db456", &aliases, &devices)
        .await
        .unwrap();
    assert_eq!(oriole.model.as_deref(), Some("Pixel 6"));
    assert_eq!(oriole.codename.as_deref(), Some("oriole"));
    assert_eq!(oriole.active_slot, Some(flashwright_device::Slot::A));
    assert_eq!(oriole.inactive_slot().unwrap(), flashwright_device::Slot::B);
    assert_eq!(oriole.lock, LockState::Unlocked);
    assert_eq!(oriole.build_id.as_deref(), Some("AP1A.240505.005"));
    assert_eq!(oriole.spl.as_deref(), Some("2024-05-05"));
    assert_eq!(oriole.sdk.as_deref(), Some("34"));
    assert_eq!(oriole.init_boot, InitBootPresence::Absent);
    assert_eq!(oriole.boot_target, BootTarget::Boot);
    assert_eq!(oriole.root, RootState::Rooted);

    let getprop = runner
        .calls()
        .into_iter()
        .find(|call| call.args == ["-s", "dib123", "shell", "'getprop'"])
        .expect("getprop");
    assert!(getprop.args.iter().all(|arg| arg != "su"));
    assert_eq!(getprop.timeout, Duration::from_secs(10));
    let su = runner
        .calls()
        .into_iter()
        .find(|call| {
            call.args.len() >= 4 && call.args[2] == "shell" && call.args[3].contains("'su'")
        })
        .expect("su");
    assert_eq!(su.timeout, Duration::from_secs(5));
    assert_eq!(su.args, ["-s", "dib123", "shell", "'su' '-c' 'id'"]);
}

#[tokio::test]
async fn t1_3_fastboot_getvar_matches_fixture() {
    let runner = Arc::new(ScriptedRunner::new());
    script_lists(&runner, "", "dib123 fastboot\n");
    runner.on(
        fastboot_name(),
        &["-s", "dib123", "getvar", "all"],
        ScriptedResponse::ok(
            "\
(bootloader) product: shiba
(bootloader) current-slot: b
(bootloader) unlocked: yes
(bootloader) version-bootloader: ripcurrent-14.0-11010374
(bootloader) partition-size:init_boot_a: 0x800000
Finished. Total time: 0.001s
",
        ),
    );
    let transport = transport(Arc::clone(&runner), TransportConfig::production());
    let (aliases, devices) = catalogues();
    let info = transport
        .device_info("dib123", &aliases, &devices)
        .await
        .unwrap();
    assert_eq!(info.mode, Mode::Fastboot);
    assert_eq!(info.model, None);
    assert_eq!(info.codename.as_deref(), Some("shiba"));
    assert_eq!(info.active_slot, Some(flashwright_device::Slot::B));
    assert_eq!(info.lock, LockState::Unlocked);
    assert_eq!(
        info.bootloader_version.as_deref(),
        Some("ripcurrent-14.0-11010374")
    );
    assert_eq!(info.init_boot, InitBootPresence::Present);
    assert_eq!(info.boot_target, BootTarget::InitBoot);
    assert_eq!(
        info.root,
        RootState::RootUnknown {
            reason: "not in adb mode".into()
        }
    );
    assert_eq!(info.magisk_version, None);
    assert_eq!(info.battery, None);
    assert!(runner
        .calls()
        .iter()
        .all(|call| call.program.file_name().unwrap() != adb_name() || call.args[0] == "devices"));
}
