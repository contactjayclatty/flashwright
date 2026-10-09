// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.5 — mode transitions on two scripted phones, including an unplugged cable.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flashwright_device::{
    contains_slot_all, Mode, Partition, RebootTarget, Slot, TransportConfig, WaitOutcome,
    WriteToken,
};
use flashwright_proc::{ScriptedResponse, ScriptedRunner};

use common::{adb_name, fastboot_name, script_lists, transport as open_transport};

const SERIALS: [&str; 2] = ["dib123", "db456"];

fn script_ready(runner: &ScriptedRunner, serial: &str, adb_line: &str, fastboot_line: &str) {
    script_lists(runner, adb_line, fastboot_line);
    runner.on(
        adb_name(),
        &["-s", serial, "reboot", "bootloader"],
        ScriptedResponse::ok(""),
    );
    runner.on(
        adb_name(),
        &["-s", serial, "reboot", "sideload"],
        ScriptedResponse::ok(""),
    );
    runner.on(
        fastboot_name(),
        &["-s", serial, "reboot", "bootloader"],
        ScriptedResponse::ok(""),
    );
    runner.on(
        fastboot_name(),
        &["-s", serial, "reboot"],
        ScriptedResponse::ok("OKAY\n"),
    );
    runner.on(
        adb_name(),
        &["-s", serial, "shell", "getprop", "sys.boot_completed"],
        ScriptedResponse::ok("1\n"),
    );
    runner.on(
        fastboot_name(),
        &["-s", serial, "--slot", "a", "flash", "init_boot"],
        ScriptedResponse::ok("OKAY\n"),
    );
}

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
async fn t1_5_transitions_reach_the_target() {
    let cases = [
        (Mode::Adb, RebootTarget::Bootloader, "", "SERIAL fastboot\n"),
        (Mode::Adb, RebootTarget::Sideload, "SERIAL sideload\n", ""),
        (
            Mode::Fastboot,
            RebootTarget::Bootloader,
            "",
            "SERIAL fastboot\n",
        ),
        (Mode::Fastbootd, RebootTarget::System, "SERIAL device\n", ""),
        (
            Mode::Sideload,
            RebootTarget::Bootloader,
            "",
            "SERIAL fastbootd\n",
        ),
    ];
    assert_eq!(cases.len(), 5);
    for serial in SERIALS {
        for (from, to, adb_line, fastboot_line) in cases {
            let adb_line = adb_line.replace("SERIAL", serial);
            let fastboot_line = fastboot_line.replace("SERIAL", serial);
            for _run in 0..5 {
                let runner = Arc::new(ScriptedRunner::new());
                script_ready(&runner, serial, &adb_line, &fastboot_line);
                let transport = open_transport(Arc::clone(&runner), TransportConfig::for_tests());
                let started = Instant::now();
                let outcome = transport
                    .reboot(&WriteToken::mint(), serial, from, to)
                    .await
                    .unwrap();
                assert_eq!(outcome, WaitOutcome::Reached);
                assert!(started.elapsed() < Duration::from_secs(2));
            }
        }
    }
}

#[tokio::test]
async fn production_config_returns_as_soon_as_the_mode_is_reached() {
    let runner = Arc::new(ScriptedRunner::new());
    script_ready(&runner, "dib123", "", "dib123 fastboot\n");
    let transport = open_transport(runner, TransportConfig::production());
    let started = Instant::now();
    let outcome = transport
        .reboot(
            &WriteToken::mint(),
            "dib123",
            Mode::Adb,
            RebootTarget::Bootloader,
        )
        .await
        .unwrap();
    assert_eq!(outcome, WaitOutcome::Reached);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn unplugged_cable_is_disappeared_only_at_the_deadline() {
    let runner = Arc::new(ScriptedRunner::new());
    script_lists(&runner, "", "");
    runner.on(
        adb_name(),
        &["-s", "dib123", "reboot", "bootloader"],
        ScriptedResponse::ok(""),
    );
    let transport = open_transport(Arc::clone(&runner), TransportConfig::for_tests());
    let timeout = transport.config().adb_to_bootloader;
    let started = Instant::now();
    let outcome = transport
        .reboot(
            &WriteToken::mint(),
            "dib123",
            Mode::Adb,
            RebootTarget::Bootloader,
        )
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(outcome, WaitOutcome::Disappeared);
    assert!(
        elapsed >= timeout,
        "elapsed {elapsed:?} timeout {timeout:?}"
    );
    assert!(elapsed < timeout + Duration::from_secs(2));
}

#[tokio::test]
async fn wrong_mode_and_boot_timeout() {
    let runner = Arc::new(ScriptedRunner::new());
    script_lists(&runner, "dib123 recovery\n", "");
    runner.on(
        fastboot_name(),
        &["-s", "dib123", "reboot"],
        ScriptedResponse::ok("OKAY\n"),
    );
    let transport = open_transport(Arc::clone(&runner), TransportConfig::for_tests());
    let outcome = transport
        .reboot(
            &WriteToken::mint(),
            "dib123",
            Mode::Fastboot,
            RebootTarget::System,
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        WaitOutcome::WrongMode {
            actual: Mode::Recovery
        }
    );

    let runner = Arc::new(ScriptedRunner::new());
    script_lists(&runner, "dib123 device\n", "");
    runner.on(
        fastboot_name(),
        &["-s", "dib123", "reboot"],
        ScriptedResponse::ok("OKAY\n"),
    );
    runner.on(
        adb_name(),
        &["-s", "dib123", "shell", "getprop", "sys.boot_completed"],
        ScriptedResponse::ok("0\n"),
    );
    let transport = open_transport(runner, TransportConfig::for_tests());
    let outcome = transport
        .reboot(
            &WriteToken::mint(),
            "dib123",
            Mode::Fastboot,
            RebootTarget::System,
        )
        .await
        .unwrap();
    assert_eq!(outcome, WaitOutcome::TimedOut);
}

#[tokio::test]
async fn flash_argv_has_no_slot_all() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        fastboot_name(),
        &["-s", "dib123", "--slot", "b", "flash", "init_boot"],
        ScriptedResponse::ok("OKAY\nFinished.\n"),
    );
    let transport = open_transport(Arc::clone(&runner), TransportConfig::for_tests());
    let image = PathBuf::from("/tmp/init_boot.img");
    transport
        .fastboot_flash(
            &WriteToken::mint(),
            "dib123",
            Slot::B,
            Partition::InitBoot,
            &image,
        )
        .await
        .unwrap();
    let vbmeta = transport
        .fastboot_flash(
            &WriteToken::mint(),
            "dib123",
            Slot::A,
            Partition::Vbmeta,
            &image,
        )
        .await
        .unwrap_err();
    assert!(vbmeta.to_string().contains("read-only"));
    for call in runner.calls() {
        assert!(!contains_slot_all(&call.args));
    }
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
            &["-s", &serial, "shell", "getprop"],
            ScriptedResponse::ok("[ro.product.device]: [shiba]\n[ro.boot.slot_suffix]: [_a]\n[ro.boot.flash.locked]: [0]\n")
                .with_delay(Duration::from_millis(120)),
        );
        runner.on(
            adb_name(),
            &["-s", &serial, "shell"],
            ScriptedResponse::ok("uid=0\n"),
        );
    }
    script_lists(&runner, &adb_list, "");
    let transport = open_transport(Arc::clone(&runner), TransportConfig::for_tests());
    let (aliases, devices) = common::catalogues();
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
