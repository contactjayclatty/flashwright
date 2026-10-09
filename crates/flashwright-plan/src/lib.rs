// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Pure plan logic: gates, confirm copy, and the plan hash.
//!
//! This crate does not spawn processes and does not mint write tokens.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const G20_ACK: &str = "A pending system update may undo root after it installs.";
pub const PREPARE_PATCH_TITLE: &str = "Patch on your phone?";
pub const PREPARE_PATCH_BUTTON: &str = "Patch now";
pub const FLASH_BUTTON: &str = "Flash now";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GateId {
    G01,
    G02,
    G03,
    G04,
    G05,
    G06,
    G07,
    G08,
    G09,
    G10,
    G11,
    G12,
    G13,
    G14,
    G15,
    G16,
    G17,
    G18,
    G19,
    G20,
    G21,
    G22,
}

impl GateId {
    pub fn all() -> &'static [GateId] {
        &[
            Self::G01,
            Self::G02,
            Self::G03,
            Self::G04,
            Self::G05,
            Self::G06,
            Self::G07,
            Self::G08,
            Self::G09,
            Self::G10,
            Self::G11,
            Self::G12,
            Self::G13,
            Self::G14,
            Self::G15,
            Self::G16,
            Self::G17,
            Self::G18,
            Self::G19,
            Self::G20,
            Self::G21,
            Self::G22,
        ]
    }
}

impl fmt::Display for GateId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::G01 => "G01",
            Self::G02 => "G02",
            Self::G03 => "G03",
            Self::G04 => "G04",
            Self::G05 => "G05",
            Self::G06 => "G06",
            Self::G07 => "G07",
            Self::G08 => "G08",
            Self::G09 => "G09",
            Self::G10 => "G10",
            Self::G11 => "G11",
            Self::G12 => "G12",
            Self::G13 => "G13",
            Self::G14 => "G14",
            Self::G15 => "G15",
            Self::G16 => "G16",
            Self::G17 => "G17",
            Self::G18 => "G18",
            Self::G19 => "G19",
            Self::G20 => "G20",
            Self::G21 => "G21",
            Self::G22 => "G22",
        };
        formatter.write_str(name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Block,
    Ack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    pub id: GateId,
    pub severity: Severity,
    pub passed: bool,
    pub message: String,
}

pub struct GateInput {
    pub tools_allow_writes: bool,
    pub one_device: bool,
    pub unlocked: bool,
    pub codename_match: bool,
    pub sha256_match: bool,
    pub full_ota: bool,
    pub no_downgrade: bool,
    pub spl_match: bool,
    pub patched_sha1_match: bool,
    pub magisk_ok: bool,
    pub battery_ok: bool,
    pub host_space_ok: bool,
    pub device_space_ok: bool,
    pub backup_ok: bool,
    pub partition_ok: bool,
    pub keep_data_clean: bool,
    pub driver_ok: bool,
    pub bootloader_ok: bool,
    pub min_bootloader_ok: bool,
    pub exe_hash_ok: bool,
    pub adb_server_ok: bool,
}

impl GateInput {
    pub fn passing() -> Self {
        Self {
            tools_allow_writes: true,
            one_device: true,
            unlocked: true,
            codename_match: true,
            sha256_match: true,
            full_ota: true,
            no_downgrade: true,
            spl_match: true,
            patched_sha1_match: true,
            magisk_ok: true,
            battery_ok: true,
            host_space_ok: true,
            device_space_ok: true,
            backup_ok: true,
            partition_ok: true,
            keep_data_clean: true,
            driver_ok: true,
            bootloader_ok: true,
            min_bootloader_ok: true,
            exe_hash_ok: true,
            adb_server_ok: true,
        }
    }
}

pub fn evaluate_gates(input: &GateInput) -> Vec<GateResult> {
    let rows = [
        (
            GateId::G01,
            Severity::Block,
            input.tools_allow_writes,
            "Platform-tools are not write-enabled.",
        ),
        (
            GateId::G02,
            Severity::Block,
            input.one_device,
            "Select one authorised device.",
        ),
        (
            GateId::G03,
            Severity::Block,
            input.unlocked,
            "The bootloader is locked.",
        ),
        (
            GateId::G04,
            Severity::Block,
            input.codename_match,
            "The firmware codename does not match this phone.",
        ),
        (
            GateId::G05,
            Severity::Block,
            input.sha256_match,
            "The firmware SHA-256 does not match.",
        ),
        (
            GateId::G06,
            Severity::Block,
            input.full_ota,
            "The package is not a full A/B update.",
        ),
        (
            GateId::G07,
            Severity::Block,
            input.no_downgrade,
            "The firmware is older than the phone.",
        ),
        (
            GateId::G08,
            Severity::Block,
            input.spl_match,
            "The image security patch does not match the firmware.",
        ),
        (
            GateId::G09,
            Severity::Block,
            input.patched_sha1_match,
            "The patched image does not match the stock image.",
        ),
        (
            GateId::G10,
            Severity::Block,
            input.magisk_ok,
            "This Magisk build is not accepted.",
        ),
        (
            GateId::G11,
            Severity::Block,
            input.battery_ok,
            "The battery is too low.",
        ),
        (
            GateId::G12,
            Severity::Block,
            input.host_space_ok,
            "The computer is short on free space.",
        ),
        (
            GateId::G13,
            Severity::Block,
            input.device_space_ok,
            "The phone is short on free space.",
        ),
        (
            GateId::G14,
            Severity::Block,
            input.backup_ok,
            "A verified backup is required.",
        ),
        (
            GateId::G15,
            Severity::Block,
            input.partition_ok,
            "The target partition is missing or too small.",
        ),
        (
            GateId::G16,
            Severity::Block,
            input.keep_data_clean,
            "The plan would wipe data or disable verification.",
        ),
        (
            GateId::G17,
            Severity::Ack,
            input.driver_ok,
            "The USB driver was not confirmed.",
        ),
        (
            GateId::G18,
            Severity::Block,
            input.bootloader_ok,
            "The firmware bootloader is older than the phone.",
        ),
        (
            GateId::G19,
            Severity::Ack,
            input.min_bootloader_ok,
            "The bootloader is below the recorded minimum.",
        ),
        (GateId::G20, Severity::Ack, true, G20_ACK),
        (
            GateId::G21,
            Severity::Block,
            input.exe_hash_ok,
            "Flashwright's copy of platform-tools changed on disk. Re-import it.",
        ),
        (
            GateId::G22,
            Severity::Block,
            input.adb_server_ok,
            "Restart adb before a write. The server on port 5037 is not the verified adb.",
        ),
    ];
    rows.into_iter()
        .map(|(id, severity, passed, message)| GateResult {
            id,
            severity,
            passed,
            message: if id == GateId::G20 {
                G20_ACK.to_string()
            } else if passed {
                String::new()
            } else {
                message.to_string()
            },
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AckError {
    pub id: GateId,
}

pub fn ack_gate(gates: &mut [GateResult], id: GateId) -> Result<(), AckError> {
    let Some(gate) = gates.iter_mut().find(|gate| gate.id == id) else {
        return Err(AckError { id });
    };
    if gate.severity == Severity::Block {
        return Err(AckError { id });
    }
    gate.passed = true;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfirmSpec {
    pub title: &'static str,
    pub button: &'static str,
    pub mnemonic: char,
    pub default_cancel: bool,
    pub delay: std::time::Duration,
}

pub fn prepare_patch_confirm() -> ConfirmSpec {
    ConfirmSpec {
        title: PREPARE_PATCH_TITLE,
        button: PREPARE_PATCH_BUTTON,
        mnemonic: 'P',
        default_cancel: true,
        delay: std::time::Duration::ZERO,
    }
}

pub fn flash_confirm() -> ConfirmSpec {
    ConfirmSpec {
        title: "Flash",
        button: FLASH_BUTTON,
        mnemonic: 'F',
        default_cancel: true,
        delay: std::time::Duration::from_secs(2),
    }
}

pub fn plan_hash(canonical_json: &[u8]) -> String {
    let digest = Sha256::digest(canonical_json);
    let mut hex = String::from("flp1-");
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

pub fn plan_code(hash: &str) -> String {
    let hex = hash.trim_start_matches("flp1-");
    let head = hex.get(..8).unwrap_or(hex);
    if head.len() >= 8 {
        format!("{}·{}", &head[..4], &head[4..8])
    } else {
        head.to_string()
    }
}

/// `DRY xxxx·xxxx` or `PLAN xxxx·xxxx`.
pub fn plan_label(hash: &str, dry_run: bool) -> String {
    let prefix = if dry_run { "DRY" } else { "PLAN" };
    format!("{prefix} {}", plan_code(hash))
}

/// SHA-256 over RFC 8785 canonical JSON, prefixed `flp1-`.
pub fn canonical_hash<T: Serialize>(value: &T) -> Result<String, String> {
    let bytes = serde_jcs::to_vec(value).map_err(|err| err.to_string())?;
    Ok(plan_hash(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gate_passes_and_fails() {
        let ok = evaluate_gates(&GateInput::passing());
        assert_eq!(ok.len(), 22);
        assert!(ok.iter().all(|gate| gate.passed));
        assert_eq!(
            ok.iter()
                .find(|gate| gate.id == GateId::G20)
                .unwrap()
                .message,
            G20_ACK
        );
        assert_eq!(
            ok.iter()
                .find(|gate| gate.id == GateId::G20)
                .unwrap()
                .severity,
            Severity::Ack
        );
        let mut bad = GateInput::passing();
        bad.exe_hash_ok = false;
        bad.adb_server_ok = false;
        bad.codename_match = false;
        let gates = evaluate_gates(&bad);
        assert!(
            !gates
                .iter()
                .find(|gate| gate.id == GateId::G21)
                .unwrap()
                .passed
        );
        assert!(
            !gates
                .iter()
                .find(|gate| gate.id == GateId::G22)
                .unwrap()
                .passed
        );
        assert_eq!(
            gates
                .iter()
                .find(|gate| gate.id == GateId::G21)
                .unwrap()
                .severity,
            Severity::Block
        );
    }

    #[test]
    fn block_gates_cannot_be_acked() {
        let mut gates = evaluate_gates(&GateInput::passing());
        for id in [
            GateId::G04,
            GateId::G05,
            GateId::G08,
            GateId::G09,
            GateId::G21,
            GateId::G22,
        ] {
            assert!(ack_gate(&mut gates, id).is_err());
        }
        assert!(ack_gate(&mut gates, GateId::G20).is_ok());
    }

    #[test]
    fn prepare_patch_cancel_is_the_default() {
        let spec = prepare_patch_confirm();
        assert_eq!(spec.title, "Patch on your phone?");
        assert_eq!(spec.button, "Patch now");
        assert_eq!(spec.mnemonic, 'P');
        assert!(spec.default_cancel);
        assert_eq!(spec.delay, std::time::Duration::ZERO);
        let flash = flash_confirm();
        assert_eq!(flash.button, "Flash now");
        assert_eq!(flash.mnemonic, 'F');
        assert_eq!(flash.delay, std::time::Duration::from_secs(2));
    }
}
