// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use serde::{Deserialize, Serialize};

use super::device::{Partition, Slot};
use crate::CoreError;

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

pub fn ota_steps(
    serial: &str,
    target: Slot,
    partition: Partition,
    ota_path: &str,
    patched: &str,
) -> Vec<Step> {
    let slot = target.as_str();
    let part = partition.as_str();
    vec![
        step(1, "preflight", StepClass::Read, Tool::Adb, &["adb", "-s", serial, "shell", "getprop"], 30),
        step(2, "backup", StepClass::Read, Tool::Internal, &["backup", "verify", serial], 600),
        step(3, "reboot_sideload", StepClass::Write, Tool::Adb, &["adb", "-s", serial, "reboot", "sideload"], 120),
        step(4, "sideload", StepClass::Write, Tool::Adb, &["adb", "-s", serial, "sideload", ota_path], 2400),
        step(5, "reboot_bootloader", StepClass::Write, Tool::Adb, &["adb", "-s", serial, "reboot", "bootloader"], 120),
        step(6, "verify_slot", StepClass::Read, Tool::Fastboot, &["fastboot", "-s", serial, "getvar", "current-slot"], 10),
        step(
            7,
            "flash_patched",
            StepClass::Write,
            Tool::Fastboot,
            &["fastboot", "-s", serial, "--slot", slot, "flash", part, patched],
            120,
        ),
        step(8, "reboot_system", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, "reboot"], 900),
        step(
            9,
            "verify_root",
            StepClass::Read,
            Tool::Adb,
            &["adb", "-s", serial, "shell", "su", "-c", "id"],
            120,
        ),
    ]
}

pub fn factory_steps(
    serial: &str,
    target: Slot,
    partition: Partition,
    package: &str,
    patched: &str,
) -> Vec<Step> {
    let slot = target.as_str();
    let part = partition.as_str();
    vec![
        step(1, "preflight", StepClass::Read, Tool::Adb, &["adb", "-s", serial, "shell", "getprop"], 30),
        step(2, "backup", StepClass::Read, Tool::Internal, &["backup", "verify", serial], 600),
        step(3, "reboot_bootloader", StepClass::Write, Tool::Adb, &["adb", "-s", serial, "reboot", "bootloader"], 90),
        step(4, "verify_unlock", StepClass::Read, Tool::Fastboot, &["fastboot", "-s", serial, "getvar", "unlocked"], 10),
        step(5, "flash_bootloader", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, "--slot", slot, "flash", "bootloader", "bootloader.img"], 300),
        step(6, "reboot_bootloader", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, "reboot", "bootloader"], 90),
        step(7, "flash_radio", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, "--slot", slot, "flash", "radio", "radio.img"], 300),
        step(8, "reboot_bootloader", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, "reboot", "bootloader"], 90),
        step(9, "update_slot", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, "--slot", slot, "--skip-reboot", "update", package], 1800),
        step(10, "set_active", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, &format!("--set-active={slot}")], 10),
        step(11, "flash_patched", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, "--slot", slot, "flash", part, patched], 120),
        step(12, "reboot_system", StepClass::Write, Tool::Fastboot, &["fastboot", "-s", serial, "reboot"], 900),
        step(13, "verify_root", StepClass::Read, Tool::Adb, &["adb", "-s", serial, "shell", "su", "-c", "id"], 120),
    ]
}

pub fn prepare_patch_steps(serial: &str, image: &str) -> Vec<Step> {
    vec![
        step(1, "push_stock", StepClass::Write, Tool::Adb, &["adb", "-s", serial, "push", image, "/data/local/tmp/flashwright/stock.img"], 120),
        step(2, "run_patcher", StepClass::Write, Tool::Adb, &["adb", "-s", serial, "shell", "su", "-c", "sh /data/local/tmp/flashwright/patch.sh"], 180),
        step(3, "pull_patched", StepClass::Read, Tool::Adb, &["adb", "-s", serial, "pull", "/data/local/tmp/flashwright/patched.img"], 120),
        step(4, "cleanup", StepClass::Write, Tool::Adb, &["adb", "-s", serial, "shell", "rm", "-rf", "/data/local/tmp/flashwright"], 30),
    ]
}

pub fn assert_no_forbidden_args(steps: &[Step]) -> Result<(), CoreError> {
    for step in steps {
        let joined = step.argv.join(" ");
        if joined.contains("--slot all")
            || step.argv.windows(2).any(|pair| pair[0] == "--slot" && pair[1] == "all")
            || step.argv.iter().any(|arg| arg == "-w" || arg == "--disable-verity" || arg == "--disable-verification")
        {
            return Err(CoreError::Rejected {
                reason: "The plan contained a forbidden argument.".to_string(),
            });
        }
        if step.class == StepClass::Write && step.tool == Tool::Fastboot && step.id.starts_with("flash") {
            let has_slot = step
                .argv
                .windows(2)
                .any(|pair| pair[0] == "--slot" && (pair[1] == "a" || pair[1] == "b"));
            if !has_slot {
                return Err(CoreError::Rejected {
                    reason: "A flash step is missing an explicit slot.".to_string(),
                });
            }
        }
    }
    Ok(())
}
