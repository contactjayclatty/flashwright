// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::collections::BTreeMap;

use crate::device::{Battery, Mode, ScanEntry};

pub type PropMap = BTreeMap<String, String>;

pub fn parse_mode_token(token: &str) -> Option<Mode> {
    match token {
        "device" => Some(Mode::Adb),
        "recovery" => Some(Mode::Recovery),
        "sideload" => Some(Mode::Sideload),
        "rescue" => Some(Mode::Rescue),
        "fastboot" => Some(Mode::Fastboot),
        "fastbootd" => Some(Mode::Fastbootd),
        "unauthorized" => Some(Mode::Unauthorized),
        "authorizing" => Some(Mode::Authorizing),
        "offline" => Some(Mode::Offline),
        _ => None,
    }
}

pub fn parse_adb_devices(text: &str) -> Vec<ScanEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("List of devices") || line.starts_with('*') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let serial = parts[0];
        let (mode, raw, rest) = if parts.len() >= 3 && parts[1] == "no" && parts[2] == "permissions"
        {
            (
                Mode::NoPermissions,
                "no permissions".to_string(),
                &parts[3..],
            )
        } else {
            let state = parts.get(1).copied().unwrap_or("");
            let mode = parse_mode_token(state).unwrap_or(Mode::Unrecognized);
            (mode, state.to_string(), &parts[2..])
        };
        let mut transport_id = None;
        for extra in rest {
            if let Some(value) = extra.strip_prefix("transport_id:") {
                transport_id = Some(value.to_string());
            }
        }
        out.push(ScanEntry {
            serial: serial.to_string(),
            mode,
            transport_id,
            raw_state: raw,
        });
    }
    out
}

pub fn parse_fastboot_devices(text: &str) -> Vec<ScanEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let serial = parts[0];
        let (mode, raw) = if parts.len() >= 3 && parts[1] == "no" && parts[2] == "permissions" {
            (Mode::NoPermissions, "no permissions".to_string())
        } else {
            let state = parts.get(1).copied().unwrap_or("");
            let mode = parse_mode_token(state).unwrap_or(Mode::Unrecognized);
            (mode, state.to_string())
        };
        out.push(ScanEntry {
            serial: serial.to_string(),
            mode,
            transport_id: None,
            raw_state: raw,
        });
    }
    out
}

/// Merge adb and fastboot rows. A serial listed by both keeps the fastboot mode.
pub fn merge_scans(adb: Vec<ScanEntry>, fastboot: Vec<ScanEntry>) -> Vec<ScanEntry> {
    let mut out = adb;
    for entry in fastboot {
        if let Some(existing) = out.iter_mut().find(|row| row.serial == entry.serial) {
            if entry.mode.is_fastboot_family() || entry.mode == Mode::NoPermissions {
                *existing = entry;
            }
        } else {
            out.push(entry);
        }
    }
    out
}

pub fn parse_getprop(text: &str) -> PropMap {
    let mut map = PropMap::new();
    for line in text.lines() {
        let line = line.trim().trim_end_matches('\r');
        if !line.starts_with('[') {
            continue;
        }
        let Some(marker) = line.find("]: [") else {
            continue;
        };
        let key = &line[1..marker];
        let rest = &line[marker + 4..];
        let value = rest.strip_suffix(']').unwrap_or(rest);
        if !key.is_empty() {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

pub fn parse_getvar(stdout: &str, stderr: &str) -> PropMap {
    let mut map = PropMap::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim().trim_end_matches('\r');
        if line.is_empty() || line.starts_with("Finished.") || line.starts_with("< waiting") {
            continue;
        }
        let line = line.strip_prefix("(bootloader)").unwrap_or(line).trim();
        if line.starts_with("FAILED") || line.starts_with("OKAY") {
            continue;
        }
        let Some((key, value)) = line.rsplit_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        map.insert(key.to_string(), value.trim().to_string());
    }
    map
}

pub fn parse_battery(text: &str) -> Battery {
    let mut level = None;
    let mut ac = None;
    let mut usb = None;
    let mut wireless = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("level:") {
            level = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("AC powered:") {
            ac = parse_bool(value);
        } else if let Some(value) = line.strip_prefix("USB powered:") {
            usb = parse_bool(value);
        } else if let Some(value) = line.strip_prefix("Wireless powered:") {
            wireless = parse_bool(value);
        }
    }
    let charging = match (ac, usb, wireless) {
        (None, None, None) => None,
        _ => Some(ac == Some(true) || usb == Some(true) || wireless == Some(true)),
    };
    Battery { level, charging }
}

pub fn parse_dumpsys_package(text: &str) -> (Option<String>, Option<u32>) {
    let mut name = None;
    let mut code = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("versionName=") {
            if name.is_none() {
                name = Some(rest.trim().to_string());
            }
        }
        if let Some(index) = line.find("versionCode=") {
            if code.is_none() {
                let rest = &line[index + "versionCode=".len()..];
                let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
                code = digits.parse().ok();
            }
        }
    }
    (name, code)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}
