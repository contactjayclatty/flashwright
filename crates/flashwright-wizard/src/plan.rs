// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::device::{inactive_slot, Partition, Slot};
use crate::error::CoreError;

pub const SCHEMA: &str = "flashwright.plan/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepClass {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Adb,
    Fastboot,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub idx: u32,
    pub id: String,
    pub class: StepClass,
    pub tool: Tool,
    pub argv: Vec<String>,
    pub timeout_s: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateView {
    pub id: String,
    pub severity: String,
    pub status: String,
    pub title: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Ota,
    FactoryKeepData,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ota => "ota",
            Self::FactoryKeepData => "factory_keep_data",
        }
    }
}

/// Fields covered by the plan hash. `plan_hash` itself is omitted.
#[derive(Debug, Serialize)]
pub struct PlanBody {
    pub schema: &'static str,
    pub plan_id: Uuid,
    pub nonce_hex: String,
    pub created_unix_ms: i64,
    pub expires_unix_ms: i64,
    pub serial: String,
    pub codename: String,
    pub fingerprint: String,
    pub spl: String,
    pub active_slot: Slot,
    pub target_slot: Slot,
    pub bootloader_unlocked: bool,
    pub route: Route,
    pub firmware_name: String,
    pub firmware_sha256: String,
    pub target_partition: Partition,
    pub backup_set_id: String,
    pub gates: Vec<GateView>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanPreview {
    pub plan_hash: String,
    pub plan_code: String,
    pub expires_unix_ms: i64,
    pub route: Route,
    pub target_slot: Slot,
    pub codename: String,
    pub build_id: String,
    pub backup_set_id: String,
    pub gates: Vec<GateView>,
    pub steps: Vec<Step>,
    pub prefer_dry_run: bool,
}

pub fn plan_hash(body: &PlanBody) -> Result<String, CoreError> {
    let canonical = serde_jcs::to_string(body)
        .map_err(|err| CoreError::message(format!("Could not canonicalise the plan: {err}")))?;
    let digest = Sha256::digest(canonical.as_bytes());
    Ok(format!("flp1-{}", hex::encode(digest)))
}

pub fn plan_code(plan_hash: &str) -> String {
    let hex_body = plan_hash.trim_start_matches("flp1-");
    let eight: String = hex_body.chars().take(8).collect();
    if eight.len() < 8 {
        return format!("PLAN {eight}");
    }
    format!("PLAN {}·{}", &eight[..4], &eight[4..])
}

pub fn quote_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg.contains(' ') || arg.contains('"') {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn step(idx: u32, id: &str, class: StepClass, tool: Tool, argv: &[&str], timeout_s: u32) -> Step {
    Step {
        idx,
        id: id.to_string(),
        class,
        tool,
        argv: argv.iter().map(|part| (*part).to_string()).collect(),
        timeout_s,
    }
}

pub fn ota_steps(serial: &str, target: Slot, ota_path: &str, patched: &str) -> Vec<Step> {
    let slot = target.as_str();
    vec![
        step(
            1,
            "preflight",
            StepClass::Read,
            Tool::Adb,
            &["adb", "-s", serial, "shell", "getprop"],
            30,
        ),
        step(
            2,
            "backup",
            StepClass::Read,
            Tool::Internal,
            &["backup", "verify", serial],
            600,
        ),
        step(
            3,
            "reboot_sideload",
            StepClass::Write,
            Tool::Adb,
            &["adb", "-s", serial, "reboot", "sideload"],
            120,
        ),
        step(
            4,
            "sideload",
            StepClass::Write,
            Tool::Adb,
            &["adb", "-s", serial, "sideload", ota_path],
            2400,
        ),
        step(
            5,
            "reboot_bootloader",
            StepClass::Write,
            Tool::Adb,
            &["adb", "-s", serial, "reboot", "bootloader"],
            120,
        ),
        step(
            6,
            "verify_slot",
            StepClass::Read,
            Tool::Fastboot,
            &["fastboot", "-s", serial, "getvar", "current-slot"],
            10,
        ),
        step(
            7,
            "flash_patched",
            StepClass::Write,
            Tool::Fastboot,
            &[
                "fastboot",
                "-s",
                serial,
                "--slot",
                slot,
                "flash",
                "init_boot",
                patched,
            ],
            120,
        ),
        step(
            8,
            "reboot_system",
            StepClass::Write,
            Tool::Fastboot,
            &["fastboot", "-s", serial, "reboot"],
            900,
        ),
        step(
            9,
            "verify_root",
            StepClass::Read,
            Tool::Adb,
            &["adb", "-s", serial, "shell", "id"],
            120,
        ),
    ]
}

pub fn factory_steps(serial: &str, target: Slot, package: &str, patched: &str) -> Vec<Step> {
    let slot = target.as_str();
    vec![
        step(
            1,
            "preflight",
            StepClass::Read,
            Tool::Adb,
            &["adb", "-s", serial, "shell", "getprop"],
            30,
        ),
        step(
            2,
            "backup",
            StepClass::Read,
            Tool::Internal,
            &["backup", "verify", serial],
            600,
        ),
        step(
            3,
            "reboot_bootloader",
            StepClass::Write,
            Tool::Adb,
            &["adb", "-s", serial, "reboot", "bootloader"],
            90,
        ),
        step(
            4,
            "verify_unlock",
            StepClass::Read,
            Tool::Fastboot,
            &["fastboot", "-s", serial, "getvar", "unlocked"],
            10,
        ),
        step(
            5,
            "flash_bootloader",
            StepClass::Write,
            Tool::Fastboot,
            &[
                "fastboot",
                "-s",
                serial,
                "--slot",
                slot,
                "flash",
                "bootloader",
                "bootloader.img",
            ],
            300,
        ),
        step(
            6,
            "reboot_bootloader",
            StepClass::Write,
            Tool::Fastboot,
            &["fastboot", "-s", serial, "reboot", "bootloader"],
            90,
        ),
        step(
            7,
            "flash_radio",
            StepClass::Write,
            Tool::Fastboot,
            &[
                "fastboot",
                "-s",
                serial,
                "--slot",
                slot,
                "flash",
                "radio",
                "radio.img",
            ],
            300,
        ),
        step(
            8,
            "reboot_bootloader",
            StepClass::Write,
            Tool::Fastboot,
            &["fastboot", "-s", serial, "reboot", "bootloader"],
            90,
        ),
        step(
            9,
            "update_slot",
            StepClass::Write,
            Tool::Fastboot,
            &[
                "fastboot",
                "-s",
                serial,
                "--slot",
                slot,
                "--skip-reboot",
                "update",
                package,
            ],
            1800,
        ),
        step(
            10,
            "set_active",
            StepClass::Write,
            Tool::Fastboot,
            &["fastboot", "-s", serial, &format!("--set-active={slot}")],
            10,
        ),
        step(
            11,
            "flash_patched",
            StepClass::Write,
            Tool::Fastboot,
            &[
                "fastboot",
                "-s",
                serial,
                "--slot",
                slot,
                "flash",
                "init_boot",
                patched,
            ],
            120,
        ),
        step(
            12,
            "reboot_system",
            StepClass::Write,
            Tool::Fastboot,
            &["fastboot", "-s", serial, "reboot"],
            900,
        ),
        step(
            13,
            "verify_root",
            StepClass::Read,
            Tool::Adb,
            &["adb", "-s", serial, "shell", "id"],
            120,
        ),
    ]
}

pub fn assert_no_forbidden_args(steps: &[Step]) -> Result<(), CoreError> {
    for step in steps {
        let joined = step.argv.join(" ");
        if joined.contains("--slot all")
            || step
                .argv
                .windows(2)
                .any(|pair| pair[0] == "--slot" && pair[1] == "all")
            || step.argv.iter().any(|arg| {
                arg == "-w" || arg == "--disable-verity" || arg == "--disable-verification"
            })
        {
            return Err(CoreError::message(
                "The plan contained a forbidden argument.",
            ));
        }
        if step.class == StepClass::Write
            && step.tool == Tool::Fastboot
            && step.id.starts_with("flash")
        {
            let has_slot = step
                .argv
                .windows(2)
                .any(|pair| pair[0] == "--slot" && (pair[1] == "a" || pair[1] == "b"));
            if !has_slot {
                return Err(CoreError::message(
                    "A flash step is missing an explicit slot.",
                ));
            }
        }
    }
    Ok(())
}

pub fn target_for(active: Option<Slot>) -> Result<Slot, CoreError> {
    inactive_slot(active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_is_display_only() {
        let shown = quote_argv(&[
            "fastboot".into(),
            "--slot".into(),
            "b".into(),
            "my file.img".into(),
        ]);
        assert_eq!(shown, "fastboot --slot b \"my file.img\"");
    }
}
