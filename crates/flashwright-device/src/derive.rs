// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Slot and lock rules.
//!
//! An unknown slot is an error. It is never treated as slot A.
//! Unlock checks compare strings: `ro.boot.flash.locked == "0"` or
//! `ro.boot.verifiedbootstate == "orange"`. Fastboot `unlocked: yes` wins
//! when the phone is in the bootloader.

use flashwright_proc::RunResult;

use crate::parse::PropMap;
use crate::{LockState, RootState, Slot};

pub fn parse_slot(raw: &str) -> Option<Slot> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.trim_start_matches('_') {
        "a" | "A" => Some(Slot::A),
        "b" | "B" => Some(Slot::B),
        _ => None,
    }
}

pub fn lock_from_adb(props: &PropMap) -> LockState {
    let locked = props.get("ro.boot.flash.locked").map(String::as_str);
    let verified = props.get("ro.boot.verifiedbootstate").map(String::as_str);
    if locked == Some("0") || verified == Some("orange") {
        LockState::Unlocked
    } else if locked == Some("1") || verified == Some("green") {
        LockState::Locked
    } else {
        LockState::Unknown
    }
}

pub fn lock_from_fastboot(vars: &PropMap) -> LockState {
    match vars.get("unlocked").map(String::as_str) {
        Some("yes") => LockState::Unlocked,
        Some("no") => LockState::Locked,
        _ => LockState::Unknown,
    }
}

pub fn interpret_su(result: &RunResult) -> RootState {
    if result.timed_out || result.killed_by_watchdog {
        return RootState::RootUnknown {
            reason: "su timed out".into(),
        };
    }
    let combined = format!("{}{}", result.stdout_text(), result.stderr_text());
    if combined.contains("uid=0") {
        return RootState::Rooted;
    }
    let lower = combined.to_ascii_lowercase();
    if lower.contains("denied") {
        return RootState::RootUnknown {
            reason: "su denied".into(),
        };
    }
    if lower.contains("not found") {
        return RootState::RootUnknown {
            reason: "su not found".into(),
        };
    }
    RootState::RootUnknown {
        reason: "su did not report uid=0".into(),
    }
}

pub fn prop(map: &PropMap, key: &str) -> Option<String> {
    let value = map.get(key)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn init_boot_from_ls(result: &RunResult) -> Option<bool> {
    let text = format!("{}{}", result.stdout_text(), result.stderr_text());
    let lower = text.to_ascii_lowercase();
    if lower.contains("no such file") {
        return Some(false);
    }
    if result.success_exit() && text.contains("init_boot_a") {
        return Some(true);
    }
    None
}

pub fn init_boot_from_size(value: Option<&str>) -> Option<bool> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u64>().ok()
    }?;
    Some(parsed > 0)
}
