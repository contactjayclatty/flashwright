// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.5 public waits and parallel probes.
//!
//! Write transitions are exercised inside `flashwright-core`, because a
//! write token cannot be minted from this crate.

mod common;

use std::sync::Arc;
use std::time::Duration;

use flashwright_device::TransportConfig;
use flashwright_proc::{ScriptedResponse, ScriptedRunner};

use common::{adb_name, catalogues, script_lists, transport as open_transport};

#[test]
fn production_timeouts_match_the_spec() {
    let config = TransportConfig::production();
    assert_eq!(config.poll_interval, Duration::from_secs(1));
    assert_eq!(config.adb_to_bootloader, Duration::from_secs(90));
    assert_eq!(config.adb_to_sideload, Duration::from_secs(120));
    assert_eq!(config.bootloader_to_bootloader, Duration::from_secs(90));
    assert_eq!(config.bootloader_to_system, Duration::from_secs(900));
    assert_eq!(config.sideload_to_bootloader, Duration::from_secs(120));
}

#[tokio::test]
async fn parallel_probes_stay_within_four() {
    let runner = Arc::new(ScriptedRunner::new());
    let mut adb_list = String::new();
    for index in 0..4 {
        let serial = format!("phone{index}");
        adb_list.push_str(&format!("{serial} device\n"));
        runner.on(
            adb_name(),
            &["-s", &serial, "shell", "'getprop'"],
            ScriptedResponse::ok("[ro.product.device]: [shiba]\n[ro.boot.slot_suffix]: [_a]\n[ro.boot.flash.locked]: [0]\n")
                .with_delay(Duration::from_millis(30)),
        );
        runner.on(
            adb_name(),
            &["-s", &serial, "shell"],
            ScriptedResponse::ok("uid=0(root)\n"),
        );
    }
    script_lists(&runner, &adb_list, "");
    let transport = open_transport(Arc::clone(&runner), TransportConfig::for_tests());
    let (aliases, devices) = catalogues();
    let serials: Vec<String> = (0..4).map(|index| format!("phone{index}")).collect();
    let infos = flashwright_device::probe_many(&transport, &serials, &aliases, &devices).await;
    assert_eq!(infos.len(), 4);
    for info in infos {
        assert!(info.is_ok(), "{info:?}");
    }
    let peak = runner.max_in_flight();
    assert!(peak >= 2, "peak {peak}");
    assert!(peak <= 4, "peak {peak}");
}
