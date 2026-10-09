// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use flashwright_device::{AliasTable, DeviceTable, PlatformToolsTransport, TransportConfig};
use flashwright_proc::{ScriptedResponse, ScriptedRunner};

pub fn adb_name() -> &'static str {
    if cfg!(windows) {
        "adb.exe"
    } else {
        "adb"
    }
}

pub fn fastboot_name() -> &'static str {
    if cfg!(windows) {
        "fastboot.exe"
    } else {
        "fastboot"
    }
}

pub fn tool_path(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"C:\flashwright-test\{name}"))
    } else {
        PathBuf::from(format!("/opt/flashwright-test/{name}"))
    }
}

pub fn transport(
    runner: Arc<ScriptedRunner>,
    config: TransportConfig,
) -> PlatformToolsTransport<ScriptedRunner> {
    PlatformToolsTransport::new(
        runner,
        tool_path(adb_name()),
        tool_path(fastboot_name()),
        config,
    )
}

pub fn catalogues() -> (AliasTable, DeviceTable) {
    (
        AliasTable::embedded().expect("aliases"),
        DeviceTable::embedded().expect("devices"),
    )
}

pub fn script_lists(runner: &ScriptedRunner, adb_text: &str, fastboot_text: &str) {
    runner.on(
        adb_name(),
        &["devices", "-l"],
        ScriptedResponse::ok(adb_text),
    );
    runner.on(
        fastboot_name(),
        &["devices", "-l"],
        ScriptedResponse::ok(fastboot_text),
    );
}

pub fn script_adb_probe(
    runner: &ScriptedRunner,
    serial: &str,
    props: &str,
    su: &str,
    ls_exit: i32,
    ls_stdout: &str,
    ls_stderr: &str,
) {
    runner.on(
        adb_name(),
        &["-s", serial, "shell", "getprop"],
        ScriptedResponse::ok(props),
    );
    runner.on(
        adb_name(),
        &["-s", serial, "shell", "su", "-c", "id"],
        ScriptedResponse::ok(su),
    );
    runner.on(
        adb_name(),
        &["-s", serial, "shell", "magisk", "-v"],
        ScriptedResponse::ok("27.0\n"),
    );
    runner.on(
        adb_name(),
        &["-s", serial, "shell", "magisk", "-V"],
        ScriptedResponse::ok("27000\n"),
    );
    runner.on(
        adb_name(),
        &[
            "-s",
            serial,
            "shell",
            "dumpsys",
            "package",
            "com.topjohnwu.magisk",
        ],
        ScriptedResponse::ok("    versionName=27.0\n    versionCode=27000 minSdk=26\n"),
    );
    runner.on(
        adb_name(),
        &["-s", serial, "shell", "dumpsys", "battery"],
        ScriptedResponse::ok(
            "  level: 80\n  AC powered: false\n  USB powered: true\n  Wireless powered: false\n",
        ),
    );
    let ls = if ls_exit == 0 {
        ScriptedResponse::ok(ls_stdout)
    } else {
        ScriptedResponse::fail(ls_exit, ls_stderr).with_stdout(ls_stdout)
    };
    runner.on(
        adb_name(),
        &[
            "-s",
            serial,
            "shell",
            "ls",
            "/dev/block/by-name/init_boot_a",
        ],
        ls,
    );
}

pub const SHIBA_PROPS: &str = "\
[ro.product.device]: [shiba]
[ro.product.model]: [Pixel 8]
[ro.build.id]: [UD1A.231105.004]
[ro.build.fingerprint]: [google/shiba/shiba:14/UD1A.231105.004/11010374:user/release-keys]
[ro.build.date.utc]: [1699000000]
[ro.build.version.sdk]: [34]
[ro.build.version.security_patch]: [2023-11-05]
[ro.boot.slot_suffix]: [_b]
[ro.boot.flash.locked]: [0]
[ro.boot.verifiedbootstate]: [orange]
[ro.bootloader]: [ripcurrent-14.0-11010374]
";

pub const ORIOLE_PROPS: &str = "\
[ro.product.device]: [oriole]
[ro.product.model]: [Pixel 6]
[ro.build.id]: [AP1A.240505.005]
[ro.build.fingerprint]: [google/oriole/oriole:14/AP1A.240505.005/11668284:user/release-keys]
[ro.build.date.utc]: [1714000000]
[ro.build.version.sdk]: [34]
[ro.build.version.security_patch]: [2024-05-05]
[ro.boot.slot_suffix]: [_a]
[ro.boot.flash.locked]: [1]
[ro.boot.verifiedbootstate]: [orange]
[ro.bootloader]: [slider-14.0-11668284]
";
