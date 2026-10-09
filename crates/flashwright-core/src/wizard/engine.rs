// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::collections::{BTreeMap, BTreeSet};

use flashwright_plan::{canonical_hash, plan_label};
use uuid::Uuid;

use super::device::{
    inactive_slot, mode_label, DeviceTransport, EmptyTransport, Mode, Partition, Slot,
};
use super::gate::{self, Check, Life};
use super::model::{
    BackupSet, BurstStats, ChoiceView, DriverStatus, ExternalLink, FirmwareRef, FirmwareReport,
    JobView, LogLine, Notice, PlanKind, RecoveryOption, Snapshot, ToolsStatus, WizardEvent,
    WizardPlan,
};
#[cfg(any(test, feature = "mock", debug_assertions))]
use super::session::FixedClock;
use super::session::{Clock, Phase, SystemClock};
use super::steps::{
    assert_no_forbidden_args, factory_steps, ota_steps, prepare_patch_steps, quote_argv, GateView,
    Route, Step, StepClass, SCHEMA,
};
use crate::cmd::{DeviceSerial, FastbootWrite, ImageRef, WriteCmd};
use crate::safety::{self, GateDecision};
use crate::wizard::PlanStep;
use crate::CoreError;

const FIFTEEN_MIN_MS: i64 = 15 * 60 * 1000;
const BACKUP_SET_ID: &str = "4f2a9c01-0000-7000-8000-000000000001";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WizardState {
    Connect,
    Choose,
    PickFirmware,
    Review(PlanKind),
    Patching,
    DryRunDone,
    Flash,
    Done,
    Recovery,
}

#[derive(Debug, serde::Serialize)]
struct PlanBody {
    schema: &'static str,
    plan_id: Uuid,
    nonce_hex: String,
    created_unix_ms: i64,
    expires_unix_ms: i64,
    serial: String,
    codename: String,
    fingerprint: String,
    spl: String,
    active_slot: Slot,
    target_slot: Slot,
    bootloader_unlocked: bool,
    route: Route,
    firmware_name: String,
    firmware_sha256: String,
    target_partition: Partition,
    backup_set_id: String,
    dry_run: bool,
    after_dry_run: Option<String>,
    kind: PlanKind,
    gates: Vec<GateView>,
    steps: Vec<Step>,
}

struct Pending {
    body: PlanBody,
    preview: WizardPlan,
    life: Life,
}

pub struct Engine<T: DeviceTransport> {
    transport: T,
    clock: Box<dyn Clock>,
    state: WizardState,
    selected: Option<String>,
    route: Route,
    prefer_dry_run: bool,
    action_chosen: bool,
    firmware: Option<FirmwareReport>,
    pending: Option<Pending>,
    consumed: BTreeSet<String>,
    job: JobView,
    notice: Option<Notice>,
    nonce_counter: u64,
    running: bool,
    picked: BTreeMap<String, (String, u64)>,
    exe_ok: bool,
    server_ok: bool,
    partition_ok: bool,
    step_bootloader_ok: bool,
    stop_after_steps: Option<u32>,
}

impl Engine<EmptyTransport> {
    pub fn empty() -> Self {
        Self::new(EmptyTransport, Box::new(SystemClock))
    }
}

#[cfg(any(test, feature = "mock", debug_assertions))]
impl Engine<crate::mock::MockTransport> {
    pub fn mock() -> Self {
        Self::new(crate::mock::MockTransport::default(), Box::new(SystemClock))
    }

    pub fn mock_at(unix_ms: i64) -> Self {
        Self::new(
            crate::mock::MockTransport::default(),
            Box::new(FixedClock(unix_ms)),
        )
    }
}

impl<T: DeviceTransport> Engine<T> {
    pub fn new(transport: T, clock: Box<dyn Clock>) -> Self {
        Self {
            transport,
            clock,
            state: WizardState::Connect,
            selected: None,
            route: Route::Ota,
            prefer_dry_run: true,
            action_chosen: false,
            firmware: None,
            pending: None,
            consumed: BTreeSet::new(),
            job: JobView::default(),
            notice: None,
            nonce_counter: 0,
            running: false,
            picked: BTreeMap::new(),
            exe_ok: true,
            server_ok: true,
            partition_ok: true,
            step_bootloader_ok: true,
            stop_after_steps: None,
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn set_clock(&mut self, clock: Box<dyn Clock>) {
        self.clock = clock;
    }

    /// Test hook: stop the executor once this many steps have finished.
    pub fn arm_stop_after(&mut self, completed_steps: u32) {
        self.stop_after_steps = Some(completed_steps);
    }

    pub fn set_write_gates(&mut self, exe_ok: bool, server_ok: bool, partition_ok: bool) {
        self.exe_ok = exe_ok;
        self.server_ok = server_ok;
        self.partition_ok = partition_ok;
    }

    /// Step-time bootloader check. The phone identity stays the same, so the
    /// plan is not discarded; a write step reports WOULD BLOCK G03 instead.
    pub fn set_step_bootloader(&mut self, ok: bool) {
        self.step_bootloader_ok = ok;
    }

    pub fn snapshot(&self) -> Result<Snapshot, CoreError> {
        let devices = self.transport.list()?;
        let selected = match &self.selected {
            Some(serial) => Some(self.transport.info(serial)?),
            None => None,
        };
        let (root_label, root_version) = if let Some(info) = &selected {
            if info.root_present {
                (
                    "Magisk app".to_string(),
                    format!("stable {}", info.root_tool_version),
                )
            } else {
                ("Magisk app".to_string(), "not detected".to_string())
            }
        } else {
            ("Magisk app".to_string(), "—".to_string())
        };
        let review_kind = match self.state {
            WizardState::Review(kind) => Some(kind),
            _ => None,
        };
        Ok(Snapshot {
            phase: visible_phase(self.state),
            review_kind,
            tools: ToolsStatus {
                version: "37.0.1".to_string(),
                classification: "allow".to_string(),
                message: "Platform tools 37.0.1 are on the allow list.".to_string(),
            },
            driver: DriverStatus {
                state: "ok".to_string(),
                message: "USB driver looks fine for the selected phone.".to_string(),
            },
            devices,
            selected,
            choice: ChoiceView {
                action: if self.action_chosen {
                    "update_keep_root".to_string()
                } else {
                    String::new()
                },
                route: self.route,
                prefer_dry_run: self.prefer_dry_run,
                slot_label: "Inactive slot (recommended)".to_string(),
                both_slots_enabled: false,
                both_slots_reason: "Writing both slots removes your fallback and is risky with anti-rollback. Coming later in Expert mode.".to_string(),
                root_tool_label: root_label,
                root_tool_version: root_version,
                backup_folder: "%LOCALAPPDATA%\\Flashwright\\backups".to_string(),
            },
            firmware: self.firmware.clone(),
            plan: self.pending.as_ref().and_then(|pending| {
                if pending.life == Life::Discarded {
                    None
                } else {
                    Some(pending.preview.clone())
                }
            }),
            job: self.job.clone(),
            backups: vec![BackupSet {
                set_id: BACKUP_SET_ID.to_string(),
                label: "harbor · slot A · HQ1A.MOCK.001".to_string(),
                size_label: "34 MiB".to_string(),
                verified: true,
            }],
            notice: self.notice.clone(),
            links: vec![
                ExternalLink { id: "platform_tools".to_string(), label: "Open the official platform-tools page".to_string() },
                ExternalLink { id: "usb_driver".to_string(), label: "Open the official USB driver page".to_string() },
                ExternalLink { id: "firmware_full".to_string(), label: "Open the official full-package page".to_string() },
                ExternalLink { id: "firmware_factory".to_string(), label: "Open the official factory-package page".to_string() },
            ],
        })
    }

    pub fn scan(&mut self) -> Result<Snapshot, CoreError> {
        self.notice = None;
        self.snapshot()
    }

    pub fn tools_status(&mut self) -> Result<Snapshot, CoreError> {
        self.snapshot()
    }

    pub fn note_tools_folder(&mut self, display: Option<String>) -> Result<Snapshot, CoreError> {
        self.notice = Some(Notice {
            level: "info".to_string(),
            message: match display {
                Some(name) => format!("Platform-tools folder selected ({name})."),
                None => "No platform-tools folder was selected.".to_string(),
            },
            gates: Vec::new(),
        });
        self.snapshot()
    }

    pub fn note_tools_zip(&mut self, display: Option<String>) -> Result<Snapshot, CoreError> {
        self.notice = Some(Notice {
            level: "info".to_string(),
            message: match display {
                Some(name) => format!("Platform-tools package selected ({name})."),
                None => "No platform-tools package was selected.".to_string(),
            },
            gates: Vec::new(),
        });
        self.snapshot()
    }

    pub fn select_device(&mut self, serial: &str) -> Result<Snapshot, CoreError> {
        let info = self.transport.info(serial)?;
        self.selected = Some(info.serial.clone());
        self.pending = None;
        self.firmware = None;
        self.notice = match info.mode {
            Mode::Adb => None,
            Mode::Unauthorized => Some(Notice {
                level: "warn".to_string(),
                message: "This phone is not authorised. Unlock it and allow this computer, then scan again.".to_string(),
                gates: Vec::new(),
            }),
            Mode::Offline => Some(Notice {
                level: "warn".to_string(),
                message: "This phone is offline. Try another cable or a USB 2.0 port, then scan again.".to_string(),
                gates: Vec::new(),
            }),
            other => Some(Notice {
                level: "warn".to_string(),
                message: format!("This phone is in {} and cannot start the update.", mode_label(other)),
                gates: Vec::new(),
            }),
        };
        self.snapshot()
    }

    pub fn continue_from_connect(&mut self) -> Result<Snapshot, CoreError> {
        let Some(serial) = self.selected.clone() else {
            self.notice = Some(block_notice("Select one phone first."));
            return self.snapshot();
        };
        let info = self.transport.info(&serial)?;
        if info.mode != Mode::Adb {
            self.notice = Some(block_notice(
                "The phone must be authorised and in the normal device state.",
            ));
            return self.snapshot();
        }
        self.state = WizardState::Choose;
        self.notice = None;
        self.snapshot()
    }

    pub fn set_choice(
        &mut self,
        action: &str,
        route: Route,
        dry_run_first: bool,
    ) -> Result<Snapshot, CoreError> {
        if action != "update_keep_root" {
            self.notice = Some(block_notice(
                "Only update-and-keep-root is available in this version.",
            ));
            return self.snapshot();
        }
        self.action_chosen = true;
        self.route = route;
        self.prefer_dry_run = dry_run_first;
        self.pending = None;
        self.notice = None;
        self.snapshot()
    }

    pub fn continue_from_choose(&mut self) -> Result<Snapshot, CoreError> {
        if !self.action_chosen {
            self.notice = Some(block_notice("Choose update and keep root to continue."));
            return self.snapshot();
        }
        self.state = WizardState::PickFirmware;
        self.notice = None;
        self.snapshot()
    }

    pub fn remember_firmware(&mut self, display_name: &str, size: u64) -> FirmwareRef {
        let id = format!("fw-{}", self.next_nonce());
        self.picked
            .insert(id.clone(), (display_name.to_string(), size));
        FirmwareRef {
            id,
            display_name: display_name.to_string(),
            size,
        }
    }

    pub fn firmware_open(
        &mut self,
        firmware_id: &str,
        published_sha256: &str,
    ) -> Result<Snapshot, CoreError> {
        let name = self
            .picked
            .get(firmware_id)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| firmware_id.to_string());
        self.open_named(&name, published_sha256)
    }

    pub fn prepare_patch(&mut self, firmware_id: &str) -> Result<Snapshot, CoreError> {
        if self.running || self.state != WizardState::PickFirmware {
            return Err(CoreError::WrongState);
        }
        let Some(firmware) = self.firmware.clone() else {
            self.notice = Some(block_notice("Check a package before patching."));
            return self.snapshot();
        };
        if firmware.id != firmware_id {
            self.notice = Some(block_notice("Open that package before patching."));
            return self.snapshot();
        }
        self.state = WizardState::Patching;
        let image = format!(
            "%LOCALAPPDATA%\\Flashwright\\cache\\stock\\{}.img",
            &firmware.sha256[..12]
        );
        let serial = self.selected.clone().unwrap_or_default();
        let steps = prepare_patch_steps(&serial, &image);
        self.issue(PlanKind::PreparePatch, false, None, steps, firmware.route)?;
        self.notice = None;
        self.snapshot()
    }

    pub fn build_plan(&mut self) -> Result<Snapshot, CoreError> {
        let Some(serial) = self.selected.clone() else {
            self.notice = Some(block_notice("Select a phone before building a plan."));
            return self.snapshot();
        };
        let info = self.transport.info(&serial)?;
        let Some(firmware) = self.firmware.clone() else {
            self.notice = Some(block_notice("Check a package before building a plan."));
            return self.snapshot();
        };
        if !self.action_chosen {
            self.notice = Some(block_notice("Choose an action before building a plan."));
            return self.snapshot();
        }
        let target = match inactive_slot(info.active_slot) {
            Ok(slot) => slot,
            Err(err) => {
                self.notice = Some(block_notice(err.to_string()));
                return self.snapshot();
            }
        };
        let partition = if info.uses_init_boot {
            Partition::InitBoot
        } else {
            Partition::Boot
        };
        let patched = format!(
            "%LOCALAPPDATA%\\Flashwright\\cache\\patched\\{}.img",
            &firmware.sha256[..12]
        );
        let steps = match firmware.route {
            Route::Ota => ota_steps(&serial, target, partition, &firmware.name, &patched),
            Route::FactoryKeepData => {
                factory_steps(&serial, target, partition, &firmware.name, &patched)
            }
        };
        if let Err(err) = assert_no_forbidden_args(&steps) {
            self.notice = Some(block_notice(err.to_string()));
            return self.snapshot();
        }
        self.issue(
            PlanKind::UpdateKeepRoot,
            self.prefer_dry_run,
            None,
            steps,
            firmware.route,
        )?;
        self.job = JobView::default();
        self.notice = None;
        self.snapshot()
    }

    pub fn ack_gate(
        &mut self,
        plan_hash_value: &str,
        gate_id: &str,
    ) -> Result<Snapshot, CoreError> {
        let Some(pending) = self.pending.as_ref() else {
            return Err(CoreError::Rejected {
                reason: "There is no plan to acknowledge.".to_string(),
            });
        };
        if pending.preview.plan_hash != plan_hash_value || pending.life != Life::Issued {
            return Err(CoreError::Rejected {
                reason: "That plan code was not issued by Flashwright.".to_string(),
            });
        }
        let Some(gate) = pending.preview.gates.iter().find(|gate| gate.id == gate_id) else {
            return Err(CoreError::Rejected {
                reason: "That check is not on this plan.".to_string(),
            });
        };
        if gate.severity != "ack" {
            return Err(CoreError::Rejected {
                reason: "That check cannot be acknowledged.".to_string(),
            });
        }
        let mut gates = pending.preview.gates.clone();
        if let Some(gate) = gates.iter_mut().find(|gate| gate.id == gate_id) {
            gate.status = "pass".to_string();
        }
        let kind = pending.preview.kind;
        let dry = pending.body.dry_run;
        let after = pending.body.after_dry_run.clone();
        let route = pending.body.route;
        let steps = pending.preview.steps.clone();
        self.pending = None;
        self.issue_with_gates(kind, dry, after, steps, route, gates)?;
        self.snapshot()
    }

    pub fn dry_run(&mut self, plan_hash_value: &str) -> Result<Snapshot, CoreError> {
        let steps = self
            .pending
            .as_ref()
            .map(|pending| pending.preview.steps.clone())
            .unwrap_or_default();
        let kind = self
            .pending
            .as_ref()
            .map(|pending| pending.preview.kind)
            .unwrap_or(PlanKind::UpdateKeepRoot);
        self.accept(plan_hash_value, true)?;
        let safety_lines = self.safety_lines();
        let blocked = safety_lines
            .iter()
            .any(|line| line.starts_with("WOULD BLOCK"));
        let lines = safety_lines
            .into_iter()
            .map(|line| log_line("would", line))
            .collect();
        self.job = JobView {
            state: "dry_done".to_string(),
            progress: 100,
            status_line: "Dry run finished. No write was started.".to_string(),
            lines,
            result_title: "Dry run".to_string(),
            result_body: "Write steps were printed only. The phone was not rebooted.".to_string(),
            recovery: Vec::new(),
            cancel_mode: "immediate".to_string(),
        };
        self.notice = None;
        if blocked {
            self.pending = None;
            self.state = WizardState::DryRunDone;
            self.notice = Some(block_notice(
                "A write step would be blocked. The real plan was not issued.",
            ));
            return self.snapshot();
        }
        let route = self
            .firmware
            .as_ref()
            .map(|fw| fw.route)
            .unwrap_or(self.route);
        self.issue(kind, false, Some(plan_hash_value.to_string()), steps, route)?;
        self.notice = Some(Notice {
            level: "info".to_string(),
            message: "The plan code changed for the real flash.".to_string(),
            gates: Vec::new(),
        });
        self.snapshot()
    }

    pub fn confirm_and_run(&mut self, plan_hash_value: &str) -> Result<Snapshot, CoreError> {
        self.confirm_with_fault(plan_hash_value, false)
    }

    pub fn confirm_with_fault(
        &mut self,
        plan_hash_value: &str,
        fail_patched_flash: bool,
    ) -> Result<Snapshot, CoreError> {
        let steps = self
            .pending
            .as_ref()
            .map(|pending| pending.preview.steps.clone())
            .unwrap_or_default();
        let build_id = self
            .pending
            .as_ref()
            .map(|pending| pending.preview.build_id.clone())
            .unwrap_or_default();
        let target = self
            .pending
            .as_ref()
            .map(|pending| pending.preview.target_slot);
        let source = self
            .pending
            .as_ref()
            .map(|pending| pending.body.active_slot);
        if let Some(reason) = self.write_block() {
            return Err(CoreError::Rejected { reason });
        }
        let kind = self.pending.as_ref().map(|pending| pending.preview.kind);
        self.accept(plan_hash_value, false)?;
        let root_version = self
            .selected_info()
            .map(|info| info.root_tool_version)
            .unwrap_or_else(|| "30.7".to_string());
        self.running = true;
        self.state = WizardState::Flash;
        if fail_patched_flash {
            self.running = false;
            self.state = WizardState::Recovery;
            self.job = JobView {
                state: "failed".to_string(),
                progress: 70,
                status_line: "The update stopped.".to_string(),
                lines: vec![log_line("error", "Flash of the patched image failed. The phone was not rebooted.")],
                result_title: "The update stopped".to_string(),
                result_body: "The patched image was not written. Pick a recovery option. Each option is its own checked plan.".to_string(),
                recovery: {
                    let Some(target) = target else {
                        return Err(CoreError::Rejected {
                            reason: "The active slot is unknown.".to_string(),
                        });
                    };
                    let Some(source) = source else {
                        return Err(CoreError::Rejected {
                            reason: "The active slot is unknown.".to_string(),
                        });
                    };
                    recovery_options(target, source)
                },
                cancel_mode: "after_step".to_string(),
            };
            self.notice = None;
            return self.snapshot();
        }
        let mut lines = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            if self
                .stop_after_steps
                .is_some_and(|limit| index as u32 >= limit)
            {
                self.running = false;
                self.job = JobView {
                    state: "cancelled".to_string(),
                    progress: 40,
                    status_line: "Stopped after this step.".to_string(),
                    lines,
                    result_title: "Stopped".to_string(),
                    result_body: "Stopped after this step.".to_string(),
                    recovery: Vec::new(),
                    cancel_mode: "after_step".to_string(),
                };
                self.notice = None;
                return self.snapshot();
            }
            lines.push(log_line("run", quote_argv(&step.argv)));
        }
        self.running = false;
        self.stop_after_steps = None;
        if kind == Some(PlanKind::PreparePatch) {
            if let Some(firmware) = self.firmware.as_mut() {
                firmware.patched_ready = true;
            }
            self.state = WizardState::PickFirmware;
            self.job = JobView {
                state: "succeeded".to_string(),
                progress: 100,
                status_line: "Patch ready".to_string(),
                lines,
                result_title: "Patch ready".to_string(),
                result_body: "The patched image is ready. Nothing was flashed.".to_string(),
                recovery: Vec::new(),
                cancel_mode: "immediate".to_string(),
            };
            self.notice = None;
            return self.snapshot();
        }
        lines.push(log_line(
            "ok",
            format!("Updated to {build_id}. Root is working (Magisk app {root_version})."),
        ));
        self.state = WizardState::Done;
        self.job = JobView {
            state: "succeeded".to_string(),
            progress: 100,
            status_line: "Finished".to_string(),
            lines,
            result_title: format!("Updated to {build_id}"),
            result_body: format!("Root is working (Magisk app {root_version})."),
            recovery: Vec::new(),
            cancel_mode: "immediate".to_string(),
        };
        self.notice = None;
        self.snapshot()
    }

    pub fn cancel(&mut self) -> Result<Snapshot, CoreError> {
        if self.state == WizardState::Done {
            self.stop_after_steps = None;
            return self.snapshot();
        }
        self.stop_after_steps = Some(0);
        if matches!(self.state, WizardState::Flash) || self.running {
            self.running = false;
            self.state = WizardState::Flash;
            self.job.state = "cancelled".to_string();
            self.job.status_line = "Stopped after this step.".to_string();
            self.job.result_title = "Stopped".to_string();
            self.job.result_body = "Stopped after this step.".to_string();
            self.job.cancel_mode = "after_step".to_string();
        }
        self.snapshot()
    }

    pub fn note_link(&mut self, url_id: &str) -> Result<Snapshot, CoreError> {
        let _url = super::links::external_url(url_id)?;
        self.notice = Some(Notice {
            level: "info".to_string(),
            message: "That page is allow-listed. Flashwright opens it in your browser.".to_string(),
            gates: Vec::new(),
        });
        self.snapshot()
    }

    pub fn back(&mut self) -> Result<Snapshot, CoreError> {
        if self.running {
            self.notice = Some(block_notice("Wait for the current step to finish."));
            return self.snapshot();
        }
        if matches!(self.state, WizardState::Review(_)) {
            if let Some(pending) = self.pending.take() {
                self.consumed.remove(&pending.preview.plan_hash);
                // Discarded, not consumed: a later confirm is WrongState once we leave Review.
                let _ = pending;
            }
        }
        self.state = match self.state {
            WizardState::Choose => WizardState::Connect,
            WizardState::PickFirmware | WizardState::Patching => WizardState::Choose,
            WizardState::Review(_) | WizardState::DryRunDone => WizardState::PickFirmware,
            other => other,
        };
        self.notice = None;
        self.snapshot()
    }

    pub fn recovery_plan(&mut self, option_id: &str) -> Result<Snapshot, CoreError> {
        if self.state != WizardState::Recovery {
            return Err(CoreError::WrongState);
        }
        let known = ["retry", "stock", "switch_back", "restore", "leave"];
        if !known.contains(&option_id) {
            return Err(CoreError::Rejected {
                reason: "Unknown recovery option.".to_string(),
            });
        }
        let info = self.selected_info().ok_or_else(|| CoreError::Rejected {
            reason: "Select a phone before recovery.".to_string(),
        })?;
        let source = info.active_slot.ok_or_else(|| CoreError::Rejected {
            reason: "The active slot is unknown.".to_string(),
        })?;
        let target = inactive_slot(Some(source))?;
        let partition = if info.uses_init_boot {
            "init_boot"
        } else {
            "boot"
        };
        let sha = self
            .firmware
            .as_ref()
            .map(|firmware| firmware.sha256.clone())
            .unwrap_or_default();
        let steps = vec![recovery_step(
            option_id,
            &info.serial,
            source,
            target,
            partition,
            &cache_image("patched", &sha),
            &cache_image("stock", &sha),
        )?];
        self.issue(PlanKind::Recovery, false, None, steps, Route::Ota)?;
        self.notice = Some(Notice {
            level: "info".to_string(),
            message: format!(
                "Recovery option “{option_id}” is plan {}.",
                self.pending
                    .as_ref()
                    .map(|p| p.preview.plan_code.as_str())
                    .unwrap_or("")
            ),
            gates: Vec::new(),
        });
        self.snapshot()
    }

    pub fn backups_list(&mut self) -> Result<Snapshot, CoreError> {
        self.snapshot()
    }

    pub fn restore_plan(&mut self, set_id: &str, item: &str) -> Result<Snapshot, CoreError> {
        if self.running
            || matches!(
                self.state,
                WizardState::Flash | WizardState::Done | WizardState::Patching
            )
        {
            return Err(CoreError::WrongState);
        }
        if set_id != BACKUP_SET_ID {
            return Err(CoreError::Rejected {
                reason: "That backup set is not on this computer.".to_string(),
            });
        }
        let image = backup_item_path(item)?;
        let info = self.selected_info().ok_or_else(|| CoreError::Rejected {
            reason: "Select a phone before restoring.".to_string(),
        })?;
        let target = inactive_slot(info.active_slot)?;
        let partition = if info.uses_init_boot {
            "init_boot"
        } else {
            "boot"
        };
        let serial = info.serial.clone();
        let steps = vec![super::steps::Step {
            idx: 1,
            id: "restore_stock".to_string(),
            class: StepClass::Write,
            tool: super::steps::Tool::Fastboot,
            argv: vec![
                "fastboot".to_string(),
                "-s".to_string(),
                serial,
                "--slot".to_string(),
                target.as_str().to_string(),
                "flash".to_string(),
                partition.to_string(),
                image,
            ],
            timeout_s: 120,
        }];
        self.issue(PlanKind::RestoreStock, false, None, steps, Route::Ota)?;
        self.snapshot()
    }

    pub fn burst_logs(&self, count: u32, cancel_after: Option<u32>) -> BurstStats {
        let mut emitted = 0u32;
        let mut cancelled = false;
        while emitted < count {
            if cancel_after.is_some_and(|limit| emitted >= limit) {
                cancelled = true;
                break;
            }
            emitted += 1;
            let _ = format!("log {emitted}");
        }
        BurstStats { emitted, cancelled }
    }

    pub fn subscribe_seed(&self) -> WizardEvent {
        WizardEvent::Log {
            v: 1,
            ts: "13:41:02".to_string(),
            level: "info".to_string(),
            source: "wizard".to_string(),
            line: "Subscribed to the wizard log.".to_string(),
        }
    }

    #[cfg(test)]
    pub fn plan_life_name(&self) -> Option<&'static str> {
        self.pending.as_ref().map(|pending| match pending.life {
            Life::Issued => "issued",
            Life::Consumed => "consumed",
            Life::Discarded => "discarded",
        })
    }

    #[cfg(test)]
    pub fn consumed_contains(&self, plan_hash_value: &str) -> bool {
        self.consumed.contains(plan_hash_value)
    }

    #[cfg(test)]
    pub fn stop_limit(&self) -> Option<u32> {
        self.stop_after_steps
    }

    #[cfg(test)]
    pub fn testing_spoil_inputs(&mut self) {
        if let Some(firmware) = self.firmware.as_mut() {
            firmware.sha256 = "0".repeat(64);
        }
    }

    /// Move the window state without touching the pending plan. Tests use this
    /// to prove confirm is refused outside the matching review step.
    #[cfg(test)]
    pub fn testing_set_state(&mut self, name: &str) {
        self.state = match name {
            "connect" => WizardState::Connect,
            "choose" => WizardState::Choose,
            "pick_firmware" => WizardState::PickFirmware,
            "patching" => WizardState::Patching,
            "dry_run_done" => WizardState::DryRunDone,
            "flash" => WizardState::Flash,
            "done" => WizardState::Done,
            "recovery" => WizardState::Recovery,
            "review_other" => WizardState::Review(PlanKind::Recovery),
            _ => WizardState::Connect,
        };
    }

    fn open_named(&mut self, name: &str, published_sha256: &str) -> Result<Snapshot, CoreError> {
        let sha = published_sha256.trim().to_ascii_lowercase();
        if !name.to_ascii_lowercase().ends_with(".zip") {
            self.notice = Some(block_notice(
                "Choose a .zip package. Other archive types are not accepted.",
            ));
            self.firmware = None;
            return self.snapshot();
        }
        if sha.len() != 64 || !sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
            self.notice = Some(block_notice(
                "Paste the published 64-character SHA-256 checksum.",
            ));
            self.firmware = None;
            return self.snapshot();
        }
        let route = if name.contains("factory") {
            Route::FactoryKeepData
        } else {
            Route::Ota
        };
        let codename = name.split('-').next().unwrap_or("").to_string();
        let phone_codename = self
            .selected_info()
            .map(|info| info.codename)
            .unwrap_or_default();
        let gates = self.package_gates(name, &sha, &phone_codename);
        if gates.iter().any(|gate| gate.status == "fail") {
            self.firmware = None;
            self.pending = None;
            self.notice = Some(Notice {
                level: "block".to_string(),
                message: "The package did not pass the checks.".to_string(),
                gates,
            });
            return self.snapshot();
        }
        let partition = self
            .selected_info()
            .map(|info| {
                if info.uses_init_boot {
                    "init_boot"
                } else {
                    "boot"
                }
            })
            .unwrap_or("init_boot");
        self.route = route;
        self.firmware = Some(FirmwareReport {
            id: format!("fw-{codename}"),
            name: name.to_string(),
            route,
            sha256: sha,
            codename,
            build_id: "HQ1A.MOCK.002".to_string(),
            patched_ready: false,
            partition: partition.to_string(),
        });
        self.pending = None;
        self.notice = None;
        self.snapshot()
    }

    fn issue(
        &mut self,
        kind: PlanKind,
        dry_run: bool,
        after_dry_run: Option<String>,
        steps: Vec<Step>,
        route: Route,
    ) -> Result<(), CoreError> {
        self.issue_with_gates(
            kind,
            dry_run,
            after_dry_run,
            steps,
            route,
            self.pass_gates(),
        )
    }

    fn issue_with_gates(
        &mut self,
        kind: PlanKind,
        dry_run: bool,
        after_dry_run: Option<String>,
        steps: Vec<Step>,
        route: Route,
        gates: Vec<GateView>,
    ) -> Result<(), CoreError> {
        let Some(serial) = self.selected.clone() else {
            self.notice = Some(block_notice("Select a phone before building a plan."));
            return Ok(());
        };
        let info = self.transport.info(&serial)?;
        let Some(active) = info.active_slot else {
            return Err(CoreError::Rejected {
                reason: "The active slot is unknown.".to_string(),
            });
        };
        let target = inactive_slot(Some(active))?;
        let firmware_name = self
            .firmware
            .as_ref()
            .map(|fw| fw.name.clone())
            .unwrap_or_default();
        let firmware_sha = self
            .firmware
            .as_ref()
            .map(|fw| fw.sha256.clone())
            .unwrap_or_default();
        let build_id = self
            .firmware
            .as_ref()
            .map(|fw| fw.build_id.clone())
            .unwrap_or_else(|| "HQ1A.MOCK.002".to_string());
        let partition = if info.uses_init_boot {
            Partition::InitBoot
        } else {
            Partition::Boot
        };
        let now = self.clock.unix_ms();
        let body = PlanBody {
            schema: SCHEMA,
            plan_id: self.next_id(now),
            nonce_hex: self.next_nonce(),
            created_unix_ms: now,
            expires_unix_ms: now + FIFTEEN_MIN_MS,
            serial,
            codename: info.codename.clone(),
            fingerprint: info.fingerprint.clone(),
            spl: info.spl.clone(),
            active_slot: active,
            target_slot: target,
            bootloader_unlocked: info.bootloader_unlocked,
            route,
            firmware_name,
            firmware_sha256: firmware_sha,
            target_partition: partition,
            backup_set_id: BACKUP_SET_ID.to_string(),
            dry_run,
            after_dry_run,
            kind,
            gates: gates.clone(),
            steps: steps.clone(),
        };
        let hash = canonical_hash(&body).map_err(|err| CoreError::Rejected {
            reason: format!("Could not canonicalise the plan: {err}"),
        })?;
        let preview = WizardPlan {
            plan_code: plan_label(&hash, dry_run),
            plan_hash: hash,
            expires_unix_ms: body.expires_unix_ms,
            route,
            target_slot: target,
            codename: info.codename,
            build_id,
            backup_set_id: BACKUP_SET_ID.to_string(),
            gates,
            steps,
            dry_run,
            after_dry_run: body.after_dry_run.clone(),
            kind,
        };
        self.pending = Some(Pending {
            body,
            preview,
            life: Life::Issued,
        });
        self.state = WizardState::Review(kind);
        Ok(())
    }

    fn accept(&mut self, plan_hash_value: &str, call_dry: bool) -> Result<(), CoreError> {
        let kind_matches = match (self.state, self.pending.as_ref()) {
            (WizardState::Review(state_kind), Some(pending)) => state_kind == pending.preview.kind,
            _ => false,
        };
        let life = self
            .pending
            .as_ref()
            .map(|pending| pending.life)
            .unwrap_or(Life::Discarded);
        let hash_eq = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.preview.plan_hash == plan_hash_value);
        let expired = self
            .pending
            .as_ref()
            .is_some_and(|pending| self.clock.unix_ms() > pending.body.expires_unix_ms);
        let plan_dry = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.body.dry_run);
        let verdict = gate::check(Check {
            previously_consumed: self.consumed.contains(plan_hash_value),
            in_matching_review: kind_matches,
            life,
            hash_eq,
            expired,
            running: self.running,
            device_ok: self.device_ok(),
            inputs_ok: self.inputs_ok(),
            plan_dry,
            call_dry,
        })?;
        match verdict {
            gate::Verdict::Consume => {
                if let Some(pending) = self.pending.as_mut() {
                    pending.life = Life::Consumed;
                }
                self.consumed.insert(plan_hash_value.to_string());
                Ok(())
            }
            gate::Verdict::Discard(reason) => {
                if let Some(pending) = self.pending.as_mut() {
                    pending.life = Life::Discarded;
                }
                Err(gate::discard_error(reason))
            }
        }
    }

    fn device_ok(&self) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        let Ok(info) = self.transport.info(&pending.body.serial) else {
            return false;
        };
        info.serial == pending.body.serial
            && info.fingerprint == pending.body.fingerprint
            && info.active_slot == Some(pending.body.active_slot)
            && info.bootloader_unlocked == pending.body.bootloader_unlocked
    }

    fn inputs_ok(&self) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        self.firmware.as_ref().map(|fw| fw.sha256.as_str())
            == Some(pending.body.firmware_sha256.as_str())
            || pending.preview.kind == PlanKind::Recovery
            || pending.preview.kind == PlanKind::RestoreStock
    }

    /// Facts for the window review. The numbered gates are decided only by
    /// `safety::evaluate`. These fields are the phone and tool inputs.
    fn review_facts(&self) -> safety::SafetyFacts {
        let mut facts = safety::SafetyFacts::synthetic_komodo();
        let device_unlocked = self
            .selected_info()
            .is_none_or(|info| info.bootloader_unlocked);
        facts.unlocked = Some(self.step_bootloader_ok && device_unlocked);
        facts.tools_verified = self.exe_ok;
        facts.tools_match = self.exe_ok;
        facts.adb_server_ok = self.server_ok;
        facts.partition_bytes = if self.partition_ok {
            Some(64 * 1024 * 1024)
        } else {
            Some(1)
        };
        facts
    }

    fn catalogue_plan(&self) -> Vec<PlanStep> {
        let serial = self
            .pending
            .as_ref()
            .map(|pending| pending.body.serial.clone())
            .or_else(|| self.selected.clone())
            .unwrap_or_default();
        let (slot, partition) = if let Some(pending) = &self.pending {
            (pending.body.target_slot, pending.body.target_partition)
        } else if let Some(info) = self.selected_info() {
            let slot = inactive_slot(info.active_slot).unwrap_or(Slot::B);
            let partition = if info.uses_init_boot {
                Partition::InitBoot
            } else {
                Partition::Boot
            };
            (slot, partition)
        } else {
            (Slot::B, Partition::InitBoot)
        };
        let Ok(serial) = DeviceSerial::try_from(serial.as_str()) else {
            return Vec::new();
        };
        let image = ImageRef::new(1, window_image_path(), 4096);
        vec![PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash {
            serial,
            slot: core_slot(slot),
            partition: core_partition(partition),
            image,
        }))]
    }

    fn safety_inputs(
        &self,
    ) -> (
        Vec<PlanStep>,
        safety::SafetyFacts,
        Option<safety::BackupState>,
    ) {
        let steps = self.catalogue_plan();
        let mut facts = self.review_facts();
        if let Some(partition) = flash_partition(&steps) {
            facts.target_partition = partition;
        }
        let backup = backup_for(&steps);
        (steps, facts, backup)
    }

    fn safety_lines(&self) -> Vec<String> {
        let (steps, facts, backup) = self.safety_inputs();
        let decisions = safety::evaluate(&steps, Some(&facts), backup.as_ref());
        let mut lines = safety::dry_run_lines(&steps, &decisions);
        if lines.iter().all(|line| !line.starts_with("WOULD BLOCK")) {
            for step in &steps {
                let collected = safety::CollectedFacts::from_ref(&facts);
                let blocks = safety::evaluate_step(step, Some(&collected));
                if !blocks.is_empty() {
                    lines = blocks
                        .iter()
                        .map(|gate| format!("WOULD BLOCK: {} {}", gate.id, gate.reason))
                        .collect();
                    break;
                }
            }
        }
        lines
    }

    fn write_block(&self) -> Option<String> {
        let (steps, facts, backup) = self.safety_inputs();
        let decisions = safety::evaluate(&steps, Some(&facts), backup.as_ref());
        if let Some(gate) = decisions
            .iter()
            .find(|gate| gate.blocked && gate.severity == safety::Severity::Block)
        {
            return Some(format!("Blocked: {} {}", gate.id, gate.reason));
        }
        for step in &steps {
            let collected = safety::CollectedFacts::from_ref(&facts);
            if let Some(gate) = safety::evaluate_step(step, Some(&collected))
                .into_iter()
                .next()
            {
                return Some(format!("Blocked: {} {}", gate.id, gate.reason));
            }
        }
        None
    }

    fn package_gates(&self, name: &str, sha: &str, phone: &str) -> Vec<GateView> {
        let (steps, mut facts, backup) = self.safety_inputs();
        // Package open compares the file with the phone. The boot image
        // choice is checked later, with the plan.
        facts.target_partition = crate::device::Partition::InitBoot;
        let prefix = name.split('-').next().unwrap_or("");
        if prefix != phone {
            facts.firmware_codename = prefix.to_string();
            facts.firmware_filename = name.to_string();
        } else {
            let code = facts.device_codename.clone();
            facts.firmware_codename = code.clone();
            facts.firmware_filename = format!("{code}-package.zip");
        }
        let fixture_ok = fixture_sha() == Some(sha);
        let fragment_ok = sha.len() >= 8 && name.to_ascii_lowercase().contains(&sha[..8]);
        if !fixture_ok || !fragment_ok {
            facts.firmware_sha256 = Some(sha.to_string());
            facts.image_sha256 = Some("mismatch".to_string());
        }
        safety::evaluate(&steps, Some(&facts), backup.as_ref())
            .into_iter()
            .filter(|decision| decision.blocked && (decision.id == "G04" || decision.id == "G05"))
            .map(view_from)
            .collect()
    }

    fn gate_views(&self) -> Vec<GateView> {
        let (steps, facts, backup) = self.safety_inputs();
        safety::evaluate(&steps, Some(&facts), backup.as_ref())
            .into_iter()
            .map(view_from)
            .collect()
    }

    fn selected_info(&self) -> Option<super::device::DeviceInfo> {
        self.selected
            .as_ref()
            .and_then(|serial| self.transport.info(serial).ok())
    }

    fn next_id(&mut self, unix_ms: i64) -> Uuid {
        self.nonce_counter += 1;
        let mut bytes = [0u8; 16];
        let millis = u64::try_from(unix_ms).unwrap_or(0);
        bytes[0..6].copy_from_slice(&millis.to_be_bytes()[2..8]);
        bytes[6] = 0x70 | ((self.nonce_counter as u8) & 0x0f);
        bytes[8] = 0x80;
        bytes[14..].copy_from_slice(&(self.nonce_counter as u16).to_be_bytes());
        Uuid::from_bytes(bytes)
    }

    fn next_nonce(&mut self) -> String {
        self.nonce_counter += 1;
        format!("{:032x}", self.nonce_counter)
    }

    fn base_gates(&self) -> Vec<GateView> {
        self.gate_views()
    }

    fn pass_gates(&self) -> Vec<GateView> {
        self.base_gates()
    }
}

fn window_image_path() -> String {
    if cfg!(windows) {
        r"C:\flashwright\init_boot.img".to_string()
    } else {
        "/var/flashwright/init_boot.img".to_string()
    }
}

fn core_slot(slot: Slot) -> crate::device::Slot {
    match slot {
        Slot::A => crate::device::Slot::A,
        Slot::B => crate::device::Slot::B,
    }
}

fn core_partition(partition: Partition) -> crate::device::Partition {
    match partition {
        Partition::Boot => crate::device::Partition::Boot,
        Partition::InitBoot => crate::device::Partition::InitBoot,
        Partition::Vbmeta => crate::device::Partition::Vbmeta,
        Partition::Bootloader => crate::device::Partition::Bootloader,
        Partition::Radio => crate::device::Partition::Radio,
    }
}

fn flash_partition(steps: &[PlanStep]) -> Option<crate::device::Partition> {
    for step in steps {
        if let PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash { partition, .. })) = step {
            return Some(*partition);
        }
    }
    None
}

fn backup_for(steps: &[PlanStep]) -> Option<safety::BackupState> {
    let PlanStep::Write(WriteCmd::Fastboot(FastbootWrite::Flash {
        serial,
        slot,
        partition,
        ..
    })) = steps.first()?
    else {
        return None;
    };
    if *partition == crate::device::Partition::Vbmeta {
        return None;
    }
    Some(safety::BackupState::Verified(safety::BackupSet {
        set_id: BACKUP_SET_ID.to_string(),
        manifest_sha256: "window-manifest".to_string(),
        serial_sha256: safety::sha256_hex(serial.as_str().as_bytes()),
        slot: *slot,
        partition: *partition,
        dir: std::path::PathBuf::new(),
    }))
}

fn view_from(decision: GateDecision) -> GateView {
    let severity = match decision.severity {
        safety::Severity::Block => "block",
        safety::Severity::Ack => "ack",
    };
    let status = if decision.blocked { "fail" } else { "pass" };
    let evidence = if decision.reason.is_empty() {
        "Passed."
    } else {
        decision.reason.as_str()
    };
    gate(decision.id, severity, status, decision.id, evidence)
}

fn fixture_sha() -> Option<&'static str> {
    #[cfg(any(test, feature = "mock", debug_assertions))]
    {
        Some(crate::mock::FIXTURE_SHA256)
    }
    #[cfg(not(any(test, feature = "mock", debug_assertions)))]
    {
        None
    }
}

fn visible_phase(state: WizardState) -> Phase {
    match state {
        WizardState::Connect => Phase::Connect,
        WizardState::Choose => Phase::Choose,
        WizardState::PickFirmware | WizardState::Patching => Phase::Firmware,
        WizardState::Review(_) | WizardState::DryRunDone => Phase::Review,
        WizardState::Flash => Phase::Flash,
        WizardState::Done => Phase::Done,
        WizardState::Recovery => Phase::Recovery,
    }
}

fn cache_image(kind: &str, sha: &str) -> String {
    let prefix = if sha.len() >= 12 { &sha[..12] } else { "stock" };
    format!("%LOCALAPPDATA%\\Flashwright\\cache\\{kind}\\{prefix}.img")
}

fn backup_item_path(item: &str) -> Result<String, CoreError> {
    let ok = !item.is_empty()
        && item.len() <= 128
        && !item.starts_with('.')
        && item.contains('.')
        && item
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-');
    if !ok {
        return Err(CoreError::Rejected {
            reason: "That backup item is not in this set.".to_string(),
        });
    }
    Ok(format!(
        "%LOCALAPPDATA%\\Flashwright\\backups\\{BACKUP_SET_ID}\\{item}"
    ))
}

fn recovery_step(
    option_id: &str,
    serial: &str,
    source: Slot,
    target: Slot,
    partition: &str,
    patched: &str,
    stock: &str,
) -> Result<Step, CoreError> {
    let (class, tool, argv) = match option_id {
        "switch_back" => (
            StepClass::Write,
            super::steps::Tool::Fastboot,
            vec![
                "fastboot".to_string(),
                "-s".to_string(),
                serial.to_string(),
                format!("--set-active={}", source.as_str()),
            ],
        ),
        "leave" => (
            StepClass::Read,
            super::steps::Tool::Internal,
            vec!["report".to_string(), serial.to_string()],
        ),
        "retry" | "stock" | "restore" => {
            let image = match option_id {
                "retry" => patched.to_string(),
                "stock" => stock.to_string(),
                _ => backup_item_path("init_boot.img")?,
            };
            (
                StepClass::Write,
                super::steps::Tool::Fastboot,
                vec![
                    "fastboot".to_string(),
                    "-s".to_string(),
                    serial.to_string(),
                    "--slot".to_string(),
                    target.as_str().to_string(),
                    "flash".to_string(),
                    partition.to_string(),
                    image,
                ],
            )
        }
        _ => {
            return Err(CoreError::Rejected {
                reason: "Unknown recovery option.".to_string(),
            })
        }
    };
    Ok(Step {
        idx: 1,
        id: format!("recovery_{option_id}"),
        class,
        tool,
        argv,
        timeout_s: 120,
    })
}

fn recovery_options(target: Slot, source: Slot) -> Vec<RecoveryOption> {
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

fn gate(id: &str, severity: &str, status: &str, title: &str, evidence: &str) -> GateView {
    GateView {
        id: id.to_string(),
        severity: severity.to_string(),
        status: status.to_string(),
        title: title.to_string(),
        evidence: evidence.to_string(),
    }
}

fn block_notice(message: impl Into<String>) -> Notice {
    Notice {
        level: "block".to_string(),
        message: message.into(),
        gates: Vec::new(),
    }
}

fn log_line(level: &str, text: impl Into<String>) -> LogLine {
    LogLine {
        ts: "13:41:02".to_string(),
        level: level.to_string(),
        text: text.into(),
    }
}
