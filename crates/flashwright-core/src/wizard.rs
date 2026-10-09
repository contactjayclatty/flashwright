// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Wizard-facing session.
//!
//! A later change moves the full window state machine into this module.
//! This seam is the public read, plan, and confirm API that window will call.
//! The public write entry points are [`WizardSession::confirm_and_run`] and
//! [`WizardSession::confirm_and_run_finally`]. Each mints a write token
//! internally. A second call with the same plan fails because the plan has
//! been consumed. Cleanup is a hashed plan step that core appends and always
//! runs. It is not a write, and the caller does not choose how many steps it is.

use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::cmd::{AdbShellWrite, CleanupCmd, DeviceSerial, ReadCmd, WriteCmd};
use crate::device::{PlatformToolsTransport, Slot};
use crate::parse::{self, Verdict};
use crate::proc::CommandRunner;
use crate::safety::{self, BackupState, FactoryInitBoot, InitBootRecord, SafetyFacts};
use crate::token::mint_confirmed;
use crate::CoreError;

pub const PLAN_SCHEMA: &str = "flashwright.plan.v1";

/// Window phases. Only [`Phase::Review`] may confirm or dry-run a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// One catalogue step. Reads, writes, and cleanup are all part of the plan hash.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlanStep {
    Read(ReadCmd),
    Write(WriteCmd),
    Cleanup(CleanupCmd),
}

/// Stock image sealed into the plan hash before confirm.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImageSeal {
    pub role: String,
    pub sha1: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanDraft {
    pub serial: String,
    pub dry_run: bool,
    pub expires_unix_ms: i64,
    pub steps: Vec<PlanStep>,
    pub images: Vec<ImageSeal>,
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

#[derive(serde::Serialize)]
struct PlanBody<'a> {
    schema: &'static str,
    serial: &'a str,
    dry_run: bool,
    expires_unix_ms: i64,
    steps: &'a [PlanStep],
    images: &'a [ImageSeal],
}

struct HeldPlan {
    hash: String,
    expires_unix_ms: i64,
    dry_run: bool,
    steps: Vec<PlanStep>,
    serial: String,
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
        }
    }

    /// Facts the safety gates read. Image writes without facts are refused.
    pub fn set_safety(&mut self, facts: SafetyFacts) {
        self.safety = Some(facts);
    }

    /// Read stock init_boot, store its SHA-256, and compare it with the factory image.
    ///
    /// A mismatch is kept as a block. A later dry run or confirm refuses the plan.
    pub async fn backup_stock_init_boot(
        &mut self,
        serial: &DeviceSerial,
        slot: Slot,
        expected_codename: &str,
        factory: &dyn FactoryInitBoot,
    ) -> Result<InitBootRecord, CoreError> {
        match safety::pull_stock_init_boot(
            &self.transport,
            serial,
            slot,
            expected_codename,
            factory,
        )
        .await
        {
            Ok(record) => {
                self.backup = Some(BackupState::Verified(record.clone()));
                Ok(record)
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

    /// Store a plan and move to review. `dry_run` is part of the plan hash.
    pub fn build_plan(&mut self, draft: PlanDraft) -> Result<PlanPreview, CoreError> {
        if self.phase == Phase::Flash {
            return Err(rejected("A job is already running."));
        }
        if self.clock.unix_ms() > draft.expires_unix_ms {
            return Err(rejected("The plan has expired. Build it again."));
        }
        let PlanDraft {
            mut steps,
            serial,
            dry_run,
            expires_unix_ms,
            images,
        } = draft;
        ensure_cleanup(&mut steps)?;
        let draft = PlanDraft {
            serial,
            dry_run,
            expires_unix_ms,
            steps,
            images,
        };
        check_serial(&draft)?;
        check_images(&draft.images)?;
        let views = step_views(&draft.steps)?;
        let hash = plan_hash(&draft)?;
        let preview = PlanPreview {
            plan_code: plan_code(&hash),
            plan_hash: hash.clone(),
            expires_unix_ms: draft.expires_unix_ms,
            dry_run: draft.dry_run,
            steps: views,
        };
        self.held = Some(HeldPlan {
            hash,
            expires_unix_ms: draft.expires_unix_ms,
            dry_run: draft.dry_run,
            steps: draft.steps,
            serial: draft.serial,
        });
        self.phase = Phase::Review;
        Ok(preview)
    }

    /// Evaluate every safety gate. Mints no token, spawns nothing, and leaves the plan.
    ///
    /// A passing plan prints `WOULD RUN` for each write. A failing gate prints
    /// `WOULD BLOCK` and the reason, and no write line.
    pub fn dry_run(&self, plan_hash_value: &str) -> Result<Vec<String>, CoreError> {
        let held = self.ready(plan_hash_value)?;
        let decisions = safety::evaluate(&held.steps, self.safety.as_ref(), self.backup.as_ref());
        Ok(safety::dry_run_lines(&held.steps, &decisions))
    }

    /// Run the reviewed plan once. The token never leaves this function.
    ///
    /// A plan built with `dry_run` is refused here, so this path mints a token
    /// only for a plan that was not a dry run.
    pub async fn confirm_and_run(&mut self, plan_hash_value: &str) -> Result<RunReport, CoreError> {
        let (dry_run, steps) = {
            let held = self.ready(plan_hash_value)?;
            (held.dry_run, held.steps.clone())
        };
        if dry_run {
            return Err(rejected("A dry-run plan does not write."));
        }
        let decisions = safety::evaluate(&steps, self.safety.as_ref(), self.backup.as_ref());
        if decisions.iter().any(|gate| gate.blocked) {
            let reason = decisions
                .iter()
                .filter(|gate| gate.blocked)
                .map(|gate| format!("{} {}", gate.id, gate.reason))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(rejected(format!("Blocked: {reason}")));
        }
        let held = self.held.take().expect("review plan");
        self.execute(held).await
    }

    /// Run a confirmed patch plan. Cleanup is the hashed suffix and always runs.
    ///
    /// The caller does not pass a cleanup count. Flash safety gates stay on
    /// [`Self::confirm_and_run`].
    pub async fn confirm_and_run_finally(
        &mut self,
        plan_hash_value: &str,
    ) -> Result<RunReport, CoreError> {
        let dry_run = self.ready(plan_hash_value)?.dry_run;
        if dry_run {
            return Err(rejected("A dry-run plan does not write."));
        }
        let held = self.held.take().expect("review plan");
        self.execute(held).await
    }

    async fn execute(&mut self, held: HeldPlan) -> Result<RunReport, CoreError> {
        self.phase = Phase::Flash;
        let split = cleanup_index(&held.steps);
        let writes: Vec<WriteCmd> = held.steps[..split]
            .iter()
            .filter_map(|step| match step {
                PlanStep::Write(cmd) => Some(cmd.clone()),
                PlanStep::Read(_) | PlanStep::Cleanup(_) => None,
            })
            .collect();
        let (plan, token) = mint_confirmed(&held.hash, &held.serial, &writes);
        self.transport.arm(&plan);
        let mut lines = Vec::new();
        let mut failed: Option<CoreError> = None;
        for step in &held.steps[..split] {
            match run_planned(&self.transport, &token, step).await {
                Ok(text) => lines.push(text),
                Err(err) => {
                    failed = Some(err);
                    break;
                }
            }
        }
        drop(token);
        for step in &held.steps[split..] {
            let PlanStep::Cleanup(cmd) = step else {
                if failed.is_none() {
                    failed = Some(rejected("Cleanup steps cannot perform writes."));
                }
                continue;
            };
            match self.transport.run_cleanup(cmd).await {
                Ok(result) if cleanup_ok(&result) => lines.push(result.stdout_text()),
                Ok(_) | Err(_) => {
                    if failed.is_none() {
                        failed = Some(rejected("Cleanup failed."));
                    }
                }
            }
        }
        if let Some(err) = failed {
            self.phase = Phase::Recovery;
            return Err(err);
        }
        self.phase = Phase::Done;
        Ok(RunReport { lines })
    }
}

async fn run_planned<R: CommandRunner>(
    transport: &PlatformToolsTransport<R>,
    token: &crate::token::WriteToken,
    step: &PlanStep,
) -> Result<String, CoreError> {
    match step {
        PlanStep::Read(cmd) => {
            let result = transport.run_read(cmd.clone()).await?;
            if !result.success_exit() {
                return Err(rejected("A read step failed."));
            }
            Ok(result.stdout_text())
        }
        PlanStep::Write(cmd) => {
            let result = transport.run_write(token, cmd.clone()).await;
            match result {
                Ok(result) if write_ok(cmd, &result) => Ok(result.stdout_text()),
                Ok(_) | Err(_) => Err(rejected("A write step failed.")),
            }
        }
        PlanStep::Cleanup(_) => Err(rejected("Cleanup steps cannot perform writes.")),
    }
}

impl<R: CommandRunner> WizardSession<R> {
    fn ready(&self, plan_hash_value: &str) -> Result<&HeldPlan, CoreError> {
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
            return Err(rejected("The plan has expired. Build it again."));
        }
        step_views(&held.steps)?;
        Ok(held)
    }
}

fn rejected(reason: impl Into<String>) -> CoreError {
    CoreError::Rejected {
        reason: reason.into(),
    }
}

fn ensure_cleanup(steps: &mut Vec<PlanStep>) -> Result<(), CoreError> {
    if let Some(index) = steps
        .iter()
        .position(|step| matches!(step, PlanStep::Cleanup(_)))
    {
        if steps[index..]
            .iter()
            .any(|step| !matches!(step, PlanStep::Cleanup(_)))
        {
            return Err(rejected("Cleanup steps cannot perform writes."));
        }
    }
    let work_serial = steps.iter().find_map(|step| match step {
        PlanStep::Write(WriteCmd::AdbShell(AdbShellWrite::MakeWorkDir { serial })) => {
            Some(serial.clone())
        }
        _ => None,
    });
    let Some(serial) = work_serial else {
        return Ok(());
    };
    let expected = PlanStep::Cleanup(CleanupCmd::RemoveWorkDir { serial });
    match steps.last() {
        Some(step) if step == &expected => {}
        Some(PlanStep::Cleanup(_)) => {
            return Err(rejected("Cleanup does not match the work directory."));
        }
        _ => steps.push(expected),
    }
    let count = steps
        .iter()
        .filter(|step| matches!(step, PlanStep::Cleanup(_)))
        .count();
    if count != 1 {
        return Err(rejected("The plan has more than one cleanup step."));
    }
    Ok(())
}

fn cleanup_index(steps: &[PlanStep]) -> usize {
    steps
        .iter()
        .position(|step| matches!(step, PlanStep::Cleanup(_)))
        .unwrap_or(steps.len())
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
            Verdict::Ok | Verdict::Uncertain { .. }
        )
}

fn check_serial(draft: &PlanDraft) -> Result<(), CoreError> {
    for step in &draft.steps {
        let serial = match step {
            PlanStep::Read(cmd) => cmd.serial().map(|serial| serial.as_str()),
            PlanStep::Write(cmd) => Some(cmd.serial().as_str()),
            PlanStep::Cleanup(cmd) => Some(cmd.serial().as_str()),
        };
        if let Some(serial) = serial {
            if serial != draft.serial {
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

pub fn plan_hash(draft: &PlanDraft) -> Result<String, CoreError> {
    let body = PlanBody {
        schema: PLAN_SCHEMA,
        serial: &draft.serial,
        dry_run: draft.dry_run,
        expires_unix_ms: draft.expires_unix_ms,
        steps: &draft.steps,
        images: &draft.images,
    };
    let bytes = serde_jcs::to_vec(&body).map_err(|err| rejected(err.to_string()))?;
    let digest = Sha256::digest(bytes);
    let mut hash = String::from("flp1-");
    for byte in digest {
        hash.push_str(&format!("{byte:02x}"));
    }
    Ok(hash)
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
        WriteCmd::AdbShell(crate::cmd::AdbShellWrite::RunPatchScript { .. }) | WriteCmd::Su(_) => {
            parse::parse_patch_script(&text)
        }
        WriteCmd::AdbShell(_) => parse::parse_fixed_shell(&text),
    };
    matches!(verdict, Verdict::Ok | Verdict::Uncertain { .. })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::cmd::{
        AdbHostWrite, AdbShellWrite, CleanupCmd, DeviceSerial, FastbootWrite, ImageRef, RebootMode,
    };
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

    fn draft(dry_run: bool, expires: i64) -> PlanDraft {
        PlanDraft {
            serial: "pixel1".into(),
            dry_run,
            expires_unix_ms: expires,
            steps: vec![PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Reboot {
                serial: serial(),
                mode: RebootMode::System,
            }))],
            images: Vec::new(),
        }
    }

    #[tokio::test]
    async fn dry_run_is_in_the_hash_and_mints_nothing() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let mut session = session(Arc::clone(&runner), 1_000);
        let preview = session.build_plan(draft(true, 5_000)).unwrap();
        assert_eq!(session.phase(), Phase::Review);
        assert!(preview.plan_hash.starts_with("flp1-"));
        let other = plan_hash(&draft(false, 5_000)).unwrap();
        assert_ne!(preview.plan_hash, other);
        let before = open_run_count();
        let lines = session.dry_run(&preview.plan_hash).unwrap();
        assert_eq!(lines, vec!["WOULD RUN: -s pixel1 reboot".to_string()]);
        assert_eq!(open_run_count(), before);
        assert!(runner.calls().is_empty());
        assert_eq!(session.phase(), Phase::Review);
        assert!(session.dry_run("flp1-deadbeef").is_err());
        let err = session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("dry-run"));
        assert!(runner.calls().is_empty());
        assert_eq!(open_run_count(), before);
        assert_eq!(session.phase(), Phase::Review);
    }

    #[tokio::test]
    async fn confirm_is_single_use_and_refused_outside_review() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let mut session = session(Arc::clone(&runner), 1_000);
        let err = session.confirm_and_run("flp1-missing").await.unwrap_err();
        assert!(err.to_string().contains("review"));
        let preview = session.build_plan(draft(false, 5_000)).unwrap();
        session.dry_run(&preview.plan_hash).unwrap();
        let report = session.confirm_and_run(&preview.plan_hash).await.unwrap();
        assert!(report.lines.len() == 1);
        assert_eq!(session.phase(), Phase::Done);
        assert_eq!(runner.calls().len(), 1);
        let again = session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(again.to_string().contains("already used"));
        assert_eq!(runner.calls().len(), 1);
    }

    #[tokio::test]
    async fn expired_plan_is_refused() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let mut session = session(runner, 1_000);
        let preview = session.build_plan(draft(false, 1_500)).unwrap();
        session.clock = Box::new(FixedClock(1_501));
        let err = session.dry_run(&preview.plan_hash).unwrap_err();
        assert!(err.to_string().contains("expired"));
        let err = session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("expired"));
        assert_eq!(session.phase(), Phase::Review);
    }

    #[tokio::test]
    async fn cleanup_is_fixed_by_core_and_always_runs() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let name = if cfg!(windows) { "adb.exe" } else { "adb" };
        runner.on_fn(name, &["-s", "pixel1", "shell"], |invocation, _| {
            let remote = invocation.args.get(3).map(String::as_str).unwrap_or("");
            if remote.contains("'rm'") {
                ScriptedResponse::ok("")
            } else {
                ScriptedResponse::fail(1, "mkdir failed\n")
            }
        });
        let mut session = session(Arc::clone(&runner), 1_000);
        let bare = PlanDraft {
            serial: "pixel1".into(),
            dry_run: false,
            expires_unix_ms: 5_000,
            steps: vec![PlanStep::Write(WriteCmd::AdbShell(
                AdbShellWrite::MakeWorkDir { serial: serial() },
            ))],
            images: Vec::new(),
        };
        let mut explicit = bare.clone();
        explicit
            .steps
            .push(PlanStep::Cleanup(CleanupCmd::RemoveWorkDir {
                serial: serial(),
            }));
        let preview = session.build_plan(bare).unwrap();
        assert_eq!(preview.plan_hash, plan_hash(&explicit).unwrap());
        assert_eq!(preview.steps.last().unwrap().class, "cleanup");
        let err = session
            .confirm_and_run_finally(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("write step"));
        assert!(runner
            .calls()
            .iter()
            .any(|call| { call.args.iter().any(|arg| arg.contains("'rm'")) }));
        let mut bad = explicit.clone();
        bad.steps
            .push(PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Reboot {
                serial: serial(),
                mode: RebootMode::System,
            })));
        let mut rejected = session;
        let err = rejected.build_plan(bad).unwrap_err();
        assert!(err.to_string().contains("cannot perform writes"));
    }

    fn komodo_serial() -> DeviceSerial {
        DeviceSerial::try_from("synth-komodo-1").unwrap()
    }

    fn flash_draft(expires: i64) -> PlanDraft {
        PlanDraft {
            serial: "synth-komodo-1".into(),
            dry_run: false,
            expires_unix_ms: expires,
            steps: vec![PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash {
                serial: komodo_serial(),
                slot: Slot::B,
                partition: Partition::InitBoot,
                image: ImageRef::new(1, "/var/flashwright/init_boot.img", 4096),
            }))],
            images: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_blocked_flash_dry_run_mints_nothing_and_writes_nothing() {
        let _gate = gate().await;
        let runner = Arc::new(ScriptedRunner::new());
        let mut session = session(Arc::clone(&runner), 1_000);
        let preview = session.build_plan(flash_draft(5_000)).unwrap();
        let before = open_run_count();
        let lines = session.dry_run(&preview.plan_hash).unwrap();
        assert!(lines.iter().any(|line| line.starts_with("WOULD BLOCK:")));
        assert!(lines.iter().all(|line| !line.starts_with("WOULD RUN")));
        assert!(runner.calls().is_empty());
        assert_eq!(open_run_count(), before);
        let err = session
            .confirm_and_run(&preview.plan_hash)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Blocked:"));
        assert_eq!(session.phase(), Phase::Review);
        assert!(runner.calls().is_empty());
        assert_eq!(open_run_count(), before);
    }

    #[tokio::test]
    async fn verified_init_boot_lets_the_dry_run_pass() {
        let _gate = gate().await;
        let stock = b"synthetic-komodo-init-boot".to_vec();
        let runner = Arc::new(ScriptedRunner::new());
        let seen = stock.clone();
        let name = if cfg!(windows) { "adb.exe" } else { "adb" };
        runner.on_fn(
            name,
            &["-s", "synth-komodo-1", "exec-out"],
            move |_call, _hit| ScriptedResponse::ok(seen.clone()),
        );
        let mut session = session(Arc::clone(&runner), 1_000);
        session.set_safety(crate::safety::SafetyFacts::komodo_ready());
        let factory = crate::safety::BytesInitBoot {
            codename: "komodo".into(),
            bytes: stock.clone(),
        };
        let record = session
            .backup_stock_init_boot(&komodo_serial(), crate::device::Slot::A, "komodo", &factory)
            .await
            .unwrap();
        assert_eq!(record.sha256.len(), 64);
        assert_eq!(runner.calls().len(), 2);
        let preview = session.build_plan(flash_draft(5_000)).unwrap();
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
        assert_eq!(session.phase(), Phase::Review);
    }
}
