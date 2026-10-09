// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Assemble a [`DeviceInfo`] from adb or fastboot text.
//!
//! The transport collects the text. This module only applies the slot, lock,
//! and init_boot rules, so the field mapping can be checked on fixtures.

use crate::proc::RunResult;

use crate::device::catalog::{AliasTable, DeviceTable};
use crate::device::derive::{
    init_boot_from_ls, init_boot_from_size, interpret_su, lock_from_adb, lock_from_fastboot,
    parse_slot, prop,
};
use crate::device::parse::{parse_battery, parse_dumpsys_package, parse_getprop, parse_getvar};
use crate::device::{BootTarget, DeviceInfo, InitBootPresence, Mode, RootState};

pub struct AdbTexts {
    pub props: String,
    pub su: RunResult,
    pub magisk_version: Option<String>,
    pub magisk_code_text: Option<String>,
    pub dumpsys_package: Option<String>,
    pub battery: Option<String>,
    pub init_boot_ls: Option<RunResult>,
}

pub fn assemble_from_adb(
    serial: &str,
    mode: Mode,
    transport_id: Option<String>,
    texts: AdbTexts,
    aliases: &AliasTable,
    devices: &DeviceTable,
) -> DeviceInfo {
    let props = parse_getprop(&texts.props);
    let raw = prop(&props, "ro.product.device");
    let codename = canonical_codename(aliases, raw.as_deref());
    let live = texts.init_boot_ls.as_ref().and_then(init_boot_from_ls);
    let init_boot = resolve_init_boot(live, codename.as_deref(), devices);
    let (app_version, app_code) = texts
        .dumpsys_package
        .as_deref()
        .map(parse_dumpsys_package)
        .unwrap_or((None, None));
    DeviceInfo {
        serial: serial.to_string(),
        mode,
        model: prop(&props, "ro.product.model"),
        codename,
        codename_raw: raw,
        build_id: prop(&props, "ro.build.id"),
        fingerprint: prop(&props, "ro.build.fingerprint"),
        build_date_utc: prop(&props, "ro.build.date.utc"),
        sdk: prop(&props, "ro.build.version.sdk"),
        spl: prop(&props, "ro.build.version.security_patch"),
        active_slot: prop(&props, "ro.boot.slot_suffix").and_then(|value| parse_slot(&value)),
        lock: lock_from_adb(&props),
        bootloader_version: prop(&props, "ro.bootloader"),
        root: interpret_su(&texts.su),
        magisk_version: clean_line(texts.magisk_version),
        magisk_code: texts.magisk_code_text.as_deref().and_then(first_u32),
        magisk_app_version: app_version,
        magisk_app_code: app_code,
        init_boot,
        boot_target: boot_target(init_boot),
        battery: texts.battery.as_deref().map(parse_battery),
        transport_id,
    }
}

pub fn assemble_from_fastboot(
    serial: &str,
    mode: Mode,
    transport_id: Option<String>,
    stdout: &str,
    stderr: &str,
    aliases: &AliasTable,
    devices: &DeviceTable,
) -> DeviceInfo {
    let vars = parse_getvar(stdout, stderr);
    let raw = prop(&vars, "product");
    let codename = canonical_codename(aliases, raw.as_deref());
    let live = init_boot_from_size(vars.get("partition-size:init_boot_a").map(String::as_str));
    let init_boot = resolve_init_boot(live, codename.as_deref(), devices);
    DeviceInfo {
        serial: serial.to_string(),
        mode,
        model: None,
        codename,
        codename_raw: raw,
        build_id: None,
        fingerprint: None,
        build_date_utc: None,
        sdk: None,
        spl: None,
        active_slot: prop(&vars, "current-slot").and_then(|value| parse_slot(&value)),
        lock: lock_from_fastboot(&vars),
        bootloader_version: prop(&vars, "version-bootloader"),
        root: RootState::RootUnknown {
            reason: "not in adb mode".into(),
        },
        magisk_version: None,
        magisk_code: None,
        magisk_app_version: None,
        magisk_app_code: None,
        init_boot,
        boot_target: boot_target(init_boot),
        battery: None,
        transport_id,
    }
}

fn canonical_codename(aliases: &AliasTable, raw: Option<&str>) -> Option<String> {
    raw.map(|name| aliases.canonical(name).to_string())
}

fn resolve_init_boot(
    live: Option<bool>,
    codename: Option<&str>,
    devices: &DeviceTable,
) -> InitBootPresence {
    match live {
        Some(true) => InitBootPresence::Present,
        Some(false) => InitBootPresence::Absent,
        None => match codename.and_then(|name| devices.get(name)) {
            Some(row) if row.has_init_boot => InitBootPresence::Present,
            Some(_) => InitBootPresence::Absent,
            None => InitBootPresence::Unknown,
        },
    }
}

fn boot_target(presence: InitBootPresence) -> BootTarget {
    match presence {
        InitBootPresence::Present => BootTarget::InitBoot,
        InitBootPresence::Absent => BootTarget::Boot,
        InitBootPresence::Unknown => BootTarget::Unknown,
    }
}

fn clean_line(value: Option<String>) -> Option<String> {
    let trimmed = value?.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn first_u32(text: &str) -> Option<u32> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}
