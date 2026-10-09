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
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::cmd::{ReadCmd, WriteCmd};
use crate::device::PlatformToolsTransport;
use crate::parse::{self, Verdict};
use crate::proc::CommandRunner;
use crate::token::mint_confirmed;
use crate::CoreError;

pub const PLAN_SCHEMA: &str = "flashwright.plan.v1";

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanDraft {
    pub serial: String,
    pub dry_run: bool,
    pub expires_unix_ms: i64,
    pub steps: Vec<PlanStep>,
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
    consumed: BTreeSet<String>,
    transport: PlatformToolsTransport<R>,
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
            consumed: BTreeSet::new(),
            transport,
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
        check_serial(&draft)?;
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

    /// Same expiry and step checks as confirm. Mints no token and leaves the plan.
    pub fn dry_run(&self, plan_hash_value: &str) -> Result<Vec<String>, CoreError> {
        let held = self.ready(plan_hash_value)?;
        let mut lines = Vec::new();
        for step in &held.steps {
            if let PlanStep::Write(cmd) = step {
                let rendered =
                    crate::cmd::write_argv(cmd).map_err(|err| rejected(err.to_string()))?;
                lines.push(format!("WOULD RUN: {}", rendered.args.join(" ")));
            }
        }
        Ok(lines)
    }

    /// Run the reviewed plan once. The token never leaves this function.
    ///
    /// A plan built with `dry_run` is refused here, so this path mints a token
    /// only for a plan that was not a dry run.
    pub async fn confirm_and_run(&mut self, plan_hash_value: &str) -> Result<RunReport, CoreError> {
        let dry_run = self.ready(plan_hash_value)?.dry_run;
        if dry_run {
            return Err(CoreError::DryRunPlan);
        }
        let held = self
            .held
            .take()
            .expect("review plan");
        // Mark the plan used before step 1. A second call returns AlreadyUsed.
        self.consumed.insert(held.hash.clone());
        self.phase = Phase::Flash;
        let writes: Vec<WriteCmd> = held
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::Write(cmd) => Some(cmd.clone()),
                PlanStep::Read(_) => None,
            })
            .collect();
        let (plan, token) = mint_confirmed(&held.hash, &held.serial, &writes);
        self.transport.arm(&plan);
        let mut lines = Vec::new();
        for step in &held.steps {
            match step {
                PlanStep::Read(cmd) => {
                    let result = self.transport.run_read(cmd.clone()).await?;
                    if !result.success_exit() {
                        self.phase = Phase::Recovery;
                        return Err(rejected("A read step failed."));
                    }
                    lines.push(result.stdout_text());
                }
                PlanStep::Write(cmd) => {
                    let result = self.transport.run_write(&token, cmd.clone()).await;
                    match result {
                        Ok(result) if write_ok(cmd, &result) => {
                            lines.push(result.stdout_text());
                        }
                        Ok(_) | Err(_) => {
                            self.phase = Phase::Recovery;
                            return Err(rejected("A write step failed."));
                        }
                    }
                }
            }
        }
        drop(token);
        self.phase = Phase::Done;
        Ok(RunReport { lines })
    }
}

impl<R: CommandRunner> WizardSession<R> {
    fn ready(&self, plan_hash_value: &str) -> Result<&HeldPlan, CoreError> {
        if self.consumed.contains(plan_hash_value) {
            return Err(CoreError::AlreadyUsed);
        }
        if self.phase != Phase::Review {
            return Err(if matches!(self.phase, Phase::Done | Phase::Flash | Phase::Recovery) {
                CoreError::AlreadyUsed
            } else {
                CoreError::WrongState
            });
        }
        let Some(held) = self.held.as_ref() else {
            return Err(CoreError::AlreadyUsed);
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

fn check_serial(draft: &PlanDraft) -> Result<(), CoreError> {
    for step in &draft.steps {
        let serial = match step {
            PlanStep::Read(cmd) => cmd.serial().map(|serial| serial.as_str()),
            PlanStep::Write(cmd) => Some(cmd.serial().as_str()),
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
        WriteCmd::AdbShell(_) | WriteCmd::Su(_) => parse::parse_patch_script(&text),
    };
    matches!(verdict, Verdict::Ok | Verdict::Uncertain { .. })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::cmd::{AdbHostWrite, DeviceSerial, RebootMode};
    use crate::device::TransportConfig;
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
}
