// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Wizard-facing session.
//!
//! A later change moves the full window state machine into this module.
//! This seam is the public read, plan, and confirm API that window will call.
//! The only public write entry point is [`WizardSession::confirm_and_run`].
//! It mints a write token internally. A second call with the same plan fails
//! because the plan has been consumed.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::cmd::{CleanupCmd, DeviceSerial, ReadCmd, WriteCmd};
use crate::device::{PlatformToolsTransport, Slot};
use crate::parse::{self, Verdict};
use crate::proc::CommandRunner;
use crate::safety::{self, BackupSet, BackupState, FactEvidence, SafetyFacts};
use crate::token::mint_confirmed;
use crate::CoreError;

pub const PLAN_SCHEMA: &str = "flashwright.plan.v1";
pub const PLAN_TTL_MS: i64 = 15 * 60 * 1000;

/// Window phases. Only [`Phase::Review`] may confirm or dry-run a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
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

pub trait Clock: Send + Sync {
    fn unix_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }
}

pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn unix_ms(&self) -> i64 {
        self.0
    }
}

/// One catalogue step. Reads and writes are both part of the plan hash.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlanStep {
    Read(ReadCmd),
    Write(WriteCmd),
    /// Removes the work directory. This is not a flash write.
    Cleanup(CleanupCmd),
}

/// Stock image bound into a patch plan. The hashes are part of the plan hash.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImageSeal {
    pub role: String,
    pub sha1: String,
    pub sha256: String,
}

/// Firmware the operator selected. The phone identity is read from the device.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FirmwareClaim {
    pub codename: String,
    pub filename: String,
    pub spl: Option<String>,
    pub fingerprint: Option<String>,
    pub timestamp: Option<u64>,
    pub bootloader: Option<String>,
    pub image_spl: Option<String>,
    pub image_fingerprint: Option<String>,
    pub images: Vec<ImageSeal>,
}

/// What the operator wants done. Core reads every fact from the phone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRequest {
    pub serial: String,
    pub dry_run: bool,
    pub steps: Vec<PlanStep>,
    pub firmware: FirmwareClaim,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanStepView {
    pub class: &'static str,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanPreview {
    pub plan_hash: String,
    pub plan_code: String,
    pub expires_unix_ms: i64,
    pub dry_run: bool,
    pub steps: Vec<PlanStepView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub lines: Vec<String>,
}

#[derive(Clone, serde::Serialize)]
struct FactSnap {
    device: String,
    firmware: String,
    filename: String,
    authorised: u32,
    unlocked: Option<bool>,
    tools_verified: bool,
    tools_match: bool,
    adb_server_ok: bool,
    partition_bytes: Option<u64>,
    pending_ota: Option<bool>,
    device_spl: Option<String>,
    firmware_spl: Option<String>,
    magisk_code: Option<u32>,
    kernel: Option<String>,
    evidence: Vec<EvidenceSnap>,
}

#[derive(Clone, serde::Serialize)]
struct EvidenceSnap {
    source: String,
    sha256: String,
}

#[derive(Clone, serde::Serialize)]
struct GateSnap {
    id: String,
    severity: String,
    status: String,
}

#[derive(Clone, serde::Serialize)]
struct BackupSnap {
    set_id: String,
    manifest_sha256: String,
    serial_sha256: String,
    slot: String,
    partition: String,
}

#[derive(Clone, serde::Serialize)]
struct TimeoutSnap {
    timeout_s: u64,
    watchdog_s: u64,
}

#[derive(serde::Serialize)]
struct PlanBody<'a> {
    schema: &'static str,
    serial: &'a str,
    nonce: &'a str,
    dry_run: bool,
    expires_unix_ms: i64,
    steps: &'a [PlanStep],
    images: &'a [ImageSeal],
    facts: Option<&'a FactSnap>,
    gates: &'a [GateSnap],
    backup: Option<&'a BackupSnap>,
    timeouts: &'a [TimeoutSnap],
    acks: &'a [String],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlanLife {
    Issued,
    Consumed,
    Discarded,
}

struct HeldPlan {
    hash: String,
    nonce: String,
    expires_unix_ms: i64,
    dry_run: bool,
    steps: Vec<PlanStep>,
    serial: String,
    firmware: FirmwareClaim,
    facts: Option<FactSnap>,
    gates: Vec<GateSnap>,
    backup: Option<BackupSnap>,
    timeouts: Vec<TimeoutSnap>,
    acks: Vec<String>,
    files: Vec<InputFile>,
    life: PlanLife,
}

#[derive(Clone)]
struct InputFile {
    path: String,
    sha256: Option<String>,
}

/// Read, plan, and confirm session for a window.
///
/// The phase starts at [`Phase::Connect`]. [`Self::build_plan`] enters
/// [`Phase::Review`]. Confirm and dry-run are refused in every other phase.
pub struct WizardSession<R: CommandRunner> {
    phase: Phase,
    clock: Box<dyn Clock>,
    held: Option<HeldPlan>,
    transport: PlatformToolsTransport<R>,
    safety: Option<SafetyFacts>,
    backup: Option<BackupState>,
    used: BTreeSet<String>,
}

impl<R: CommandRunner> WizardSession<R> {
    pub fn new(transport: PlatformToolsTransport<R>) -> Self {
        Self::with_clock(transport, Box::new(SystemClock))
    }

    pub fn with_clock(transport: PlatformToolsTransport<R>, clock: Box<dyn Clock>) -> Self {
        Self {
            phase: Phase::Connect,
            clock,
            held: None,
            transport,
            safety: None,
            backup: None,
            used: BTreeSet::new(),
        }
    }

    /// Read gate facts from the phone. Callers cannot supply them.
    pub async fn read_gate_facts(&mut self, serial: &DeviceSerial) -> Result<(), CoreError> {
        self.safety = Some(observe_phone(&self.transport, serial).await?);
        Ok(())
    }

    /// Back up both slots of the boot image and vbmeta, bound to this phone and target.
    pub async fn backup_stock(
        &mut self,
        serial: &DeviceSerial,
        slot: Slot,
        partition: crate::device::Partition,
        dest: &Path,
    ) -> Result<BackupSet, CoreError> {
        match safety::capture_stock(&self.transport, serial, slot, partition, dest).await {
            Ok(set) => {
                self.backup = Some(BackupState::Verified(set.clone()));
                Ok(set)
            }
            Err(block) => {
                let reason = block.reason.clone();
                self.backup = Some(BackupState::Blocked(block));
                Err(rejected(reason))
            }
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn transport(&self) -> &PlatformToolsTransport<R> {
        &self.transport
    }

    pub async fn read(&self, cmd: ReadCmd) -> Result<crate::proc::RunResult, CoreError> {
        Ok(self.transport.run_read(cmd).await?)
    }

    /// Store a plan and move to review.
    ///
    /// The caller describes the work. Core reads the phone, hashes the files,
    /// and puts those facts, their evidence, the gates, the backup, the
    /// timeouts, and a random nonce into the plan hash. The plan expires
    /// fifteen minutes after it is built.
    pub async fn build_plan(&mut self, mut request: PlanRequest) -> Result<PlanPreview, CoreError> {
        if self.phase == Phase::Flash {
            return Err(rejected("A job is already running."));
        }
        let expires_unix_ms = self.clock.unix_ms().saturating_add(PLAN_TTL_MS);
        if let Err(err) = check_serial(&request) {
            self.note_discard_input();
            return Err(err);
        }
        seal_image_sizes(&mut request.steps)?;
        check_images(&request.firmware.images)?;
        let views = step_views(&request.steps)?;
        let facts = self.collect_facts(&request).await;
        let decisions = safety::evaluate(&request.steps, Some(&facts), self.backup.as_ref());
        let nonce = Self::next_nonce()?;
        let acks = Vec::new();
        let snap = fact_snap(&facts);
        let gates = gate_snaps(&decisions, &acks);
        let backup = backup_snap(self.backup.as_ref());
        let timeouts = step_timeouts(&request.steps);
        let hash = hash_plan(
            &request,
            &nonce,
            expires_unix_ms,
            Some(&snap),
            &gates,
            backup.as_ref(),
            &timeouts,
            &acks,
        )?;
        if self.used.contains(&hash) {
            return Err(rejected("That plan was already used."));
        }
        let files = input_files(&request);
        let preview = PlanPreview {
            plan_code: plan_code(&hash),
            plan_hash: hash.clone(),
            expires_unix_ms,
            dry_run: request.dry_run,
            steps: views,
        };
        self.held = Some(HeldPlan {
            hash,
            nonce,
            expires_unix_ms,
            dry_run: request.dry_run,
            steps: request.steps,
            serial: request.serial,
            firmware: request.firmware,
            facts: Some(snap),
            gates,
            backup,
            timeouts,
            acks,
            files,
            life: PlanLife::Issued,
        });
        self.safety = Some(facts);
        self.phase = Phase::Review;
        Ok(preview)
    }

    async fn collect_facts(&self, request: &PlanRequest) -> SafetyFacts {
        let mut facts = match crate::cmd::DeviceSerial::try_from(request.serial.as_str()) {
            Ok(serial) => observe_phone(&self.transport, &serial)
                .await
                .unwrap_or_else(|_| empty_facts()),
            Err(_) => empty_facts(),
        };
        apply_firmware(&mut facts, request);
        hash_images(&mut facts, &request.steps);
        if facts.host_free_bytes.is_none() {
            facts.host_free_bytes = host_free_bytes();
        }
        facts
    }

    async fn reprobe(&mut self) -> Result<(), CoreError> {
        let request = {
            let held = self.held.as_ref().expect("plan");
            PlanRequest {
                serial: held.serial.clone(),
                dry_run: held.dry_run,
                steps: held.steps.clone(),
                firmware: held.firmware.clone(),
            }
        };
        self.safety = Some(self.collect_facts(&request).await);
        Ok(())
    }

    /// Record an acknowledgement. The new plan hash covers that acknowledgement.
    pub fn acknowledge(
        &mut self,
        plan_hash_value: &str,
        gate_id: &str,
    ) -> Result<PlanPreview, CoreError> {
        self.ready(plan_hash_value)?;
        let severity = safety::evaluate(
            &self.held.as_ref().expect("plan").steps,
            self.safety.as_ref(),
            self.backup.as_ref(),
        );
        let Some(gate) = severity.iter().find(|gate| gate.id == gate_id) else {
            return Err(rejected("That check is not part of this plan."));
        };
        if gate.severity != safety::Severity::Ack {
            return Err(rejected("This check cannot be acknowledged."));
        }
        if !gate.blocked
            && !self
                .held
                .as_ref()
                .expect("plan")
                .acks
                .iter()
                .any(|id| id == gate_id)
        {
            return Err(rejected(
                "That check is not waiting for an acknowledgement.",
            ));
        }
        let held = self.held.as_mut().expect("plan");
        if !held.acks.iter().any(|id| id == gate_id) {
            held.acks.push(gate_id.to_string());
            held.acks.sort();
        }
        let request = PlanRequest {
            serial: held.serial.clone(),
            dry_run: held.dry_run,
            steps: held.steps.clone(),
            firmware: held.firmware.clone(),
        };
        let expires = held.expires_unix_ms;
        let decisions = safety::evaluate_acked(
            &held.steps,
            self.safety.as_ref(),
            self.backup.as_ref(),
            &held.acks,
        );
        held.gates = gate_snaps(&decisions, &held.acks);
        held.hash = hash_plan(
            &request,
            &held.nonce,
            expires,
            held.facts.as_ref(),
            &held.gates,
            held.backup.as_ref(),
            &held.timeouts,
            &held.acks,
        )?;
        let hash = held.hash.clone();
        Ok(PlanPreview {
            plan_code: plan_code(&hash),
            plan_hash: hash,
            expires_unix_ms: held.expires_unix_ms,
            dry_run: held.dry_run,
            steps: step_views(&held.steps)?,
        })
    }

    /// Evaluate every safety gate. A dry-run plan is consumed. A real plan is left issued.
    ///
    /// A passing plan prints `WOULD RUN` for each write. A failing gate prints
    /// `WOULD BLOCK` and the reason, and no write line. No token is minted.
    pub fn dry_run(&mut self, plan_hash_value: &str) -> Result<Vec<String>, CoreError> {
        self.ready(plan_hash_value)?;
        let held = self.held.as_ref().expect("plan");
        if !held.dry_run {
            return Err(rejected("This plan is not a dry run."));
        }
        let decisions = safety::evaluate_acked(
            &held.steps,
            self.safety.as_ref(),
            self.backup.as_ref(),
            &held.acks,
        );
        let mut lines = safety::dry_run_lines(&held.steps, &decisions);
        if lines.iter().all(|line| !line.starts_with("WOULD BLOCK")) {
            for step in &held.steps {
                let blocks = safety::evaluate_step(step, self.safety.as_ref());
                if !blocks.is_empty() {
                    lines = blocks
                        .iter()
                        .map(|gate| format!("WOULD BLOCK: {} {}", gate.id, gate.reason))
                        .collect();
                    break;
                }
            }
        }
        let hash = held.hash.clone();
        self.consume(&hash);
        Ok(lines)
    }

    /// Run the reviewed plan once. The token never leaves this function.
    pub async fn confirm_and_run(&mut self, plan_hash_value: &str) -> Result<RunReport, CoreError> {
        self.confirm_and_run_finally(plan_hash_value, 0).await
    }

    /// Run a confirmed plan. The last `finally_steps` still run when an earlier step fails.
    ///
    /// Safety gates are evaluated before a token is minted and again before each write.
    pub async fn confirm_and_run_finally(
        &mut self,
        plan_hash_value: &str,
        finally_steps: usize,
    ) -> Result<RunReport, CoreError> {
        self.ready(plan_hash_value)?;
        let dry_run = self.held.as_ref().expect("plan").dry_run;
        if dry_run {
            return Err(rejected("A dry-run plan does not write."));
        }
        let files = self.held.as_ref().expect("plan").files.clone();
        if !files_match(&files) {
            let hash = self.held.as_ref().expect("plan").hash.clone();
            self.discard(&hash);
            return Err(rejected("An input file changed. Build the plan again."));
        }
        self.reprobe().await?;
        let steps = self.held.as_ref().expect("plan").steps.clone();
        let acks = self.held.as_ref().expect("plan").acks.clone();
        let decisions =
            safety::evaluate_acked(&steps, self.safety.as_ref(), self.backup.as_ref(), &acks);
        if let Some(reason) = block_reason(&decisions) {
            return Err(rejected(format!("Blocked: {reason}")));
        }
        if finally_steps > steps.len() {
            return Err(rejected("Cleanup is longer than the plan."));
        }
        let held = self.held.take().expect("review plan");
        let hash = held.hash.clone();
        self.consume(&hash);
        self.phase = Phase::Flash;
        let writes: Vec<WriteCmd> = held
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::Write(cmd) => Some(cmd.clone()),
                PlanStep::Read(_) | PlanStep::Cleanup(_) => None,
            })
            .collect();
        let (plan, token) = mint_confirmed(&held.hash, &held.serial, &writes);
        self.transport.arm(&plan);
        let trailing_cleanup = held
            .steps
            .iter()
            .rev()
            .take_while(|step| matches!(step, PlanStep::Cleanup(_)))
            .count();
        let body_end = held.steps.len() - trailing_cleanup;
        let split = body_end.saturating_sub(finally_steps);
        let finally_writes = held.steps[split..]
            .iter()
            .filter(|step| matches!(step, PlanStep::Write(_)))
            .count();
        let mut lines = Vec::new();
        let mut failed: Option<CoreError> = None;
        for step in &held.steps[..split] {
            match self.run_planned(&token, step).await {
                Ok(text) => lines.push(text),
                Err(err) => {
                    failed = Some(err);
                    break;
                }
            }
        }
        if failed.is_some() {
            self.transport.keep_last_pending(finally_writes);
        }
        for step in &held.steps[split..] {
            match self.run_planned(&token, step).await {
                Ok(text) => lines.push(text),
                Err(err) => {
                    if failed.is_none() {
                        failed = Some(err);
                    }
                }
            }
        }
        drop(token);
        if let Some(err) = failed {
            self.phase = Phase::Recovery;
            return Err(err);
        }
        self.phase = Phase::Done;
        Ok(RunReport { lines })
    }

    async fn run_planned(
        &mut self,
        token: &crate::token::WriteToken,
        step: &PlanStep,
    ) -> Result<String, CoreError> {
        if let PlanStep::Write(cmd) = step {
            self.refresh_live(cmd).await?;
            let blocks = safety::evaluate_step(step, self.safety.as_ref());
            if let Some(gate) = blocks.first() {
                return Err(rejected(format!("Blocked: {} {}", gate.id, gate.reason)));
            }
        }
        match step {
            PlanStep::Read(cmd) => {
                let result = self.transport.run_read(cmd.clone()).await?;
                if !result.success_exit() {
                    return Err(rejected("A read step failed."));
                }
                Ok(result.stdout_text())
            }
            PlanStep::Write(cmd) => {
                let result = self.transport.run_write(token, cmd.clone()).await;
                match result {
                    Ok(result) if write_ok(cmd, &result) => Ok(result.stdout_text()),
                    Ok(_) | Err(_) => Err(rejected("A write step failed.")),
                }
            }
            PlanStep::Cleanup(cmd) => {
                let result = self.transport.run_cleanup(cmd).await?;
                if !cleanup_ok(&result) {
                    return Err(rejected("Cleanup failed."));
                }
                Ok(result.stdout_text())
            }
        }
    }

    async fn refresh_live(&mut self, cmd: &WriteCmd) -> Result<(), CoreError> {
        let (verified, matched, server) = self.transport.tool_gate();
        if self.safety.is_none() {
            self.safety = Some(empty_facts());
        }
        let facts = self.safety.as_mut().expect("facts");
        facts.tools_verified = verified;
        facts.tools_match = matched;
        facts.adb_server_ok = server;
        if !matches!(cmd, WriteCmd::Fastboot(_)) {
            return Ok(());
        }
        let serial = cmd.serial().clone();
        let unlocked = self
            .transport
            .run_read(ReadCmd::Fastboot(crate::cmd::FastbootRead::Getvar {
                serial: serial.clone(),
                var: crate::cmd::FastbootVar::Unlocked,
            }))
            .await;
        facts.unlocked = unlocked.ok().and_then(|result| {
            if result.success_exit() {
                parse_yes(&result.stdout_text())
            } else {
                None
            }
        });
        if let Some((partition, slot)) = flash_target(cmd) {
            let size = self
                .transport
                .run_read(ReadCmd::Fastboot(crate::cmd::FastbootRead::Getvar {
                    serial,
                    var: crate::cmd::FastbootVar::PartitionSize { partition, slot },
                }))
                .await;
            facts.partition_bytes = size.ok().and_then(|result| {
                if result.success_exit() {
                    parse_size(&result.stdout_text())
                } else {
                    None
                }
            });
        }
        Ok(())
    }
}

impl<R: CommandRunner> WizardSession<R> {
    fn ready(&mut self, plan_hash_value: &str) -> Result<(), CoreError> {
        if self.phase != Phase::Review {
            let reason = if matches!(self.phase, Phase::Done | Phase::Flash | Phase::Recovery) {
                "That plan was already used."
            } else {
                "A plan can only be confirmed from the review step."
            };
            return Err(rejected(reason));
        }
        let Some(held) = self.held.as_ref() else {
            return Err(rejected("That plan was already used."));
        };
        if held.hash != plan_hash_value {
            return Err(rejected("That plan code was not issued by Flashwright."));
        }
        if self.clock.unix_ms() > held.expires_unix_ms {
            let hash = held.hash.clone();
            self.discard(&hash);
            return Err(rejected("The plan has expired. Build it again."));
        }
        if self.held.as_ref().expect("plan").life != PlanLife::Issued {
            return Err(rejected("That plan was already used."));
        }
        step_views(&self.held.as_ref().expect("plan").steps)?;
        Ok(())
    }

    fn next_nonce() -> Result<String, CoreError> {
        let mut bytes = [0u8; 16];
        fill_csprng(&mut bytes)?;
        let mut nonce = String::with_capacity(32);
        for byte in bytes {
            nonce.push_str(&format!("{byte:02x}"));
        }
        Ok(nonce)
    }

    fn consume(&mut self, hash: &str) {
        self.used.insert(hash.to_string());
        if let Some(held) = self.held.as_mut() {
            if held.hash == hash {
                held.life = PlanLife::Consumed;
            }
        }
        self.phase = Phase::Done;
    }

    fn discard(&mut self, hash: &str) {
        self.used.insert(hash.to_string());
        if let Some(held) = self.held.as_mut() {
            if held.hash == hash {
                held.life = PlanLife::Discarded;
            }
        }
    }

    fn note_discard_input(&mut self) {
        if let Some(held) = self.held.as_ref() {
            if held.life == PlanLife::Issued {
                let hash = held.hash.clone();
                self.discard(&hash);
            }
        }
    }
}

fn rejected(reason: impl Into<String>) -> CoreError {
    CoreError::Rejected {
        reason: reason.into(),
    }
}

fn check_serial(request: &PlanRequest) -> Result<(), CoreError> {
    for step in &request.steps {
        let serial = match step {
            PlanStep::Read(cmd) => cmd.serial().map(|serial| serial.as_str()),
            PlanStep::Write(cmd) => Some(cmd.serial().as_str()),
            PlanStep::Cleanup(cmd) => Some(cmd.serial().as_str()),
        };
        if let Some(serial) = serial {
            if serial != request.serial {
                return Err(rejected("A plan step names a different phone."));
            }
        }
    }
    Ok(())
}

fn step_views(steps: &[PlanStep]) -> Result<Vec<PlanStepView>, CoreError> {
    let mut views = Vec::with_capacity(steps.len());
    for step in steps {
        let (class, rendered) = match step {
            PlanStep::Read(cmd) => (
                "read",
                crate::cmd::read_argv(cmd).map_err(|err| rejected(err.to_string()))?,
            ),
            PlanStep::Write(cmd) => (
                "write",
                crate::cmd::write_argv(cmd).map_err(|err| rejected(err.to_string()))?,
            ),
            PlanStep::Cleanup(cmd) => (
                "cleanup",
                crate::cmd::cleanup_argv(cmd).map_err(|err| rejected(err.to_string()))?,
            ),
        };
        views.push(PlanStepView {
            class,
            argv: rendered.args,
        });
    }
    Ok(views)
}

pub fn plan_hash(request: &PlanRequest, expires_unix_ms: i64) -> Result<String, CoreError> {
    hash_plan(request, "", expires_unix_ms, None, &[], None, &[], &[])
}

#[allow(clippy::too_many_arguments)]
fn hash_plan(
    request: &PlanRequest,
    nonce: &str,
    expires_unix_ms: i64,
    facts: Option<&FactSnap>,
    gates: &[GateSnap],
    backup: Option<&BackupSnap>,
    timeouts: &[TimeoutSnap],
    acks: &[String],
) -> Result<String, CoreError> {
    let body = PlanBody {
        schema: PLAN_SCHEMA,
        serial: &request.serial,
        nonce,
        dry_run: request.dry_run,
        expires_unix_ms,
        steps: &request.steps,
        images: &request.firmware.images,
        facts,
        gates,
        backup,
        timeouts,
        acks,
    };
    let bytes = serde_jcs::to_vec(&body).map_err(|err| rejected(err.to_string()))?;
    let digest = Sha256::digest(bytes);
    let mut hash = String::from("flp1-");
    for byte in digest {
        hash.push_str(&format!("{byte:02x}"));
    }
    Ok(hash)
}

fn fact_snap(facts: &SafetyFacts) -> FactSnap {
    FactSnap {
        device: facts.plan_device.clone(),
        firmware: facts.firmware_codename.clone(),
        filename: facts.firmware_filename.clone(),
        authorised: facts.authorised_devices,
        unlocked: facts.unlocked,
        tools_verified: facts.tools_verified,
        tools_match: facts.tools_match,
        adb_server_ok: facts.adb_server_ok,
        partition_bytes: facts.partition_bytes,
        pending_ota: facts.pending_ota,
        device_spl: facts.device_spl.clone(),
        firmware_spl: facts.firmware_spl.clone(),
        magisk_code: facts.magisk_code,
        kernel: facts.kernel.clone(),
        evidence: facts
            .evidence
            .iter()
            .map(|item| EvidenceSnap {
                source: item.source.clone(),
                sha256: item.sha256.clone(),
            })
            .collect(),
    }
}

fn gate_snaps(decisions: &[safety::GateDecision], acks: &[String]) -> Vec<GateSnap> {
    decisions
        .iter()
        .map(|gate| {
            let status = if !gate.blocked && acks.iter().any(|id| id == gate.id) {
                "acked"
            } else if gate.blocked {
                "fail"
            } else {
                "pass"
            };
            GateSnap {
                id: gate.id.to_string(),
                severity: match gate.severity {
                    safety::Severity::Block => "block".into(),
                    safety::Severity::Ack => "ack".into(),
                },
                status: status.into(),
            }
        })
        .collect()
}

fn backup_snap(backup: Option<&BackupState>) -> Option<BackupSnap> {
    match backup {
        Some(BackupState::Verified(set)) => Some(BackupSnap {
            set_id: set.set_id.clone(),
            manifest_sha256: set.manifest_sha256.clone(),
            serial_sha256: set.serial_sha256.clone(),
            slot: set.slot.as_str().into(),
            partition: set.partition.fastboot_name().into(),
        }),
        _ => None,
    }
}

fn step_timeouts(steps: &[PlanStep]) -> Vec<TimeoutSnap> {
    steps
        .iter()
        .map(|step| {
            let budget = match step {
                PlanStep::Read(cmd) => crate::timeouts::read_budget(cmd),
                PlanStep::Cleanup(_) => crate::timeouts::cleanup_budget(),
                PlanStep::Write(cmd) => {
                    let size = write_size(cmd);
                    crate::timeouts::write_budget(cmd, size, 1.0).unwrap_or(
                        crate::timeouts::StepBudget {
                            timeout: std::time::Duration::from_secs(0),
                            watchdog: None,
                            finalising: None,
                        },
                    )
                }
            };
            TimeoutSnap {
                timeout_s: budget.timeout.as_secs(),
                watchdog_s: budget.watchdog.map(|wait| wait.as_secs()).unwrap_or(0),
            }
        })
        .collect()
}

fn write_size(cmd: &WriteCmd) -> u64 {
    match cmd {
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash { image, .. }) => image.size_bytes(),
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Update { package, .. }) => {
            crate::timeouts::update_size_bytes(Path::new(package.path()), package.size_bytes())
        }
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload { package, .. }) => {
            package.size_bytes()
        }
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Push { src, .. }) => match src {
            crate::cmd::HostRef::Image(image) => image.size_bytes(),
            crate::cmd::HostRef::Asset(_) => 0,
        },
        _ => 0,
    }
}

fn apply_firmware(facts: &mut SafetyFacts, request: &PlanRequest) {
    if !request.firmware.codename.is_empty() {
        facts.firmware_codename = request.firmware.codename.clone();
    }
    if !request.firmware.filename.is_empty() {
        facts.firmware_filename = request.firmware.filename.clone();
    }
    if request.firmware.spl.is_some() {
        facts.firmware_spl = request.firmware.spl.clone();
        facts.image_spl = request
            .firmware
            .image_spl
            .clone()
            .or(request.firmware.spl.clone());
    }
    if request.firmware.fingerprint.is_some() {
        facts.firmware_fingerprint = request.firmware.fingerprint.clone();
        facts.image_fingerprint = request
            .firmware
            .image_fingerprint
            .clone()
            .or(request.firmware.fingerprint.clone());
    }
    if request.firmware.timestamp.is_some() {
        facts.firmware_timestamp = request.firmware.timestamp;
    }
    if request.firmware.bootloader.is_some() {
        facts.firmware_bootloader = request.firmware.bootloader.clone();
    }
    if let Some(partition) = request.steps.iter().find_map(|step| match step {
        PlanStep::Write(WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash {
            partition, ..
        })) => Some(*partition),
        _ => None,
    }) {
        facts.target_partition = partition;
    }
    if facts.plan_device.is_empty() {
        facts.plan_device = facts.device_codename.clone();
    }
}

fn hash_images(facts: &mut SafetyFacts, steps: &[PlanStep]) {
    let mut image_sha = None;
    for step in steps {
        let Some(path) = image_path(step) else {
            continue;
        };
        if let Some(digest) = hash_file(Path::new(path)) {
            image_sha = Some(digest);
        }
    }
    if let Some(digest) = image_sha {
        facts.image_sha256 = Some(digest.clone());
        facts.evidence.push(FactEvidence {
            source: "image-file".into(),
            sha256: digest.clone(),
        });
        if facts.firmware_sha256.is_none() {
            facts.firmware_sha256 = Some(digest);
        }
    }
    let firmware = Path::new(&facts.firmware_filename);
    if firmware.is_file() {
        if let Some(digest) = hash_file(firmware) {
            facts.firmware_sha256 = Some(digest.clone());
            facts.evidence.push(FactEvidence {
                source: "firmware-file".into(),
                sha256: digest,
            });
        }
    }
}

fn image_path(step: &PlanStep) -> Option<&str> {
    match step {
        PlanStep::Write(WriteCmd::Fastboot(
            crate::cmd::FastbootWrite::Flash { image, .. }
            | crate::cmd::FastbootWrite::Update { package: image, .. },
        ))
        | PlanStep::Write(WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload {
            package: image,
            ..
        })) => Some(image.path()),
        PlanStep::Write(WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Push {
            src: crate::cmd::HostRef::Image(image),
            ..
        })) => Some(image.path()),
        PlanStep::Write(_) | PlanStep::Read(_) | PlanStep::Cleanup(_) => None,
    }
}

fn check_images(images: &[ImageSeal]) -> Result<(), CoreError> {
    for image in images {
        if image.role.is_empty()
            || image.role.len() > 64
            || !hex_len(&image.sha1, 40)
            || !hex_len(&image.sha256, 64)
        {
            return Err(rejected("The image binding is not valid."));
        }
    }
    Ok(())
}

fn hex_len(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn cleanup_ok(result: &crate::proc::RunResult) -> bool {
    result.success_exit()
        && matches!(
            parse::parse_fixed_shell(&format!("{}{}", result.stdout_text(), result.stderr_text())),
            parse::Verdict::Ok | parse::Verdict::Uncertain { .. }
        )
}

fn input_files(request: &PlanRequest) -> Vec<InputFile> {
    let mut files = Vec::new();
    for step in &request.steps {
        let Some(path) = image_path(step) else {
            continue;
        };
        push_input(&mut files, path);
    }
    if !request.firmware.filename.is_empty() {
        push_input(&mut files, &request.firmware.filename);
    }
    files
}

fn push_input(files: &mut Vec<InputFile>, path: &str) {
    if files.iter().any(|file| file.path == path) {
        return;
    }
    files.push(InputFile {
        sha256: hash_file(Path::new(path)),
        path: path.to_string(),
    });
}

fn files_match(expected: &[InputFile]) -> bool {
    expected
        .iter()
        .all(|file| hash_file(Path::new(&file.path)) == file.sha256)
}

fn hash_file(path: &Path) -> Option<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Some(safety::sha256_hex(&hasher.finalize()))
}

fn empty_facts() -> SafetyFacts {
    SafetyFacts::blank()
}

fn block_reason(decisions: &[safety::GateDecision]) -> Option<String> {
    let reason = decisions
        .iter()
        .filter(|gate| gate.blocked)
        .map(|gate| format!("{} {}", gate.id, gate.reason))
        .collect::<Vec<_>>()
        .join("; ");
    if reason.is_empty() {
        None
    } else {
        Some(reason)
    }
}

fn seal_image_sizes(steps: &mut [PlanStep]) -> Result<(), CoreError> {
    for step in steps {
        let image = match step {
            PlanStep::Write(WriteCmd::Fastboot(
                crate::cmd::FastbootWrite::Flash { image, .. }
                | crate::cmd::FastbootWrite::Update { package: image, .. },
            ))
            | PlanStep::Write(WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload {
                package: image,
                ..
            })) => image,
            PlanStep::Write(WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Push {
                src: crate::cmd::HostRef::Image(image),
                ..
            })) => image,
            _ => continue,
        };
        let path = Path::new(image.path());
        if path.is_file() {
            let len = std::fs::metadata(path)
                .map_err(|err| rejected(err.to_string()))?
                .len();
            image.set_size(len);
        }
    }
    Ok(())
}

fn flash_target(cmd: &WriteCmd) -> Option<(crate::device::Partition, Slot)> {
    match cmd {
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash {
            partition, slot, ..
        }) => Some((*partition, *slot)),
        _ => None,
    }
}

fn parse_yes(text: &str) -> Option<bool> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("yes") || lower.contains("unlocked: true") {
        Some(true)
    } else if lower.contains("no") || lower.contains("unlocked: false") {
        Some(false)
    } else {
        None
    }
}

fn parse_size(text: &str) -> Option<u64> {
    let token = text.split_whitespace().last()?.trim();
    if let Some(hex) = token.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        token.parse().ok()
    }
}

async fn observe_phone<R: CommandRunner>(
    transport: &PlatformToolsTransport<R>,
    serial: &DeviceSerial,
) -> Result<SafetyFacts, CoreError> {
    use crate::cmd::{AdbHostRead, AdbShellRead, FastbootRead, FastbootVar};
    use crate::device::{parse_adb_devices, parse_getprop, LockState, Mode};

    let devices = transport
        .run_read(ReadCmd::AdbHost(AdbHostRead::Devices))
        .await?;
    let authorised = parse_adb_devices(&devices.stdout_text())
        .into_iter()
        .filter(|entry| entry.mode == Mode::Adb)
        .count() as u32;
    let props_run = transport
        .run_read(ReadCmd::AdbShell(AdbShellRead::GetpropAll {
            serial: serial.clone(),
        }))
        .await?;
    let props = parse_getprop(&props_run.stdout_text());
    let codename = props.get("ro.product.device").cloned().unwrap_or_default();
    let (verified, matched, server) = transport.tool_gate();
    let mut facts = empty_facts();
    note(&mut facts, "adb-devices", &devices.stdout);
    note(&mut facts, "getprop", &props_run.stdout);
    facts.device_codename = codename.clone();
    facts.plan_device = codename;
    facts.authorised_devices = authorised;
    facts.tools_verified = verified;
    facts.tools_match = matched;
    facts.adb_server_ok = server;
    facts.active_slot = props
        .get("ro.boot.slot_suffix")
        .and_then(|value| crate::device::parse_slot(value));
    facts.device_spl = props.get("ro.build.version.security_patch").cloned();
    facts.device_build = props.get("ro.build.id").cloned();
    facts.image_fingerprint = props.get("ro.build.fingerprint").cloned();
    facts.firmware_fingerprint = facts.image_fingerprint.clone();
    facts.device_timestamp = props
        .get("ro.build.date.utc")
        .and_then(|value| value.parse().ok());
    facts.api_level = props
        .get("ro.build.version.sdk")
        .and_then(|value| value.parse().ok());
    facts.device_bootloader = props.get("ro.bootloader").cloned();
    facts.bootloader_a = facts.device_bootloader.clone();
    facts.bootloader_b = facts.device_bootloader.clone();
    facts.kernel = props.get("ro.kernel.version").cloned();
    facts.unlocked = match crate::device::lock_from_adb(&props) {
        LockState::Unlocked => Some(true),
        LockState::Locked => Some(false),
        LockState::Unknown => None,
    };
    facts.pending_ota = match props.get("persist.sys.update.pending").map(String::as_str) {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    };
    if let Ok(code) = transport
        .run_read(ReadCmd::Su(crate::cmd::SuRead::MagiskVersionCode {
            serial: serial.clone(),
        }))
        .await
    {
        if code.success_exit() {
            facts.magisk_code = code
                .stdout_text()
                .trim()
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok();
        }
    }
    if let Ok(label) = transport
        .run_read(ReadCmd::Su(crate::cmd::SuRead::MagiskVersion {
            serial: serial.clone(),
        }))
        .await
    {
        if label.success_exit() {
            let text = label.stdout_text();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                facts.magisk_label = Some(trimmed.to_string());
            }
        }
    }
    if facts.magisk_code.is_some() {
        facts.magisk_package = Some(crate::cmd::PackageName::magisk_app().as_str().to_string());
    }
    for (partition, slot) in [
        (crate::device::Partition::InitBoot, crate::device::Slot::A),
        (crate::device::Partition::InitBoot, crate::device::Slot::B),
        (crate::device::Partition::Boot, crate::device::Slot::A),
        (crate::device::Partition::Boot, crate::device::Slot::B),
    ] {
        let Ok(result) = transport
            .run_read(ReadCmd::Fastboot(FastbootRead::Getvar {
                serial: serial.clone(),
                var: FastbootVar::PartitionSize { partition, slot },
            }))
            .await
        else {
            continue;
        };
        if result.success_exit() {
            if let Some(size) = parse_size(&result.stdout_text()) {
                facts.partition_bytes = Some(facts.partition_bytes.unwrap_or(0).max(size));
            }
        }
    }
    if let Ok(battery) = transport
        .run_read(ReadCmd::AdbShell(AdbShellRead::DumpsysBattery {
            serial: serial.clone(),
        }))
        .await
    {
        if battery.success_exit() {
            note(&mut facts, "dumpsys-battery", &battery.stdout);
            facts.battery_percent = crate::device::parse_battery(&battery.stdout_text()).level;
        }
    }
    if let Ok(disk) = transport
        .run_read(ReadCmd::AdbShell(AdbShellRead::DumpsysDiskstats {
            serial: serial.clone(),
        }))
        .await
    {
        if disk.success_exit() {
            note(&mut facts, "dumpsys-diskstats", &disk.stdout);
            facts.device_free_bytes = parse_free_bytes(&disk.stdout_text());
        }
    }
    // A completed phone read is the evidence the host talked to this device.
    facts.driver_ok = true;
    Ok(facts)
}

fn note(facts: &mut SafetyFacts, source: &str, bytes: &[u8]) {
    facts.evidence.push(FactEvidence {
        source: source.to_string(),
        sha256: safety::sha256_hex(bytes),
    });
}

fn parse_free_bytes(text: &str) -> Option<u64> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("free_bytes:") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn host_free_bytes() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: `stat` is a valid out-pointer and the path is a nul-terminated literal.
        let rc = unsafe { libc::statvfs(c"/tmp".as_ptr(), stat.as_mut_ptr()) };
        if rc != 0 {
            return None;
        }
        // SAFETY: `statvfs` returned 0, so the struct was written.
        let stat = unsafe { stat.assume_init() };
        Some(stat.f_bavail.saturating_mul(stat.f_frsize))
    }
    #[cfg(windows)]
    {
        windows_free_bytes()
    }
}

#[cfg(windows)]
fn windows_free_bytes() -> Option<u64> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            available: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let dir: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        std::env::temp_dir()
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let mut available = 0u64;
    // SAFETY: `dir` is a nul-terminated wide path and `available` is a valid out-pointer.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            dir.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        None
    } else {
        Some(available)
    }
}

fn fill_csprng(buf: &mut [u8]) -> Result<(), CoreError> {
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut file = std::fs::File::open("/dev/urandom")
            .map_err(|err| rejected(format!("The plan nonce could not be drawn: {err}")))?;
        file.read_exact(buf)
            .map_err(|err| rejected(format!("The plan nonce could not be drawn: {err}")))?;
        Ok(())
    }
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        extern "system" {
            fn BCryptGenRandom(
                algorithm: *mut std::ffi::c_void,
                buffer: *mut u8,
                length: u32,
                flags: u32,
            ) -> i32;
        }
        // SAFETY: `buf` is a writable byte slice. Flag 0x2 is BCRYPT_USE_SYSTEM_PREFERRED_RNG.
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                u32::try_from(buf.len()).unwrap_or(u32::MAX),
                0x0000_0002,
            )
        };
        if status != 0 {
            return Err(rejected("The plan nonce could not be drawn."));
        }
        Ok(())
    }
}

pub fn plan_code(hash: &str) -> String {
    let hex = hash.trim_start_matches("flp1-");
    let head: String = hex.chars().take(8).collect();
    if head.len() >= 8 {
        format!("{}·{}", &head[..4], &head[4..8])
    } else {
        head
    }
}

fn write_ok(cmd: &WriteCmd, result: &crate::proc::RunResult) -> bool {
    if !result.success_exit() {
        return false;
    }
    let text = format!("{}{}", result.stdout_text(), result.stderr_text());
    let verdict = match cmd {
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash {
            partition, slot, ..
        }) => parse::parse_flash(&text, *partition, *slot),
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::SetActive { slot, .. }) => {
            parse::parse_set_active(&text, *slot)
        }
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Update { slot, .. }) => {
            parse::parse_update(&text, *slot, slot.other())
        }
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Reboot { .. }) => {
            parse::parse_fastboot_reboot(&text)
        }
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Reboot { .. }) => {
            parse::parse_adb_reboot(&text)
        }
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload { .. }) => {
            parse::parse_sideload(&text)
        }
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Push { .. }) => parse::parse_push(&text),
        WriteCmd::AdbShell(_) | WriteCmd::Su(_) => parse::parse_patch_script(&text),
    };
    matches!(verdict, Verdict::Ok | Verdict::Uncertain { .. })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::cmd::{AdbHostWrite, DeviceSerial, FastbootWrite, ImageRef, RebootMode};
    use crate::device::{Partition, Slot, TransportConfig};
    use crate::proc::{ScriptedResponse, ScriptedRunner};
    use crate::token::open_run_count;

    fn serial() -> DeviceSerial {
        DeviceSerial::try_from("pixel1").unwrap()
    }

    async fn gate() -> tokio::sync::MutexGuard<'static, ()> {
        crate::token::test_gate().await
    }

    fn session(runner: Arc<ScriptedRunner>, now: i64) -> WizardSession<ScriptedRunner> {
        let adb = if cfg!(windows) {
            PathBuf::from(r"C:\flashwright-test\adb.exe")
        } else {
            PathBuf::from("/opt/flashwright-test/adb")
        };
        let fastboot = if cfg!(windows) {
            PathBuf::from(r"C:\flashwright-test\fastboot.exe")
        } else {
            PathBuf::from("/opt/flashwright-test/fastboot")
        };
        let name = if cfg!(windows) { "adb.exe" } else { "adb" };
        runner.on(name, &["-s", "pixel1", "reboot"], ScriptedResponse::ok(""));
        let transport =
            PlatformToolsTransport::new(runner, adb, fastboot, TransportConfig::for_tests());
        WizardSession::with_clock(transport, Box::new(FixedClock(now)))
    }

    fn adb_bin() -> &'static str {
        if cfg!(windows) {
            "adb.exe"
        } else {
            "adb"
        }
    }

    fn fastboot_bin() -> &'static str {
        if cfg!(windows) {
            "fastboot.exe"
        } else {
            "fastboot"
        }
    }

    fn draft(dry_run: bool, _expires: i64) -> PlanRequest {
        PlanRequest {
            serial: "pixel1".into(),
            dry_run,
            steps: vec![PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Reboot {
                serial: serial(),
                mode: RebootMode::System,
            }))],
            firmware: FirmwareClaim::default(),
        }
    }

    fn komodo_props() -> String {
        [
            "[ro.product.device]: [komodo]",
            "[ro.boot.flash.locked]: [0]",
            "[persist.sys.update.pending]: [0]",
            "[ro.boot.slot_suffix]: [_a]",
            "[ro.build.version.security_patch]: [2026-02-01]",
            "[ro.build.id]: [TEST.260201.001]",
            "[ro.build.fingerprint]: [synthetic/komodo/test]",
            "[ro.build.date.utc]: [1700000000]",
            "[ro.build.version.sdk]: [34]",
            "[ro.bootloader]: [16.2-100]",
            "[ro.kernel.version]: [6.1.0-android14-synthetic]",
        ]
        .join("\n")
    }

    fn oriole_props() -> String {
        [
            "[ro.product.device]: [oriole]",
            "[ro.boot.flash.locked]: [0]",
            "[persist.sys.update.pending]: [0]",
            "[ro.boot.slot_suffix]: [_a]",
            "[ro.bootloader]: [15.3-13239611]",
        ]
        .join("\n")
    }

    fn script_phone(runner: &ScriptedRunner, phone: &str, props: String) {
        runner.on(
            adb_bin(),
            &["devices", "-l"],
            ScriptedResponse::ok(format!("List of devices attached\n{phone} device\n")),
        );
        runner.on_fn(adb_bin(), &["-s", phone, "shell"], move |call, _hit| {
            let remote = call.args.last().map(String::as_str).unwrap_or("");
            if remote.contains("-V") {
                ScriptedResponse::ok("30700\n")
            } else if remote.contains("magisk") {
                ScriptedResponse::ok("30.7\n")
            } else if remote.contains("battery") {
                ScriptedResponse::ok("level: 80\n")
            } else if remote.contains("diskstats") {
                ScriptedResponse::ok("free_bytes: 8000000000\n")
            } else if remote.contains("getprop") {
                ScriptedResponse::ok(props.clone())
            } else {
                ScriptedResponse::ok("")
            }
        });
    }

    fn script_partition_sizes(runner: &ScriptedRunner, phone: &str) {
        runner.on(
            fastboot_bin(),
            &["-s", phone, "getvar"],
            ScriptedResponse::ok("0x4000000\n"),
        );
    }

    fn temp_image() -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "flashwright-img-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("init_boot.img");
        std::fs::write(&path, b"synthetic-komodo-init-boot").unwrap();
        (dir, path)
    }

    fn reboot_calls(runner: &ScriptedRunner) -> usize {
        runner
            .calls()
            .iter()
            .filter(|call| call.args.last().is_some_and(|arg| arg == "reboot"))
            .count()
    }

    fn install_tools(transport: &PlatformToolsTransport<ScriptedRunner>) {
        use crate::exe::{platform_tool, ListenerImage};
        use sha2::{Digest, Sha256};
        let dir = std::env::temp_dir().join(format!(
            "flashwright-tools-{}-{}",
            std::process::id(),
            transport.config().command_timeout.as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let adb_path = dir.join(if cfg!(windows) { "adb.exe" } else { "adb" });
        let fastboot_path = dir.join(if cfg!(windows) {
            "fastboot.exe"
        } else {
            "fastboot"
        });
        std::fs::write(&adb_path, b"adb-bytes").unwrap();
        std::fs::write(&fastboot_path, b"fastboot-bytes").unwrap();
        let hash = |path: &std::path::Path| {
            let bytes = std::fs::read(path).unwrap();
            let digest = Sha256::digest(bytes);
            let mut hex = String::new();
            for byte in digest {
                hex.push_str(&format!("{byte:02x}"));
            }
            hex
        };
        let adb_hash = hash(&adb_path);
        let fastboot_hash = hash(&fastboot_path);
        let adb = platform_tool(&adb_path, &adb_hash).unwrap();
        let fastboot = platform_tool(&fastboot_path, &fastboot_hash).unwrap();
        transport.install_verified(
            adb,
            fastboot,
            Some(ListenerImage {
                path: adb_path,
                sha256: adb_hash,
            }),
        );
        transport.note_tools_verdict(true);
    }

    #[tokio::test]
    async fn dry_run_is_in_the_hash_and_mints_nothing() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let mut session = session(Arc::clone(&runner), 1_000);
        let preview = session.build_plan(draft(true, 5_000)).await.unwrap();
        assert_eq!(session.phase(), Phase::Review);
        assert_eq!(preview.expires_unix_ms, 1_000 + PLAN_TTL_MS);
        assert!(preview.plan_hash.starts_with("flp1-"));
        let real = hash_plan(
            &draft(false, 5_000),
            "same-nonce",
            5_000,
            None,
            &[],
            None,
            &[],
            &[],
        );
        let dry = hash_plan(
            &draft(true, 5_000),
            "same-nonce",
            5_000,
            None,
            &[],
            None,
            &[],
            &[],
        );
        assert_ne!(real.unwrap(), dry.unwrap());
        let refused = session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("does not write"));
        assert_eq!(session.phase(), Phase::Review);
        let before = open_run_count();
        let lines = session.dry_run(&preview.plan_hash).unwrap();
        assert!(lines.iter().any(|line| line.contains("G02")));
        assert!(lines.iter().any(|line| line.contains("G21")));
        assert!(lines.iter().any(|line| line.contains("G22")));
        assert!(lines.iter().all(|line| !line.starts_with("WOULD RUN")));
        assert_eq!(open_run_count(), before);
        assert!(runner.calls().is_empty());
        assert_eq!(session.phase(), Phase::Done);
        let again = session.dry_run(&preview.plan_hash).unwrap_err();
        assert!(again.to_string().contains("already used"));
        assert_eq!(open_run_count(), before);
    }

    #[tokio::test]
    async fn confirm_is_single_use_and_refused_outside_review() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let open = session;
        let mut session = open(Arc::clone(&runner), 1_000);
        let err = session.confirm_and_run("flp1-missing").await.unwrap_err();
        assert!(err.to_string().contains("review"));
        script_phone(&runner, "pixel1", komodo_props());
        let mut session = open(Arc::clone(&runner), 1_000);
        install_tools(session.transport());
        let preview = session.build_plan(draft(false, 5_000)).await.unwrap();
        let wrong = session
            .confirm_and_run("flp1-not-issued")
            .await
            .unwrap_err();
        assert!(wrong.to_string().contains("not issued"));
        assert_eq!(session.phase(), Phase::Review);
        let not_dry = session.dry_run(&preview.plan_hash).unwrap_err();
        assert!(not_dry.to_string().contains("not a dry run"));
        assert_eq!(session.phase(), Phase::Review);
        let report = session.confirm_and_run(&preview.plan_hash).await.unwrap();
        assert_eq!(report.lines.len(), 1);
        assert_eq!(reboot_calls(&runner), 1);
        assert_eq!(session.phase(), Phase::Done);
        let again = session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(again.to_string().contains("already used"));
        assert_eq!(reboot_calls(&runner), 1);
    }

    #[tokio::test]
    async fn expired_plan_is_refused() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let mut session = session(runner, 1_000);
        let preview = session.build_plan(draft(false, 1_500)).await.unwrap();
        session.clock = Box::new(FixedClock(1_000 + PLAN_TTL_MS + 1));
        let err = session.dry_run(&preview.plan_hash).unwrap_err();
        assert!(err.to_string().contains("expired"));
        let err = session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("expired"));
        assert_eq!(session.phase(), Phase::Review);
    }

    fn komodo_serial() -> DeviceSerial {
        DeviceSerial::try_from("synth-komodo-1").unwrap()
    }

    fn flash_draft(dry_run: bool, image: &str) -> PlanRequest {
        PlanRequest {
            serial: "synth-komodo-1".into(),
            dry_run,
            steps: vec![PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash {
                serial: komodo_serial(),
                slot: Slot::B,
                partition: Partition::InitBoot,
                image: ImageRef::new(1, image, 4096),
            }))],
            firmware: FirmwareClaim {
                codename: "komodo".into(),
                filename: "komodo-factory-synthetic.zip".into(),
                spl: Some("2026-02-01".into()),
                fingerprint: Some("synthetic/komodo/test".into()),
                timestamp: Some(1_700_000_000),
                bootloader: Some("16.2-100".into()),
                image_spl: Some("2026-02-01".into()),
                image_fingerprint: Some("synthetic/komodo/test".into()),
                images: Vec::new(),
            },
        }
    }

    #[tokio::test]
    async fn a_blocked_flash_dry_run_mints_nothing_and_writes_nothing() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let open = session;
        let mut session = open(Arc::clone(&runner), 1_000);
        let preview = session
            .build_plan(flash_draft(true, "/var/flashwright/missing.img"))
            .await
            .unwrap();
        let before = open_run_count();
        let lines = session.dry_run(&preview.plan_hash).unwrap();
        assert!(lines.iter().any(|line| line.starts_with("WOULD BLOCK:")));
        assert!(lines.iter().all(|line| !line.starts_with("WOULD RUN")));
        assert!(runner.calls().is_empty());
        assert_eq!(open_run_count(), before);
        assert_eq!(session.phase(), Phase::Done);
        let mut confirm_session = open(Arc::clone(&runner), 1_000);
        let preview = confirm_session
            .build_plan(flash_draft(false, "/var/flashwright/missing.img"))
            .await
            .unwrap();
        let err = confirm_session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Blocked:"));
        assert_eq!(confirm_session.phase(), Phase::Review);
        assert!(runner.calls().is_empty());
        assert_eq!(open_run_count(), before);
    }

    #[tokio::test]
    async fn verified_init_boot_lets_the_dry_run_pass() {
        let _gate = gate().await;
        let (dir, image) = temp_image();
        let runner = Arc::new(ScriptedRunner::new());
        script_phone(&runner, "synth-komodo-1", komodo_props());
        script_partition_sizes(&runner, "synth-komodo-1");
        let mut session = session(Arc::clone(&runner), 1_000);
        install_tools(session.transport());
        session.backup = Some(crate::safety::BackupState::Verified(
            crate::safety::BackupSet::bound(
                "set-komodo",
                "manifest",
                "synth-komodo-1",
                crate::device::Slot::B,
                Partition::InitBoot,
            ),
        ));
        let preview = session
            .build_plan(flash_draft(true, image.to_string_lossy().as_ref()))
            .await
            .unwrap();
        let before = open_run_count();
        let calls = runner.calls().len();
        let lines = session.dry_run(&preview.plan_hash).unwrap();
        assert!(
            lines.iter().all(|line| line.starts_with("WOULD RUN:")),
            "{lines:?}"
        );
        assert!(lines[0].contains("flash init_boot"));
        assert_eq!(runner.calls().len(), calls);
        assert_eq!(open_run_count(), before);
        assert_eq!(session.phase(), Phase::Done);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plan_hash_covers_facts_gates_backup_timeouts_and_nonce() {
        let draft = draft(false, 5_000);
        let base = hash_plan(&draft, "n1", 5_000, None, &[], None, &[], &[]).unwrap();
        let facts = FactSnap {
            device: "komodo".into(),
            firmware: "komodo".into(),
            filename: "komodo.zip".into(),
            authorised: 1,
            unlocked: Some(true),
            tools_verified: true,
            tools_match: true,
            adb_server_ok: true,
            partition_bytes: Some(4096),
            pending_ota: Some(false),
            device_spl: None,
            firmware_spl: None,
            magisk_code: None,
            kernel: None,
            evidence: vec![EvidenceSnap {
                source: "getprop".into(),
                sha256: "ab".repeat(32),
            }],
        };
        assert_ne!(
            base,
            hash_plan(&draft, "n1", 5_000, Some(&facts), &[], None, &[], &[]).unwrap()
        );
        let gates = vec![GateSnap {
            id: "G03".into(),
            severity: "block".into(),
            status: "fail".into(),
        }];
        assert_ne!(
            base,
            hash_plan(&draft, "n1", 5_000, None, &gates, None, &[], &[]).unwrap()
        );
        let backup = BackupSnap {
            set_id: "set".into(),
            manifest_sha256: "ab".repeat(32),
            serial_sha256: "cd".repeat(32),
            slot: "b".into(),
            partition: "init_boot".into(),
        };
        assert_ne!(
            base,
            hash_plan(&draft, "n1", 5_000, None, &[], Some(&backup), &[], &[]).unwrap()
        );
        let timeouts = vec![TimeoutSnap {
            timeout_s: 90,
            watchdog_s: 60,
        }];
        assert_ne!(
            base,
            hash_plan(&draft, "n1", 5_000, None, &[], None, &timeouts, &[]).unwrap()
        );
        assert_ne!(
            base,
            hash_plan(&draft, "n2", 5_000, None, &[], None, &[], &[]).unwrap()
        );
        assert_ne!(
            base,
            hash_plan(&draft, "n1", 5_000, None, &[], None, &[], &["G19".into()]).unwrap()
        );
    }

    #[tokio::test]
    async fn phone_reads_fill_gate_facts_and_a_later_read_blocks_the_step() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        script_phone(&runner, "pixel1", komodo_props());
        let open = session;
        let mut session = open(Arc::clone(&runner), 1_000);
        let preview = session.build_plan(draft(true, 5_000)).await.unwrap();
        let lines = session.dry_run(&preview.plan_hash).unwrap();
        assert!(lines.iter().all(|line| !line.contains("G02 ")), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("G21")));

        let (dir, image) = temp_image();
        script_phone(&runner, "synth-komodo-1", komodo_props());
        script_partition_sizes(&runner, "synth-komodo-1");
        runner.on(
            fastboot_bin(),
            &["-s", "synth-komodo-1", "getvar", "unlocked"],
            ScriptedResponse::ok("unlocked: no\n"),
        );
        let mut session = open(Arc::clone(&runner), 1_000);
        session.backup = Some(crate::safety::BackupState::Verified(
            crate::safety::BackupSet::bound(
                "set-komodo",
                "manifest",
                "synth-komodo-1",
                Slot::B,
                Partition::InitBoot,
            ),
        ));
        install_tools(session.transport());
        let preview = session
            .build_plan(flash_draft(false, image.to_string_lossy().as_ref()))
            .await
            .unwrap();
        let before = open_run_count();
        let err = session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("G03"), "{err}");
        assert!(runner
            .calls()
            .iter()
            .all(|call| !call.args.iter().any(|arg| arg == "flash")));
        assert_eq!(open_run_count(), before);
        assert_eq!(session.phase(), Phase::Recovery);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn acknowledge_changes_the_hash_and_cleanup_still_checks_gates() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        script_phone(&runner, "pixel1", oriole_props());
        let open = session;
        let mut session = open(Arc::clone(&runner), 1_000);
        install_tools(session.transport());
        let preview = session.build_plan(draft(false, 5_000)).await.unwrap();
        let acked = session.acknowledge(&preview.plan_hash, "G19").unwrap();
        assert_ne!(preview.plan_hash, acked.plan_hash);
        let refused = session.acknowledge(&acked.plan_hash, "G20").unwrap_err();
        assert!(refused.to_string().contains("cannot be acknowledged"));

        let mut blocked = open(runner, 1_000);
        let preview = blocked
            .build_plan(flash_draft(false, "/var/flashwright/missing.img"))
            .await
            .unwrap();
        let before = open_run_count();
        let err = blocked
            .confirm_and_run_finally(&preview.plan_hash, 0)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Blocked:"));
        assert_eq!(blocked.phase(), Phase::Review);
        assert_eq!(open_run_count(), before);
    }

    #[tokio::test]
    async fn cleanup_tail_runs_after_an_earlier_write_fails() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        script_phone(&runner, "pixel1", komodo_props());
        let mut session = session(Arc::clone(&runner), 1_000);
        runner.on(
            adb_bin(),
            &["-s", "pixel1", "reboot", "bootloader"],
            ScriptedResponse::ok(""),
        );
        runner.on(
            adb_bin(),
            &["-s", "pixel1", "reboot"],
            ScriptedResponse::fail(1, "FAILED"),
        );
        install_tools(session.transport());
        let mut plan = draft(false, 5_000);
        plan.steps
            .push(PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Reboot {
                serial: serial(),
                mode: RebootMode::Bootloader,
            })));
        let preview = session.build_plan(plan).await.unwrap();
        let err = session
            .confirm_and_run_finally(&preview.plan_hash, 1)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("write step failed"));
        assert!(runner
            .calls()
            .iter()
            .any(|call| call.args.iter().any(|arg| arg == "bootloader")));
        assert_eq!(session.phase(), Phase::Recovery);
    }

    #[test]
    fn a_verified_file_replaces_the_declared_image_size() {
        let dir = std::env::temp_dir().join(format!("flashwright-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("init_boot.img");
        std::fs::write(&path, vec![9u8; 32]).unwrap();
        let mut steps = vec![PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash {
            serial: komodo_serial(),
            slot: Slot::B,
            partition: Partition::InitBoot,
            image: ImageRef::new(1, path.to_string_lossy().as_ref(), 4),
        }))];
        seal_image_sizes(&mut steps).unwrap();
        let PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash { image, .. })) = &steps[0]
        else {
            panic!("flash");
        };
        assert_eq!(image.size_bytes(), 32);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
