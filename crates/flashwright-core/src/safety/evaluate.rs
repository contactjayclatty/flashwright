// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! The single gate path.
//!
//! Plan build, dry run, and confirm all call [`evaluate`]. Table gates are
//! decided here. A failing block gate is `WOULD BLOCK` on a dry run.

use crate::cmd::{write_argv, WriteCmd};
use crate::device::{Partition, Slot};
use crate::wizard::PlanStep;

use super::backup::BackupState;
use super::tables::{self, SafetyTables};

/// Magisk versionCode required once the firmware security patch is on or after this day.
const MAGISK_FLOOR: u32 = 30600;
const MAGISK_FLOOR_SPL: &str = "2025-12-01";
const OFFICIAL_MAGISK: &str = "com.topjohnwu.magisk";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Block,
    Ack,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateDecision {
    pub id: &'static str,
    pub severity: Severity,
    pub blocked: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateBlock {
    pub id: &'static str,
    pub reason: String,
}

/// Facts the table gates read. Filled from typed device reads, not from the caller.
#[derive(Clone, Debug)]
pub(crate) struct SafetyFacts {
    pub(crate) device_codename: String,
    pub(crate) firmware_codename: String,
    pub(crate) firmware_filename: String,
    pub(crate) active_slot: Option<Slot>,
    pub(crate) target_partition: Partition,
    pub(crate) bootloader_a: Option<String>,
    pub(crate) bootloader_b: Option<String>,
    pub(crate) device_bootloader: Option<String>,
    pub(crate) firmware_bootloader: Option<String>,
    pub(crate) api_level: Option<u32>,
    pub(crate) device_spl: Option<String>,
    pub(crate) firmware_spl: Option<String>,
    pub(crate) image_spl: Option<String>,
    pub(crate) image_fingerprint: Option<String>,
    pub(crate) firmware_fingerprint: Option<String>,
    pub(crate) device_build: Option<String>,
    pub(crate) device_timestamp: Option<u64>,
    pub(crate) firmware_timestamp: Option<u64>,
    pub(crate) magisk_label: Option<String>,
    pub(crate) magisk_code: Option<u32>,
    pub(crate) magisk_package: Option<String>,
    pub(crate) kernel: Option<String>,
    /// Codename read from the phone and stored on the plan. G04 compares firmware to this.
    pub(crate) plan_device: String,
    pub(crate) authorised_devices: u32,
    pub(crate) unlocked: Option<bool>,
    pub(crate) tools_verified: bool,
    pub(crate) tools_match: bool,
    pub(crate) adb_server_ok: bool,
    pub(crate) partition_bytes: Option<u64>,
    pub(crate) pending_ota: Option<bool>,
    pub(crate) firmware_sha256: Option<String>,
    pub(crate) image_sha256: Option<String>,
    pub(crate) full_ota: bool,
    pub(crate) patched_sha1: Option<String>,
    pub(crate) stock_sha1: Option<String>,
    pub(crate) battery_percent: Option<u32>,
    pub(crate) host_free_bytes: Option<u64>,
    pub(crate) device_free_bytes: Option<u64>,
    pub(crate) driver_ok: bool,
    pub(crate) evidence: Vec<FactEvidence>,
    pub(crate) checks: LegacyChecks,
}

/// One phone read that a fact came from. The digest is part of the plan hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FactEvidence {
    pub(crate) source: String,
    pub(crate) sha256: String,
}

/// Pass or fail for the gates that are not computed from a phone read.
#[derive(Clone, Debug)]
pub(crate) struct LegacyChecks {
    failing: Vec<&'static str>,
}

impl LegacyChecks {
    fn none() -> Self {
        Self {
            failing: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn pass() -> Self {
        Self::none()
    }

    #[cfg(test)]
    pub(crate) fn fail(id: &'static str) -> Self {
        Self { failing: vec![id] }
    }

    fn allows(&self, id: &str) -> bool {
        !self.failing.contains(&id)
    }
}

impl SafetyFacts {
    pub(crate) fn blank() -> Self {
        Self {
            device_codename: String::new(),
            firmware_codename: String::new(),
            firmware_filename: String::new(),
            active_slot: None,
            target_partition: Partition::InitBoot,
            bootloader_a: None,
            bootloader_b: None,
            device_bootloader: None,
            firmware_bootloader: None,
            api_level: None,
            device_spl: None,
            firmware_spl: None,
            image_spl: None,
            image_fingerprint: None,
            firmware_fingerprint: None,
            device_build: None,
            device_timestamp: None,
            firmware_timestamp: None,
            magisk_label: None,
            magisk_code: None,
            magisk_package: None,
            kernel: None,
            plan_device: String::new(),
            authorised_devices: 0,
            unlocked: None,
            tools_verified: false,
            tools_match: false,
            adb_server_ok: false,
            partition_bytes: None,
            pending_ota: None,
            firmware_sha256: None,
            image_sha256: None,
            full_ota: false,
            patched_sha1: None,
            stock_sha1: None,
            battery_percent: None,
            host_free_bytes: None,
            device_free_bytes: None,
            driver_ok: false,
            evidence: Vec::new(),
            checks: LegacyChecks::none(),
        }
    }

    /// Synthetic Pixel 9 Pro XL facts that pass every table gate.
    #[cfg(test)]
    pub(crate) fn komodo_ready() -> Self {
        Self {
            device_codename: "komodo".into(),
            firmware_codename: "komodo".into(),
            firmware_filename: "komodo-factory-synthetic.zip".into(),
            active_slot: Some(Slot::A),
            target_partition: Partition::InitBoot,
            bootloader_a: Some("16.2-100".into()),
            bootloader_b: Some("16.2-100".into()),
            device_bootloader: Some("16.2-100".into()),
            firmware_bootloader: Some("16.2-100".into()),
            api_level: Some(34),
            device_spl: Some("2026-02-01".into()),
            firmware_spl: Some("2026-02-01".into()),
            image_spl: Some("2026-02-01".into()),
            image_fingerprint: Some("synthetic/komodo/test".into()),
            firmware_fingerprint: Some("synthetic/komodo/test".into()),
            device_build: Some("TEST.260201.001".into()),
            device_timestamp: Some(1_700_000_000),
            firmware_timestamp: Some(1_700_000_000),
            magisk_label: Some("30.7".into()),
            magisk_code: Some(30700),
            magisk_package: Some(OFFICIAL_MAGISK.into()),
            kernel: Some("6.1.0-android14-synthetic".into()),
            plan_device: "komodo".into(),
            authorised_devices: 1,
            unlocked: Some(true),
            tools_verified: true,
            tools_match: true,
            adb_server_ok: true,
            partition_bytes: Some(64 * 1024 * 1024),
            pending_ota: Some(false),
            firmware_sha256: Some("synthetic-sha".into()),
            image_sha256: Some("synthetic-sha".into()),
            full_ota: true,
            patched_sha1: Some("synthetic-sha1".into()),
            stock_sha1: Some("synthetic-sha1".into()),
            battery_percent: Some(80),
            host_free_bytes: Some(8 * 1024 * 1024 * 1024),
            device_free_bytes: Some(8 * 1024 * 1024 * 1024),
            driver_ok: true,
            evidence: vec![FactEvidence {
                source: "synthetic".into(),
                sha256: "synthetic-sha".into(),
            }],
            checks: LegacyChecks::pass(),
        }
    }
}

/// Every gate the dry run evaluates, in order.
pub fn gate_ids() -> &'static [&'static str] {
    &[
        "G01", "G02", "G03", "G04", "G05", "G06", "G07", "G08", "G09", "G10", "G11", "G12", "G13",
        "G14", "G15", "G16", "G17", "G18", "G19", "G20", "G21", "G22", "G23", "G24", "G25", "G26",
        "G27", "G28",
    ]
}

pub(crate) fn evaluate(
    steps: &[PlanStep],
    facts: Option<&SafetyFacts>,
    backup: Option<&BackupState>,
) -> Vec<GateDecision> {
    evaluate_acked(steps, facts, backup, &[])
}

pub(crate) fn evaluate_acked(
    steps: &[PlanStep],
    facts: Option<&SafetyFacts>,
    backup: Option<&BackupState>,
    acks: &[String],
) -> Vec<GateDecision> {
    tables::log_ported_items();
    let tables = tables::tables();
    let image_write = needs_backup(steps);
    let rendered = rendered_args(steps);
    gate_ids()
        .iter()
        .map(|id| {
            let mut decision = decide(id, steps, facts, backup, tables, image_write, &rendered);
            if decision.blocked
                && decision.severity == Severity::Ack
                && acks.iter().any(|ack| ack == decision.id)
            {
                decision.blocked = false;
            }
            decision
        })
        .collect()
}

/// Gates for one write, including reboot, set-active, and a cleanup step.
///
/// An empty result means this step is not a write. A blocked entry refuses the write.
pub(crate) fn evaluate_step(step: &PlanStep, facts: Option<&SafetyFacts>) -> Vec<GateDecision> {
    let PlanStep::Write(cmd) = step else {
        return Vec::new();
    };
    let mut ids = vec!["G01", "G02", "G21", "G22"];
    if cmd_is_fastboot(cmd) {
        ids.push("G03");
    }
    if is_image_write(cmd) {
        ids.push("G15");
        ids.push("G28");
    }
    if is_vbmeta_flash(cmd) {
        ids.push("G26");
    }
    let steps = std::slice::from_ref(step);
    ids.into_iter()
        .filter_map(|id| {
            let (blocked, reason) = match id {
                "G01" => g01(facts),
                "G02" => g02(facts),
                "G03" => g03(steps, facts),
                "G15" => g15(steps, facts),
                "G21" => g21(facts),
                "G22" => g22(facts),
                "G26" => g26(steps),
                "G28" => g28(steps),
                _ => (false, String::new()),
            };
            if !blocked {
                return None;
            }
            Some(GateDecision {
                id: static_id(id),
                severity: Severity::Block,
                blocked: true,
                reason,
            })
        })
        .collect()
}

pub fn blocking(decisions: &[GateDecision]) -> Vec<GateBlock> {
    decisions
        .iter()
        .filter(|gate| gate.blocked && gate.severity == Severity::Block)
        .map(|gate| GateBlock {
            id: gate.id,
            reason: gate.reason.clone(),
        })
        .collect()
}

pub fn dry_run_lines(steps: &[PlanStep], decisions: &[GateDecision]) -> Vec<String> {
    let mut lines = Vec::new();
    for gate in decisions.iter().filter(|gate| gate.blocked) {
        lines.push(format!("WOULD BLOCK: {} {}", gate.id, gate.reason));
    }
    if lines.is_empty() {
        for step in steps {
            let rendered = match step {
                PlanStep::Write(cmd) => write_argv(cmd).ok(),
                PlanStep::Cleanup(cmd) => crate::cmd::cleanup_argv(cmd).ok(),
                PlanStep::Read(_) => None,
            };
            if let Some(rendered) = rendered {
                lines.push(format!("WOULD RUN: {}", rendered.args.join(" ")));
            }
        }
    }
    lines
}

pub fn needs_backup(steps: &[PlanStep]) -> bool {
    steps.iter().any(|step| match step {
        PlanStep::Write(cmd) => is_image_write(cmd),
        PlanStep::Read(_) | PlanStep::Cleanup(_) => false,
    })
}

fn is_image_write(cmd: &WriteCmd) -> bool {
    match cmd {
        WriteCmd::Fastboot(
            crate::cmd::FastbootWrite::Flash { .. } | crate::cmd::FastbootWrite::Update { .. },
        )
        | WriteCmd::AdbHost(
            crate::cmd::AdbHostWrite::Push { .. } | crate::cmd::AdbHostWrite::Sideload { .. },
        )
        | WriteCmd::AdbShell(_)
        | WriteCmd::Su(_) => true,
        WriteCmd::Fastboot(
            crate::cmd::FastbootWrite::SetActive { .. } | crate::cmd::FastbootWrite::Reboot { .. },
        )
        | WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Reboot { .. }) => false,
    }
}

fn decide(
    id: &str,
    steps: &[PlanStep],
    facts: Option<&SafetyFacts>,
    backup: Option<&BackupState>,
    tables: &SafetyTables,
    image_write: bool,
    rendered: &[String],
) -> GateDecision {
    let severity = severity_of(id);
    if facts.is_some_and(|facts| !facts.checks.allows(id)) {
        return GateDecision {
            id: static_id(id),
            severity,
            blocked: true,
            reason: legacy_message(id).to_string(),
        };
    }
    let (blocked, reason) = match id {
        "G01" => g01(facts),
        "G02" => g02(facts),
        "G03" => g03(steps, facts),
        "G04" => g04(facts, tables, image_write),
        "G05" => g05(facts, image_write),
        "G06" => g06(steps, facts),
        "G07" => g07(facts, image_write),
        "G08" => g08(facts, image_write),
        "G09" => g09(steps, facts),
        "G10" => g10(facts, tables, image_write),
        "G11" => g11(facts, image_write),
        "G12" => g12(facts, image_write),
        "G13" => g13(facts, image_write),
        "G14" => g14(backup, steps, image_write),
        "G15" => g15(steps, facts),
        "G16" => g16(rendered, tables),
        "G17" => g17(facts, image_write),
        "G18" => g18(facts, steps),
        "G19" => g19(facts, tables),
        "G20" => g20(facts, image_write),
        "G21" => g21(facts),
        "G22" => g22(facts),
        "G23" => g23(facts, steps, tables),
        "G24" => g24(steps, facts, tables, rendered),
        "G25" => g25(rendered, tables),
        "G26" => g26(steps),
        "G27" => g27(rendered, tables),
        "G28" => g28(steps),
        other => legacy(other, facts, image_write),
    };
    GateDecision {
        id: static_id(id),
        severity,
        blocked,
        reason,
    }
}

fn static_id(id: &str) -> &'static str {
    gate_ids()
        .iter()
        .copied()
        .find(|known| *known == id)
        .unwrap_or("G27")
}

fn severity_of(id: &str) -> Severity {
    match id {
        "G17" | "G19" => Severity::Ack,
        _ => Severity::Block,
    }
}

fn legacy(id: &str, facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    let message = legacy_message(id);
    match facts {
        Some(facts) if facts.checks.allows(id) => (false, String::new()),
        Some(_) => (true, message.to_string()),
        None if image_write => (true, format!("{message} This check was not run.")),
        None => (false, String::new()),
    }
}

fn legacy_message(id: &str) -> &'static str {
    match id {
        "G01" => "Platform-tools are not write-enabled.",
        "G02" => "Select one authorised device.",
        "G03" => "The bootloader is locked.",
        "G05" => "The firmware SHA-256 does not match.",
        "G06" => "The package is not a full A/B update.",
        "G09" => "The patched image does not match the stock image.",
        "G11" => "The battery is too low.",
        "G12" => "The computer is short on free space.",
        "G13" => "The phone is short on free space.",
        "G15" => "The target partition is missing or too small.",
        "G17" => "The USB driver was not confirmed.",
        "G20" => "A pending system update may undo root after it installs.",
        "G21" => "Flashwright's copy of platform-tools changed on disk. Re-import it.",
        "G22" => "Restart adb before a write. The server on port 5037 is not the verified adb.",
        "G23" => "This Tensor phone cannot take that bootloader.",
        "G24" => "The plan writes a slot other than the inactive slot.",
        "G25" => "That region is off-limits and cannot be read or written.",
        "G26" => "vbmeta is read-only.",
        "G27" => "That write is off-limits.",
        "G28" => "The image is larger than the catalogue allows.",
        _ => "This check failed.",
    }
}

fn g01(facts: Option<&SafetyFacts>) -> (bool, String) {
    match facts {
        Some(facts) if facts.tools_verified => (false, String::new()),
        _ => (true, legacy_message("G01").to_string()),
    }
}

fn g05(facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    if !image_write {
        return (false, String::new());
    }
    let Some(facts) = facts else {
        return missing(true, "The firmware SHA-256 was not checked.");
    };
    match (&facts.firmware_sha256, &facts.image_sha256) {
        (Some(firmware), Some(image)) if firmware == image => (false, String::new()),
        (Some(_), Some(_)) => (true, legacy_message("G05").to_string()),
        _ => (
            true,
            "The firmware SHA-256 was not read from the file.".into(),
        ),
    }
}

fn g06(steps: &[PlanStep], facts: Option<&SafetyFacts>) -> (bool, String) {
    let ota = steps.iter().any(|step| {
        matches!(
            step,
            PlanStep::Write(WriteCmd::Fastboot(crate::cmd::FastbootWrite::Update { .. }))
                | PlanStep::Write(WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload { .. }))
        )
    });
    if !ota {
        return (false, String::new());
    }
    if facts.is_some_and(|facts| facts.full_ota) {
        (false, String::new())
    } else {
        (true, legacy_message("G06").to_string())
    }
}

fn g09(steps: &[PlanStep], facts: Option<&SafetyFacts>) -> (bool, String) {
    let patch = steps.iter().any(|step| {
        matches!(
            step,
            PlanStep::Write(WriteCmd::Su(_)) | PlanStep::Write(WriteCmd::AdbShell(_))
        )
    });
    if !patch {
        return (false, String::new());
    }
    match facts {
        Some(facts) if facts.patched_sha1.is_some() && facts.patched_sha1 == facts.stock_sha1 => {
            (false, String::new())
        }
        _ => (true, legacy_message("G09").to_string()),
    }
}

fn g11(facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    if !image_write {
        return (false, String::new());
    }
    match facts.and_then(|facts| facts.battery_percent) {
        Some(level) if level >= 20 => (false, String::new()),
        Some(_) => (true, legacy_message("G11").to_string()),
        None => (true, "The battery level was not read.".into()),
    }
}

fn g12(facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    if !image_write {
        return (false, String::new());
    }
    match facts.and_then(|facts| facts.host_free_bytes) {
        Some(free) if free >= 512 * 1024 * 1024 => (false, String::new()),
        Some(_) => (true, legacy_message("G12").to_string()),
        None => (true, "Free space on the computer was not read.".into()),
    }
}

fn g13(facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    if !image_write {
        return (false, String::new());
    }
    match facts.and_then(|facts| facts.device_free_bytes) {
        Some(free) if free >= 512 * 1024 * 1024 => (false, String::new()),
        Some(_) => (true, legacy_message("G13").to_string()),
        None => (true, "Free space on the phone was not read.".into()),
    }
}

fn g17(facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    if !image_write {
        return (false, String::new());
    }
    if facts.is_some_and(|facts| facts.driver_ok) {
        (false, String::new())
    } else {
        (true, legacy_message("G17").to_string())
    }
}

fn g02(facts: Option<&SafetyFacts>) -> (bool, String) {
    match facts {
        Some(facts) if facts.authorised_devices == 1 => (false, String::new()),
        Some(_) => (true, legacy_message("G02").to_string()),
        None => (
            true,
            format!("{} This check was not run.", legacy_message("G02")),
        ),
    }
}

fn g03(steps: &[PlanStep], facts: Option<&SafetyFacts>) -> (bool, String) {
    if !steps
        .iter()
        .any(|step| matches!(step, PlanStep::Write(cmd) if cmd_is_fastboot(cmd)))
    {
        return (false, String::new());
    }
    match facts.and_then(|facts| facts.unlocked) {
        Some(true) => (false, String::new()),
        Some(false) => (true, legacy_message("G03").to_string()),
        None => (
            true,
            format!("{} This check was not run.", legacy_message("G03")),
        ),
    }
}

fn g15(steps: &[PlanStep], facts: Option<&SafetyFacts>) -> (bool, String) {
    let Some(needed) = image_bytes(steps) else {
        return (false, String::new());
    };
    match facts.and_then(|facts| facts.partition_bytes) {
        Some(have) if have >= needed => (false, String::new()),
        Some(_) => (true, legacy_message("G15").to_string()),
        None => (
            true,
            format!("{} This check was not run.", legacy_message("G15")),
        ),
    }
}

fn g20(facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    if !image_write {
        return (false, String::new());
    }
    match facts.and_then(|facts| facts.pending_ota) {
        Some(false) => (false, String::new()),
        Some(true) => (true, legacy_message("G20").to_string()),
        None => (true, "A pending system update was not checked.".to_string()),
    }
}

fn g21(facts: Option<&SafetyFacts>) -> (bool, String) {
    match facts {
        Some(facts) if facts.tools_verified && facts.tools_match => (false, String::new()),
        Some(_) => (true, legacy_message("G21").to_string()),
        None => (
            true,
            format!("{} This check was not run.", legacy_message("G21")),
        ),
    }
}

fn g22(facts: Option<&SafetyFacts>) -> (bool, String) {
    match facts {
        Some(facts) if facts.adb_server_ok => (false, String::new()),
        Some(_) => (true, legacy_message("G22").to_string()),
        None => (
            true,
            format!("{} This check was not run.", legacy_message("G22")),
        ),
    }
}

fn cmd_is_fastboot(cmd: &WriteCmd) -> bool {
    matches!(cmd, WriteCmd::Fastboot(_))
}

fn image_bytes(steps: &[PlanStep]) -> Option<u64> {
    let mut needed = None;
    for step in steps {
        let PlanStep::Write(cmd) = step else {
            continue;
        };
        let size = match cmd {
            WriteCmd::Fastboot(
                crate::cmd::FastbootWrite::Flash { image, .. }
                | crate::cmd::FastbootWrite::Update { package: image, .. },
            )
            | WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload { package: image, .. }) => {
                Some(image.size_bytes())
            }
            WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Push { src, .. }) => match src {
                crate::cmd::HostRef::Image(image) => Some(image.size_bytes()),
                crate::cmd::HostRef::Asset(_) => None,
            },
            _ => None,
        };
        if let Some(size) = size {
            needed = Some(needed.unwrap_or(0).max(size));
        }
    }
    needed
}

fn g04(facts: Option<&SafetyFacts>, tables: &SafetyTables, image_write: bool) -> (bool, String) {
    let Some(facts) = facts else {
        return missing(image_write, "The phone and the firmware were not compared.");
    };
    if !image_write {
        return (false, String::new());
    }
    let phone = if facts.plan_device.is_empty() {
        facts.device_codename.as_str()
    } else {
        facts.plan_device.as_str()
    };
    let Some(row) = find_device(tables, phone) else {
        return (true, format!("{phone} is not a known device."));
    };
    if !row.ab {
        return (true, "This phone is not an A/B device.".into());
    }
    if tables.aliases.canonical(phone) != tables.aliases.canonical(&facts.firmware_codename) {
        return (
            true,
            format!(
                "The firmware is for {}, and this phone is {phone}.",
                facts.firmware_codename
            ),
        );
    }
    let prefix = filename_prefix(&facts.firmware_filename);
    if tables.aliases.canonical(prefix) != tables.aliases.canonical(phone) {
        return (
            true,
            "The firmware file name does not match this phone.".into(),
        );
    }
    let wants_init = facts.target_partition == Partition::InitBoot;
    if row.has_init_boot != wants_init {
        let target = facts.target_partition.fastboot_name();
        return (
            true,
            format!("This phone does not use {target} as its boot image."),
        );
    }
    (false, String::new())
}

fn g07(facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    let Some(facts) = facts else {
        return missing(image_write, "The firmware age was not checked.");
    };
    if !image_write {
        return (false, String::new());
    }
    if let (Some(firmware), Some(device)) = (&facts.firmware_spl, &facts.device_spl) {
        if firmware.as_str() < device.as_str() {
            return (
                true,
                "The firmware security patch is older than the phone.".into(),
            );
        }
    } else {
        return (true, "The security patch dates are missing.".into());
    }
    let (Some(firmware), Some(device)) = (facts.firmware_timestamp, facts.device_timestamp) else {
        return (true, "The firmware build timestamp is missing.".into());
    };
    if firmware < device {
        return (true, "The firmware build is older than the phone.".into());
    }
    (false, String::new())
}

fn g08(facts: Option<&SafetyFacts>, image_write: bool) -> (bool, String) {
    let Some(facts) = facts else {
        return missing(image_write, "The image security patch was not checked.");
    };
    if !image_write {
        return (false, String::new());
    }
    let (Some(image), Some(firmware)) = (&facts.image_spl, &facts.firmware_spl) else {
        return (true, "The image security patch is missing.".into());
    };
    if image != firmware {
        return (
            true,
            "The image security patch does not match the firmware.".into(),
        );
    }
    let (Some(image_id), Some(firmware_id)) =
        (&facts.image_fingerprint, &facts.firmware_fingerprint)
    else {
        return (true, "The image fingerprint is missing.".into());
    };
    if image_id != firmware_id {
        return (
            true,
            "The image fingerprint does not match the firmware.".into(),
        );
    }
    if !plan_updates_system_from_facts(facts) {
        let device_build = facts
            .device_build
            .as_deref()
            .and_then(spl_from_build)
            .or(facts.device_spl.clone());
        if device_build.as_deref() != Some(image.as_str()) {
            return (
                true,
                "The image security patch does not match the phone.".into(),
            );
        }
    }
    (false, String::new())
}

fn plan_updates_system_from_facts(facts: &SafetyFacts) -> bool {
    facts.firmware_spl.as_deref() != facts.device_spl.as_deref()
        && facts
            .firmware_spl
            .as_deref()
            .zip(facts.device_spl.as_deref())
            .is_some_and(|(firmware, device)| firmware > device)
}

fn g10(facts: Option<&SafetyFacts>, tables: &SafetyTables, image_write: bool) -> (bool, String) {
    let Some(facts) = facts else {
        return missing(image_write, "Magisk was not checked.");
    };
    if !image_write {
        return (false, String::new());
    }
    let Some(code) = facts.magisk_code else {
        return (true, "Magisk was not read from the phone.".into());
    };
    if let Some(label) = &facts.magisk_label {
        if tables
            .combos
            .iter()
            .any(|combo| combo.label == *label && combo.version_code == code)
        {
            return (true, "This Magisk build is on the known-bad list.".into());
        }
    }
    let Some(package) = &facts.magisk_package else {
        return (
            true,
            "The Magisk package was not read from the phone.".into(),
        );
    };
    if package != OFFICIAL_MAGISK || tables.off_limits.packages.iter().any(|id| id == package) {
        return (true, "This Magisk app is not an official build.".into());
    }
    let Some(kernel) = &facts.kernel else {
        return (true, "The kernel was not read from the phone.".into());
    };
    if let Some(fragment) = tables
        .kernels
        .iter()
        .find(|fragment| kernel.contains(fragment.as_str()))
    {
        return (
            true,
            format!("The kernel name contains the known-bad fragment {fragment}."),
        );
    }
    let spl = facts
        .firmware_spl
        .as_deref()
        .or(facts.device_spl.as_deref());
    if spl.is_some_and(|day| day >= MAGISK_FLOOR_SPL) && code < MAGISK_FLOOR {
        return (
            true,
            "This Magisk version is too old for the firmware security patch.".into(),
        );
    }
    (false, String::new())
}

fn g14(backup: Option<&BackupState>, steps: &[PlanStep], image_write: bool) -> (bool, String) {
    if !image_write {
        return (false, String::new());
    }
    let Some((serial, slot, partition)) = plan_target(steps) else {
        return (
            true,
            "Stock init_boot has not been backed up and verified.".into(),
        );
    };
    match backup {
        Some(BackupState::Verified(set)) if set.matches_plan(&serial, slot, partition) => {
            (false, String::new())
        }
        Some(BackupState::Verified(_)) => (
            true,
            "The backup is for a different phone, slot, or partition.".into(),
        ),
        Some(BackupState::Blocked(block)) => (true, block.reason.clone()),
        _ => (
            true,
            "Stock init_boot has not been backed up and verified.".into(),
        ),
    }
}

fn plan_target(steps: &[PlanStep]) -> Option<(String, Slot, Partition)> {
    for step in steps {
        if let PlanStep::Write(WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash {
            serial,
            slot,
            partition,
            ..
        })) = step
        {
            if *partition != Partition::Vbmeta {
                return Some((serial.as_str().to_string(), *slot, *partition));
            }
        }
    }
    None
}

fn g16(rendered: &[String], tables: &SafetyTables) -> (bool, String) {
    for token in rendered {
        if tables.off_limits.argv.iter().any(|banned| banned == token) {
            return (
                true,
                "The plan would wipe data or disable verification.".into(),
            );
        }
    }
    (false, String::new())
}

fn g18(facts: Option<&SafetyFacts>, steps: &[PlanStep]) -> (bool, String) {
    let flashes = flashes_bootloader(steps);
    let Some(facts) = facts else {
        return missing(flashes, "The bootloader versions were not compared.");
    };
    match (&facts.firmware_bootloader, &facts.device_bootloader) {
        (Some(firmware), Some(device)) => {
            if bootloader_older(firmware, device) {
                return (
                    true,
                    "The firmware bootloader is older than the phone.".into(),
                );
            }
            (false, String::new())
        }
        _ if flashes => (true, "The bootloader versions are missing.".into()),
        _ => (false, String::new()),
    }
}

fn g19(facts: Option<&SafetyFacts>, tables: &SafetyTables) -> (bool, String) {
    let Some(facts) = facts else {
        return (false, String::new());
    };
    let Some(row) = tables
        .bootloaders
        .iter()
        .find(|row| row.codename == tables.aliases.canonical(&facts.device_codename))
    else {
        return (false, String::new());
    };
    for (slot, version) in [("a", &facts.bootloader_a), ("b", &facts.bootloader_b)] {
        let Some(version) = version else {
            return (true, format!("Slot {slot} bootloader version is missing."));
        };
        if parse_bootloader(version).is_none() || bootloader_older(version, &row.min) {
            return (
                true,
                format!(
                    "Slot {slot} bootloader {version} is older than the minimum safe version {}.",
                    row.min
                ),
            );
        }
    }
    (false, String::new())
}

fn arb(facts: Option<&SafetyFacts>, steps: &[PlanStep], tables: &SafetyTables) -> (bool, String) {
    if !flashes_bootloader(steps) {
        return (false, String::new());
    }
    let Some(facts) = facts else {
        return (
            true,
            "A bootloader write needs an anti-rollback check.".into(),
        );
    };
    let codename = tables.aliases.canonical(&facts.device_codename);
    let Some(row) = tables.tensor.iter().find(|row| row.codename == codename) else {
        return (false, String::new());
    };
    match facts.api_level {
        Some(api) if api >= row.min_api => (false, String::new()),
        _ => (
            true,
            format!(
                "{codename} is below API {} and a bootloader write would bump anti-rollback on one slot.",
                row.min_api
            ),
        ),
    }
}

fn slot_gate(
    steps: &[PlanStep],
    facts: Option<&SafetyFacts>,
    tables: &SafetyTables,
    rendered: &[String],
) -> (bool, String) {
    if tables.slots.both_slots != "block" || tables.slots.slot_all != "block" {
        return (true, "Slot rules are not set to block.".into());
    }
    if rendered
        .windows(2)
        .any(|pair| pair[0] == "--slot" && pair[1] == "all")
    {
        return (true, "The plan uses --slot all.".into());
    }
    let mut written = Vec::new();
    for step in steps {
        let PlanStep::Write(cmd) = step else {
            continue;
        };
        if let Some(slot) = write_slot(cmd) {
            if !written.contains(&slot) {
                written.push(slot);
            }
        }
    }
    if written.len() > 1 {
        return (
            true,
            "The plan writes both slots. That removes the fallback.".into(),
        );
    }
    for step in steps {
        let PlanStep::Write(cmd) = step else {
            continue;
        };
        if let Some(reason) = slot_violation(cmd, facts, tables) {
            return (true, reason);
        }
    }
    (false, String::new())
}

fn slot_violation(
    cmd: &WriteCmd,
    facts: Option<&SafetyFacts>,
    tables: &SafetyTables,
) -> Option<String> {
    if matches!(
        cmd,
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash {
            partition: Partition::Vbmeta,
            ..
        })
    ) {
        return None;
    }
    let rendered = write_argv(cmd).ok();
    let args = rendered
        .as_ref()
        .map(|item| item.args.as_slice())
        .unwrap_or(&[]);
    let slot = write_slot(cmd)?;
    if tables.slots.explicit_slot && !explicit_slot(args) {
        return Some("A write is missing an explicit slot.".into());
    }
    if tables.slots.target != "inactive" {
        return Some("Slot rules do not require the inactive slot.".into());
    }
    let Some(active) = facts.and_then(|facts| facts.active_slot) else {
        return Some("The active slot is unknown.".into());
    };
    if slot != active.other() {
        return Some("The plan writes a slot other than the inactive slot.".into());
    }
    None
}

fn explicit_slot(args: &[String]) -> bool {
    let named = args
        .windows(2)
        .any(|pair| pair[0] == "--slot" && (pair[1] == "a" || pair[1] == "b"));
    let set_active = args
        .iter()
        .any(|arg| matches!(arg.strip_prefix("--set-active="), Some("a" | "b")));
    named || set_active
}

fn write_slot(cmd: &WriteCmd) -> Option<Slot> {
    match cmd {
        WriteCmd::Fastboot(
            crate::cmd::FastbootWrite::Flash { slot, .. }
            | crate::cmd::FastbootWrite::Update { slot, .. }
            | crate::cmd::FastbootWrite::SetActive { slot, .. },
        ) => Some(*slot),
        _ => None,
    }
}

fn g23(facts: Option<&SafetyFacts>, steps: &[PlanStep], tables: &SafetyTables) -> (bool, String) {
    arb(facts, steps, tables)
}

fn g24(
    steps: &[PlanStep],
    facts: Option<&SafetyFacts>,
    tables: &SafetyTables,
    rendered: &[String],
) -> (bool, String) {
    slot_gate(steps, facts, tables, rendered)
}

fn g25(rendered: &[String], tables: &SafetyTables) -> (bool, String) {
    for token in rendered {
        if let Some(region) = region_hit(token, tables) {
            return (
                true,
                format!("The {region} region is off-limits and cannot be read or written."),
            );
        }
    }
    (false, String::new())
}

fn g26(steps: &[PlanStep]) -> (bool, String) {
    if steps.iter().any(|step| {
        matches!(
            step,
            PlanStep::Write(cmd) if is_vbmeta_flash(cmd)
        )
    }) {
        (true, legacy_message("G26").to_string())
    } else {
        (false, String::new())
    }
}

fn g27(rendered: &[String], tables: &SafetyTables) -> (bool, String) {
    for token in rendered {
        if token == "erase" {
            return (true, "Erasing a partition is off-limits.".into());
        }
        let lower = token.to_ascii_lowercase();
        if tables
            .off_limits
            .shell
            .iter()
            .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
        {
            return (true, "A host shell is off-limits.".into());
        }
    }
    (false, String::new())
}

fn g28(steps: &[PlanStep]) -> (bool, String) {
    match image_bytes(steps) {
        Some(size) if size > crate::cmd::MAX_BLOCK_LEN => (true, legacy_message("G28").to_string()),
        _ => (false, String::new()),
    }
}

fn is_vbmeta_flash(cmd: &WriteCmd) -> bool {
    matches!(
        cmd,
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash {
            partition: Partition::Vbmeta,
            ..
        })
    )
}

fn region_hit<'a>(token: &str, tables: &'a SafetyTables) -> Option<&'a str> {
    let mut current = String::new();
    let mut hit = None;
    for ch in token.chars().chain(std::iter::once('\0')) {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
            continue;
        }
        if tables
            .off_limits
            .regions
            .iter()
            .any(|region| region.eq_ignore_ascii_case(&current))
        {
            hit = tables
                .off_limits
                .regions
                .iter()
                .find(|region| region.eq_ignore_ascii_case(&current))
                .map(String::as_str);
        }
        current.clear();
    }
    hit
}

fn missing(needed: bool, reason: &str) -> (bool, String) {
    if needed {
        (true, reason.to_string())
    } else {
        (false, String::new())
    }
}

fn find_device<'a>(tables: &'a SafetyTables, codename: &str) -> Option<&'a tables::DeviceCompat> {
    let canonical = tables.aliases.canonical(codename);
    tables
        .devices
        .iter()
        .find(|row| row.codename == codename)
        .or_else(|| tables.devices.iter().find(|row| row.codename == canonical))
}

fn filename_prefix(filename: &str) -> &str {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    name.split(['-', '_']).next().unwrap_or(name)
}

fn rendered_args(steps: &[PlanStep]) -> Vec<String> {
    let mut out = Vec::new();
    for step in steps {
        let rendered = match step {
            PlanStep::Read(cmd) => crate::cmd::read_argv(cmd).ok(),
            PlanStep::Write(cmd) => write_argv(cmd).ok(),
            PlanStep::Cleanup(cmd) => crate::cmd::cleanup_argv(cmd).ok(),
        };
        if let Some(rendered) = rendered {
            out.extend(rendered.args);
        }
    }
    out
}

fn flashes_bootloader(steps: &[PlanStep]) -> bool {
    steps.iter().any(|step| {
        matches!(
            step,
            PlanStep::Write(WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash {
                partition: Partition::Bootloader,
                ..
            }))
        )
    })
}

/// `major.minor-patch`, compared the way the upstream helper compares it.
pub fn parse_bootloader(version: &str) -> Option<(u64, u64, u64)> {
    let (major_minor, patch) = version.split_once('-')?;
    let (major, minor) = major_minor.split_once('.')?;
    Some((
        major.parse().ok()?,
        minor.parse().ok()?,
        patch.parse().ok()?,
    ))
}

pub fn bootloader_older(version: &str, min_version: &str) -> bool {
    let Some(left) = parse_bootloader(version) else {
        return true;
    };
    let Some(right) = parse_bootloader(min_version) else {
        return true;
    };
    left < right
}

/// `TEST.260201.001` yields `2026-02-01`.
pub fn spl_from_build(build: &str) -> Option<String> {
    let bytes = build.as_bytes();
    if bytes.len() < 7 {
        return None;
    }
    for index in 0..=bytes.len() - 7 {
        if bytes[index] != b'.' {
            continue;
        }
        let digits = &bytes[index + 1..index + 7];
        if !digits.iter().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        if bytes
            .get(index + 7)
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let yy = &build[index + 1..index + 3];
        let mm = &build[index + 3..index + 5];
        let dd = &build[index + 5..index + 7];
        return Some(format!("20{yy}-{mm}-{dd}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::{AdbHostWrite, DeviceSerial, FastbootWrite, ImageRef, RebootMode, WriteCmd};
    use crate::safety::backup::BackupSet;
    use crate::safety::tables::{ported_items, tables};
    use crate::wizard::PlanStep;

    fn serial() -> DeviceSerial {
        DeviceSerial::try_from("synth-komodo-1").unwrap()
    }

    fn flash(slot: Slot, partition: Partition, path: &str) -> PlanStep {
        PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash {
            serial: serial(),
            slot,
            partition,
            image: ImageRef::new(1, path, 4096),
        }))
    }

    fn stock_flash() -> Vec<PlanStep> {
        vec![flash(
            Slot::B,
            Partition::InitBoot,
            "/var/flashwright/init_boot.img",
        )]
    }

    fn verified() -> BackupState {
        BackupState::Verified(BackupSet::bound(
            "set-komodo",
            "manifest",
            "synth-komodo-1",
            Slot::B,
            Partition::InitBoot,
        ))
    }

    fn preview(
        steps: &[PlanStep],
        facts: Option<&SafetyFacts>,
        backup: Option<&BackupState>,
    ) -> Vec<String> {
        dry_run_lines(steps, &evaluate(steps, facts, backup))
    }

    fn assert_blocked(lines: &[String], id: &str) {
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with(&format!("WOULD BLOCK: {id} "))),
            "{id} missing from {lines:?}"
        );
        assert!(
            lines.iter().all(|line| !line.starts_with("WOULD RUN")),
            "a blocked dry run printed a write: {lines:?}"
        );
    }

    #[test]
    fn every_gate_is_evaluated_once() {
        let facts = SafetyFacts::komodo_ready();
        let backup = verified();
        let steps = stock_flash();
        let decisions = evaluate(&steps, Some(&facts), Some(&backup));
        let ids: Vec<_> = decisions.iter().map(|gate| gate.id).collect();
        assert_eq!(ids, gate_ids());
        assert!(decisions.iter().all(|gate| !gate.blocked));
        let lines = dry_run_lines(&steps, &decisions);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("WOULD RUN: -s synth-komodo-1 --slot b flash init_boot "));
        assert!(blocking(&decisions).is_empty());
    }

    #[test]
    fn every_numbered_gate_can_block() {
        let backup = verified();
        let steps = stock_flash();
        for id in gate_ids() {
            let mut facts = SafetyFacts::komodo_ready();
            facts.checks = LegacyChecks::fail(id);
            assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), id);
        }
        assert_eq!(gate_ids().len(), 28);
    }

    #[test]
    fn image_write_without_facts_blocks_and_prints_nothing_to_run() {
        let lines = preview(&stock_flash(), None, None);
        assert_blocked(&lines, "G01");
        assert_blocked(&lines, "G14");
    }

    #[test]
    fn reboot_without_facts_blocks_the_device_and_the_tools() {
        let steps = vec![PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Reboot {
            serial: serial(),
            mode: RebootMode::System,
        }))];
        let lines = preview(&steps, None, None);
        for id in ["G02", "G21", "G22"] {
            assert_blocked(&lines, id);
        }
    }

    #[test]
    fn observed_reboot_would_run() {
        let steps = vec![PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Reboot {
            serial: serial(),
            mode: RebootMode::System,
        }))];
        let lines = preview(&steps, Some(&SafetyFacts::komodo_ready()), None);
        assert_eq!(
            lines,
            vec!["WOULD RUN: -s synth-komodo-1 reboot".to_string()]
        );
    }

    #[test]
    fn device_and_build_mismatches_block() {
        let backup = verified();
        let steps = stock_flash();
        let mut facts = SafetyFacts::komodo_ready();
        facts.firmware_codename = "shiba".into();
        facts.firmware_filename = "shiba-factory-synthetic.zip".into();
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G04");

        let mut facts = SafetyFacts::komodo_ready();
        facts.firmware_filename = "shiba-factory-synthetic.zip".into();
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G04");

        let mut facts = SafetyFacts::komodo_ready();
        facts.target_partition = Partition::Boot;
        let lines = preview(&steps, Some(&facts), Some(&backup));
        assert_blocked(&lines, "G04");
        assert!(lines.iter().any(|line| line.contains("boot")));
    }

    #[test]
    fn security_patch_and_magisk_blocks() {
        let backup = verified();
        let steps = stock_flash();

        let mut facts = SafetyFacts::komodo_ready();
        facts.firmware_spl = Some("2025-01-01".into());
        facts.image_spl = Some("2025-01-01".into());
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G07");

        let mut facts = SafetyFacts::komodo_ready();
        facts.image_spl = Some("2026-03-01".into());
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G08");

        let mut facts = SafetyFacts::komodo_ready();
        facts.device_build = Some("TEST.260301.001".into());
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G08");

        let mut facts = SafetyFacts::komodo_ready();
        facts.device_spl = Some("2026-01-01".into());
        facts.device_build = Some("TEST.260101.001".into());
        facts.device_timestamp = Some(1_600_000_000);
        let lines = preview(&steps, Some(&facts), Some(&backup));
        assert!(
            lines.iter().all(|line| !line.starts_with("WOULD BLOCK")),
            "{lines:?}"
        );

        let mut facts = SafetyFacts::komodo_ready();
        facts.magisk_label = Some("7dbfba76".into());
        facts.magisk_code = Some(25207);
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G10");

        let mut facts = SafetyFacts::komodo_ready();
        facts.magisk_code = Some(30500);
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G10");

        let mut facts = SafetyFacts::komodo_ready();
        facts.kernel = Some("6.1.0-lineage-synthetic".into());
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G10");

        let mut facts = SafetyFacts::komodo_ready();
        facts.magisk_package = Some(tables().off_limits.packages[0].clone());
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G10");

        let mut facts = SafetyFacts::komodo_ready();
        facts.magisk_code = None;
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G10");
    }

    #[test]
    fn bootloader_and_slot_blocks() {
        let backup = verified();

        let mut facts = SafetyFacts::komodo_ready();
        facts.device_codename = "oriole".into();
        facts.plan_device = "oriole".into();
        facts.firmware_codename = "oriole".into();
        facts.firmware_filename = "oriole-factory-synthetic.zip".into();
        facts.target_partition = Partition::Boot;
        facts.bootloader_a = Some("15.3-13239611".into());
        facts.bootloader_b = Some("15.3-13239612".into());
        let steps = vec![flash(Slot::B, Partition::Boot, "/var/flashwright/boot.img")];
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G19");

        let mut facts = SafetyFacts::komodo_ready();
        facts.device_codename = "shiba".into();
        facts.plan_device = "shiba".into();
        facts.firmware_codename = "shiba".into();
        facts.firmware_filename = "shiba-factory-synthetic.zip".into();
        facts.bootloader_a = None;
        assert_blocked(&preview(&stock_flash(), Some(&facts), Some(&backup)), "G19");

        let mut facts = SafetyFacts::komodo_ready();
        facts.device_codename = "oriole".into();
        facts.plan_device = "oriole".into();
        facts.firmware_codename = "oriole".into();
        facts.firmware_filename = "oriole-factory-synthetic.zip".into();
        facts.target_partition = Partition::Boot;
        facts.api_level = Some(32);
        facts.bootloader_a = Some("15.3-13239612".into());
        facts.bootloader_b = Some("15.3-13239612".into());
        let steps = vec![flash(
            Slot::B,
            Partition::Bootloader,
            "/var/flashwright/bootloader.img",
        )];
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G23");

        let facts = SafetyFacts::komodo_ready();
        let steps = vec![flash(
            Slot::A,
            Partition::InitBoot,
            "/var/flashwright/init_boot.img",
        )];
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G24");

        let steps = vec![
            flash(
                Slot::B,
                Partition::InitBoot,
                "/var/flashwright/init_boot.img",
            ),
            flash(
                Slot::A,
                Partition::InitBoot,
                "/var/flashwright/init_boot.img",
            ),
        ];
        let lines = preview(&steps, Some(&facts), Some(&backup));
        assert_blocked(&lines, "G24");
        assert!(lines.iter().any(|line| line.contains("both slots")));
    }

    #[test]
    fn off_limits_regions_and_flags_block() {
        let facts = SafetyFacts::komodo_ready();
        let backup = verified();
        let vbmeta = vec![flash(
            Slot::B,
            Partition::Vbmeta,
            "/var/flashwright/vbmeta.img",
        )];
        assert_blocked(&preview(&vbmeta, Some(&facts), Some(&backup)), "G26");

        for region in ["lu0", "fips"] {
            let path = format!("/var/flashwright/{region}/init_boot.img");
            let steps = vec![flash(Slot::B, Partition::InitBoot, &path)];
            let lines = preview(&steps, Some(&facts), Some(&backup));
            assert_blocked(&lines, "G25");
            assert!(lines.iter().any(|line| line.contains(region)));
        }

        let steps = vec![flash(Slot::B, Partition::InitBoot, "-w")];
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G16");

        let steps = vec![flash(Slot::B, Partition::InitBoot, "--disable-verity")];
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G16");

        let needle = tables().off_limits.shell[0].clone();
        let path = format!("/var/flashwright/{needle}/init_boot.img");
        let steps = vec![flash(Slot::B, Partition::InitBoot, &path)];
        let lines = preview(&steps, Some(&facts), Some(&backup));
        assert_blocked(&lines, "G27");
        assert!(lines.iter().any(|line| line.contains("host shell")));

        let (blocked, reason) = g27(&["erase".into()], tables());
        assert!(blocked);
        assert!(reason.contains("off-limits"));

        let (blocked, reason) = g24(
            &[],
            Some(&facts),
            tables(),
            &["--slot".into(), "all".into()],
        );
        assert!(blocked, "{reason}");
        assert!(reason.contains("slot all"));
    }

    #[test]
    fn a_failed_boolean_gate_is_a_dry_run_block() {
        let mut facts = SafetyFacts::komodo_ready();
        facts.checks = LegacyChecks::fail("G01");
        assert_blocked(
            &preview(&stock_flash(), Some(&facts), Some(&verified())),
            "G01",
        );

        let mut facts = SafetyFacts::komodo_ready();
        facts.checks = LegacyChecks::fail("G17");
        let decisions = evaluate(&stock_flash(), Some(&facts), Some(&verified()));
        let lines = dry_run_lines(&stock_flash(), &decisions);
        assert_blocked(&lines, "G17");
        assert!(blocking(&decisions).is_empty());
    }

    #[test]
    fn missing_patch_fingerprint_and_magisk_fail_closed() {
        let backup = verified();
        let steps = stock_flash();
        let mut facts = SafetyFacts::komodo_ready();
        facts.firmware_timestamp = None;
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G07");

        let mut facts = SafetyFacts::komodo_ready();
        facts.image_fingerprint = None;
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G08");

        let mut facts = SafetyFacts::komodo_ready();
        facts.magisk_package = None;
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G10");

        let mut facts = SafetyFacts::komodo_ready();
        facts.kernel = None;
        assert_blocked(&preview(&steps, Some(&facts), Some(&backup)), "G10");
    }

    #[test]
    fn g04_compares_firmware_with_the_plan_device() {
        let mut facts = SafetyFacts::komodo_ready();
        facts.plan_device = "shiba".into();
        facts.device_codename = "komodo".into();
        facts.firmware_codename = "komodo".into();
        facts.firmware_filename = "komodo-factory-synthetic.zip".into();
        let lines = preview(&stock_flash(), Some(&facts), Some(&verified()));
        assert_blocked(&lines, "G04");
        assert!(lines.iter().any(|line| line.contains("shiba")));
    }

    #[test]
    fn g19_is_acknowledged_and_g20_is_a_block() {
        let mut facts = SafetyFacts::komodo_ready();
        facts.device_codename = "oriole".into();
        facts.plan_device = "oriole".into();
        facts.firmware_codename = "oriole".into();
        facts.firmware_filename = "oriole-factory-synthetic.zip".into();
        facts.target_partition = Partition::Boot;
        facts.bootloader_a = Some("15.3-13239611".into());
        let steps = vec![flash(Slot::B, Partition::Boot, "/var/flashwright/boot.img")];
        let backup = BackupState::Verified(BackupSet::bound(
            "set-oriole",
            "manifest",
            "synth-komodo-1",
            Slot::B,
            Partition::Boot,
        ));
        let open = evaluate(&steps, Some(&facts), Some(&backup));
        let g19 = open.iter().find(|gate| gate.id == "G19").unwrap();
        assert_eq!(g19.severity, Severity::Ack);
        assert!(g19.blocked);
        let acked = evaluate_acked(&steps, Some(&facts), Some(&backup), &["G19".into()]);
        assert!(!acked.iter().find(|gate| gate.id == "G19").unwrap().blocked);

        let mut facts = SafetyFacts::komodo_ready();
        facts.pending_ota = Some(true);
        let blocked = evaluate(&stock_flash(), Some(&facts), Some(&verified()));
        let g20 = blocked.iter().find(|gate| gate.id == "G20").unwrap();
        assert_eq!(g20.severity, Severity::Block);
        assert!(g20.blocked);
        let still = evaluate_acked(
            &stock_flash(),
            Some(&facts),
            Some(&verified()),
            &["G20".into()],
        );
        assert!(still.iter().find(|gate| gate.id == "G20").unwrap().blocked);
    }

    #[test]
    fn a_backup_for_another_slot_does_not_satisfy_g14() {
        let backup = BackupState::Verified(BackupSet::bound(
            "set-a",
            "manifest",
            "synth-komodo-1",
            Slot::A,
            Partition::InitBoot,
        ));
        assert_blocked(
            &preview(
                &stock_flash(),
                Some(&SafetyFacts::komodo_ready()),
                Some(&backup),
            ),
            "G14",
        );
    }

    #[test]
    fn step_gates_fail_closed_when_the_phone_changes() {
        let step = flash(
            Slot::B,
            Partition::InitBoot,
            "/var/flashwright/init_boot.img",
        );
        let mut facts = SafetyFacts::komodo_ready();
        assert!(evaluate_step(&step, Some(&facts)).is_empty());
        facts.unlocked = Some(false);
        assert!(evaluate_step(&step, Some(&facts))
            .iter()
            .any(|gate| gate.id == "G03"));
        facts.unlocked = Some(true);
        facts.partition_bytes = Some(10);
        assert!(evaluate_step(&step, Some(&facts))
            .iter()
            .any(|gate| gate.id == "G15"));
        facts.partition_bytes = None;
        assert!(evaluate_step(&step, Some(&facts))
            .iter()
            .any(|gate| gate.id == "G15"));
        let reboot = PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Reboot {
            serial: serial(),
            mode: RebootMode::System,
        }));
        facts = SafetyFacts::komodo_ready();
        facts.tools_verified = false;
        assert!(evaluate_step(&reboot, Some(&facts))
            .iter()
            .any(|gate| gate.id == "G21"));
        facts.tools_verified = true;
        facts.adb_server_ok = false;
        assert!(evaluate_step(&reboot, Some(&facts))
            .iter()
            .any(|gate| gate.id == "G22"));
        let set_active = PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::SetActive {
            serial: serial(),
            slot: Slot::B,
        }));
        facts = SafetyFacts::komodo_ready();
        facts.unlocked = Some(false);
        assert!(evaluate_step(&set_active, Some(&facts))
            .iter()
            .any(|gate| gate.id == "G03"));
        let mut huge = flash(Slot::B, Partition::InitBoot, "/var/flashwright/huge.img");
        if let PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash { image, .. })) = &mut huge {
            image.set_size(crate::cmd::MAX_BLOCK_LEN + 1);
        }
        assert!(evaluate_step(&huge, Some(&SafetyFacts::komodo_ready()))
            .iter()
            .any(|gate| gate.id == "G28"));
        assert_blocked(
            &preview(
                &[huge],
                Some(&SafetyFacts::komodo_ready()),
                Some(&verified()),
            ),
            "G28",
        );
    }

    #[test]
    fn bootloader_compare_and_spl_follow_the_ported_rules() {
        assert!(!bootloader_older("15.3-13239612", "15.3-13239612"));
        assert!(bootloader_older("15.3-13239611", "15.3-13239612"));
        assert!(bootloader_older("nope", "15.3-13239612"));
        assert_eq!(
            spl_from_build("TEST.260201.001").as_deref(),
            Some("2026-02-01")
        );
        assert_eq!(
            spl_from_build("BP3A.251005.004").as_deref(),
            Some("2025-10-05")
        );
        assert!(spl_from_build("nope").is_none());
    }

    #[test]
    fn ported_tables_are_loaded() {
        let loaded = tables();
        assert_eq!(loaded.devices.len(), 49);
        let komodo = loaded
            .devices
            .iter()
            .find(|row| row.codename == "komodo")
            .unwrap();
        assert!(komodo.has_init_boot);
        assert!(komodo.ab);
        let angler = loaded
            .devices
            .iter()
            .find(|row| row.codename == "angler")
            .unwrap();
        assert!(!angler.ab, "Nexus 6P is not an A/B device");
        let ryu = loaded
            .devices
            .iter()
            .find(|row| row.codename == "ryu")
            .unwrap();
        assert!(!ryu.ab, "Pixel C is not an A/B device");
        assert_eq!(komodo.bootloader_codename, "ripcurrentpro");
        assert!(loaded
            .combos
            .iter()
            .any(|combo| combo.label == "7dbfba76" && combo.version_code == 25207));
        assert!(loaded
            .bootloaders
            .iter()
            .any(|row| row.codename == "oriole" && row.min == "15.3-13239612"));
        assert!(loaded.kernels.iter().any(|name| name == "-mokee"));
        assert!(loaded
            .kernels
            .iter()
            .any(|name| name == "-mokee-MoRoKernel"));
        assert_eq!(loaded.slots.both_slots, "block");
        assert_eq!(loaded.slots.slot_all, "block");
        assert!(loaded.off_limits.regions.iter().any(|name| name == "lu0"));
        assert!(loaded.off_limits.regions.iter().any(|name| name == "fips"));
        let items = ported_items();
        assert!(items.iter().any(|item| {
            item.upstream_path == "android_devices.json" && item.commit == "081286d"
        }));
        assert!(items
            .iter()
            .any(|item| item.upstream_path == "constants.py"));
        assert!(items.iter().any(|item| item.upstream_path == "runtime.py"));
        assert!(items
            .iter()
            .any(|item| item.upstream_path == "pf_modules.py"));
    }
}
