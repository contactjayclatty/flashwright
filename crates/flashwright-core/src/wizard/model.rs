// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use serde::Serialize;

use super::device::{DeviceInfo, DeviceSummary, Slot};
use super::session::Phase;
use super::steps::{GateView, Route, Step};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    PreparePatch,
    UpdateKeepRoot,
    Recovery,
    RestoreStock,
}

impl PlanKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreparePatch => "prepare_patch",
            Self::UpdateKeepRoot => "update_keep_root",
            Self::Recovery => "recovery",
            Self::RestoreStock => "restore_stock",
        }
    }
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
pub struct FirmwareRef {
    pub id: String,
    pub display_name: String,
    pub size: u64,
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

/// Window-facing plan. `dry_run` and `after_dry_run` are inside the hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WizardPlan {
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
    pub dry_run: bool,
    pub after_dry_run: Option<String>,
    pub kind: PlanKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub phase: Phase,
    pub review_kind: Option<PlanKind>,
    pub tools: ToolsStatus,
    pub driver: DriverStatus,
    pub devices: Vec<DeviceSummary>,
    pub selected: Option<DeviceInfo>,
    pub choice: ChoiceView,
    pub firmware: Option<FirmwareReport>,
    pub plan: Option<WizardPlan>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WizardEvent {
    Log {
        v: u32,
        ts: String,
        level: String,
        source: String,
        line: String,
    },
}
