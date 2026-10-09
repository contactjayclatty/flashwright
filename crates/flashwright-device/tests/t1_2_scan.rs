// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.2 — each connection state is reported from a scripted device list.

mod common;

use std::sync::Arc;
use std::time::Instant;

use flashwright_device::{Mode, TransportConfig};
use flashwright_proc::ScriptedRunner;

use common::{script_lists, transport};

#[tokio::test]
async fn t1_2_scan_states() {
    let runner = Arc::new(ScriptedRunner::new());
    script_lists(
        &runner,
        "\
serial-device device product:shiba model:Pixel_8 device:shiba transport_id:7
serial-adb device product:shiba transport_id:8
serial-authz authorizing
serial-adb-np no permissions
serial-unauth unauthorized
serial-off offline
serial-rec recovery
serial-side sideload transport_id:3
serial-rescue rescue
serial-odd not-a-mode
",
        "\
serial-fb fastboot
serial-fbd fastbootd
serial-np no permissions
serial-device fastboot
",
    );
    let transport = transport(Arc::clone(&runner), TransportConfig::production());
    let started = Instant::now();
    let rows = transport.list().await.unwrap();
    assert!(started.elapsed().as_secs() < 3);

    let mode = |serial: &str| {
        rows.iter()
            .find(|row| row.serial == serial)
            .unwrap_or_else(|| panic!("missing {serial}"))
            .mode
    };
    assert_eq!(mode("serial-adb"), Mode::Adb);
    assert_eq!(mode("serial-authz"), Mode::Authorizing);
    assert_eq!(mode("serial-adb-np"), Mode::NoPermissions);
    assert_eq!(mode("serial-unauth"), Mode::Unauthorized);
    assert_eq!(mode("serial-off"), Mode::Offline);
    assert_eq!(mode("serial-rec"), Mode::Recovery);
    assert_eq!(mode("serial-side"), Mode::Sideload);
    assert_eq!(mode("serial-fb"), Mode::Fastboot);
    assert_eq!(mode("serial-fbd"), Mode::Fastbootd);
    assert_eq!(mode("serial-rescue"), Mode::Rescue);
    assert_eq!(mode("serial-np"), Mode::NoPermissions);
    assert_eq!(mode("serial-odd"), Mode::Unrecognized);
    let replaced = rows
        .iter()
        .find(|row| row.serial == "serial-device")
        .unwrap();
    assert_eq!(replaced.mode, Mode::Fastboot);
    let side = rows.iter().find(|row| row.serial == "serial-side").unwrap();
    assert_eq!(side.transport_id.as_deref(), Some("3"));
}
