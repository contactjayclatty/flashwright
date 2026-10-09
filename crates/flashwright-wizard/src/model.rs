// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use serde::Serialize;

use crate::device::{DeviceInfo, DeviceSummary, Mode, Slot};
use crate::plan::{GateView, PlanPreview, Route};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Connect,
    Choose,
    Firmware,
    Review,
    Flash,
    Done,
    Recovery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolsStatus {
    pub version: String,
    pub classification: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriverStatus {
    pub state: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChoiceView {
    pub action: String,
    pub route: Route,
    pub prefer_dry_run: bool,
    pub slot_label: String,
    pub both_slots_enabled: bool,
    pub both_slots_reason: String,
    pub root_tool_label: String,
    pub root_tool_version: String,
    pub backup_folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirmwareReport {
    pub id: String,
    pub name: String,
    pub route: Route,
    pub sha256: String,
    pub codename: String,
    pub build_id: String,
    pub patched_ready: bool,
    pub partition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogLine {
    pub ts: String,
    pub level: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryOption {
    pub id: String,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobView {
    pub state: String,
    pub progress: u8,
    pub status_line: String,
    pub lines: Vec<LogLine>,
    pub result_title: String,
    pub result_body: String,
    pub recovery: Vec<RecoveryOption>,
    pub cancel_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupSet {
    pub set_id: String,
    pub label: String,
    pub size_label: String,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub level: String,
    pub message: String,
    pub gates: Vec<GateView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalLink {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub phase: Phase,
    pub tools: ToolsStatus,
    pub driver: DriverStatus,
    pub devices: Vec<DeviceSummary>,
    pub selected: Option<DeviceInfo>,
    pub choice: ChoiceView,
    pub firmware: Option<FirmwareReport>,
    pub plan: Option<PlanPreview>,
    pub job: JobView,
    pub backups: Vec<BackupSet>,
    pub notice: Option<Notice>,
    pub links: Vec<ExternalLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BurstStats {
    pub emitted: u32,
    pub cancelled: bool,
}

pub fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Adb => "device",
        Mode::Recovery => "recovery",
        Mode::Sideload => "sideload",
        Mode::Rescue => "rescue",
        Mode::Fastboot => "fastboot",
        Mode::Fastbootd => "fastbootd",
        Mode::Unauthorized => "not authorised",
        Mode::NoPermissions => "no permission",
        Mode::Offline => "offline",
    }
}

impl Default for JobView {
    fn default() -> Self {
        Self {
            state: "idle".to_string(),
            progress: 0,
            status_line: "Ready".to_string(),
            lines: Vec::new(),
            result_title: String::new(),
            result_body: String::new(),
            recovery: Vec::new(),
            cancel_mode: "immediate".to_string(),
        }
    }
}

/// Recovery choices after a failed write. Each one is itself a plan.
pub fn recovery_options(target: Slot, source: Slot) -> Vec<RecoveryOption> {
    vec![
        RecoveryOption {
            id: "retry".to_string(),
            title: "Retry this step".to_string(),
            detail: "Only for a step that can be repeated safely.".to_string(),
        },
        RecoveryOption {
            id: "stock".to_string(),
            title: format!("Flash the stock image to slot {}", target.as_str().to_uppercase()),
            detail: "Leaves the phone on the new build without root.".to_string(),
        },
        RecoveryOption {
            id: "switch_back".to_string(),
            title: format!("Switch back to slot {}", source.as_str().to_uppercase()),
            detail: "The original slot was not written. Anti-rollback can still block an older slot after a newer bootloader.".to_string(),
        },
        RecoveryOption {
            id: "restore".to_string(),
            title: "Restore from a backup".to_string(),
            detail: "Builds a restore plan for one backup item. It still needs a review and a confirm.".to_string(),
        },
        RecoveryOption {
            id: "leave".to_string(),
            title: "Leave the phone in the bootloader and get help".to_string(),
            detail: "Writes a plain-text report on this computer. Nothing is uploaded.".to_string(),
        },
    ]
}
