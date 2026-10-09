// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::device::{DeviceTransport, Mode, Partition, Slot};
use crate::error::CoreError;
use crate::mock::FIXTURE_SHA256;
use crate::model::{
    mode_label, recovery_options, BackupSet, BurstStats, ChoiceView, DriverStatus, ExternalLink,
    FirmwareReport, JobView, LogLine, Notice, Phase, Snapshot, ToolsStatus,
};
use crate::plan::{
    assert_no_forbidden_args, factory_steps, ota_steps, plan_code, plan_hash, GateView, PlanBody,
    PlanPreview, Route, StepClass,
};

const FIFTEEN_MIN_MS: i64 = 15 * 60 * 1000;
const BACKUP_SET_ID: &str = "4f2a9c01-0000-7000-8000-000000000001";

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

struct Pending {
    body: PlanBody,
    preview: PlanPreview,
}

pub struct Engine<T: DeviceTransport> {
    transport: T,
    clock: Box<dyn Clock>,
    phase: Phase,
    selected: Option<String>,
    route: Route,
    prefer_dry_run: bool,
    action_chosen: bool,
    firmware: Option<FirmwareReport>,
    pending: Option<Pending>,
    job: JobView,
    notice: Option<Notice>,
    nonce_counter: u64,
    running: bool,
}

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
            phase: Phase::Connect,
            selected: None,
            route: Route::Ota,
            prefer_dry_run: true,
            action_chosen: false,
            firmware: None,
            pending: None,
            job: JobView::default(),
            notice: None,
            nonce_counter: 0,
            running: false,
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn set_clock(&mut self, clock: Box<dyn Clock>) {
        self.clock = clock;
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
                    "On-device root tool".to_string(),
                    format!("stable {}", info.root_tool_version),
                )
            } else {
                (
                    "On-device root tool".to_string(),
                    "not detected".to_string(),
                )
            }
        } else {
            ("On-device root tool".to_string(), "—".to_string())
        };
        Ok(Snapshot {
            phase: self.phase,
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
            plan: self.pending.as_ref().map(|pending| pending.preview.clone()),
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
        self.phase = Phase::Choose;
        self.notice = None;
        self.snapshot()
    }

    pub fn set_choice(
        &mut self,
        action: &str,
        route: Route,
        prefer_dry_run: bool,
    ) -> Result<Snapshot, CoreError> {
        if action != "update_keep_root" {
            self.notice = Some(block_notice(
                "Only update-and-keep-root is available in this version.",
            ));
            return self.snapshot();
        }
        self.action_chosen = true;
        self.route = route;
        self.prefer_dry_run = prefer_dry_run;
        self.pending = None;
        self.notice = None;
        self.snapshot()
    }

    pub fn reject_both_slots(&mut self) -> Result<Snapshot, CoreError> {
        self.notice = Some(block_notice(
            "Writing both slots removes your fallback and is risky with anti-rollback. Coming later in Expert mode.",
        ));
        self.snapshot()
    }

    pub fn continue_from_choose(&mut self) -> Result<Snapshot, CoreError> {
        if !self.action_chosen {
            self.notice = Some(block_notice("Choose update and keep root to continue."));
            return self.snapshot();
        }
        self.phase = Phase::Firmware;
        self.notice = None;
        self.snapshot()
    }

    pub fn open_firmware(
        &mut self,
        name: &str,
        published_sha256: &str,
    ) -> Result<Snapshot, CoreError> {
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
        let mut gates = self.base_gates();
        let mut failed = false;
        if sha != FIXTURE_SHA256 {
            mark_fail(
                &mut gates,
                "G05",
                "The file checksum does not match the published SHA-256.",
            );
            failed = true;
        } else if !name.to_ascii_lowercase().contains(&sha[..8]) {
            mark_fail(
                &mut gates,
                "G05",
                "The checksum fragment in the file name does not match the published SHA-256.",
            );
            failed = true;
        }
        let phone_codename = self.phone_codename();
        if codename != phone_codename {
            mark_fail(&mut gates, "G04", "This package is for a different phone.");
            failed = true;
        }
        if failed {
            self.firmware = None;
            self.pending = None;
            self.notice = Some(Notice {
                level: "block".to_string(),
                message: "The package did not pass the checks.".to_string(),
                gates,
            });
            return self.snapshot();
        }
        self.route = route;
        self.firmware = Some(FirmwareReport {
            id: format!("fw-{codename}"),
            name: name.to_string(),
            route,
            sha256: sha,
            codename,
            build_id: "HQ1A.MOCK.002".to_string(),
            patched_ready: true,
            partition: "init_boot".to_string(),
        });
        self.pending = None;
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
        let target = match crate::plan::target_for(info.active_slot) {
            Ok(slot) => slot,
            Err(err) => {
                self.notice = Some(block_notice(err.to_string()));
                return self.snapshot();
            }
        };
        let patched = format!(
            "%LOCALAPPDATA%\\Flashwright\\cache\\patched\\{}.img",
            &firmware.sha256[..12]
        );
        let steps = match firmware.route {
            Route::Ota => ota_steps(&serial, target, &firmware.name, &patched),
            Route::FactoryKeepData => factory_steps(&serial, target, &firmware.name, &patched),
        };
        if let Err(err) = assert_no_forbidden_args(&steps) {
            self.notice = Some(block_notice(err.to_string()));
            return self.snapshot();
        }
        let gates = self.pass_gates();
        if gates
            .iter()
            .any(|gate| gate.status == "fail" && gate.severity == "block")
        {
            self.notice = Some(Notice {
                level: "block".to_string(),
                message: "A blocking check failed.".to_string(),
                gates,
            });
            return self.snapshot();
        }
        let now = self.clock.unix_ms();
        let body = PlanBody {
            schema: crate::plan::SCHEMA,
            plan_id: self.next_id(now),
            nonce_hex: self.next_nonce(),
            created_unix_ms: now,
            expires_unix_ms: now + FIFTEEN_MIN_MS,
            serial: serial.clone(),
            codename: info.codename.clone(),
            fingerprint: info.fingerprint.clone(),
            spl: info.spl.clone(),
            active_slot: info.active_slot.unwrap_or(Slot::A),
            target_slot: target,
            bootloader_unlocked: info.bootloader_unlocked,
            route: firmware.route,
            firmware_name: firmware.name.clone(),
            firmware_sha256: firmware.sha256.clone(),
            target_partition: Partition::InitBoot,
            backup_set_id: BACKUP_SET_ID.to_string(),
            gates: gates.clone(),
            steps: steps.clone(),
        };
        let hash = plan_hash(&body)?;
        let preview = PlanPreview {
            plan_code: plan_code(&hash),
            plan_hash: hash,
            expires_unix_ms: body.expires_unix_ms,
            route: firmware.route,
            target_slot: target,
            codename: info.codename.clone(),
            build_id: firmware.build_id.clone(),
            backup_set_id: BACKUP_SET_ID.to_string(),
            gates,
            steps,
            prefer_dry_run: self.prefer_dry_run,
        };
        self.pending = Some(Pending { body, preview });
        self.phase = Phase::Review;
        self.job = JobView::default();
        self.notice = None;
        self.snapshot()
    }

    pub fn dry_run(&mut self, plan_hash_value: &str) -> Result<Snapshot, CoreError> {
        let Some(pending) = self.pending.as_ref() else {
            self.notice = Some(block_notice("Build a plan before a dry run."));
            return self.snapshot();
        };
        if pending.preview.plan_hash != plan_hash_value {
            return Err(CoreError::Rejected {
                reason: "The dry run hash does not match the plan this program issued.".to_string(),
            });
        }
        let mut lines = vec![
            log_line("read", "Pre-flight checks passed."),
            log_line("read", "Backup set is present and verified."),
        ];
        for step in &pending.preview.steps {
            if step.class == StepClass::Write {
                lines.push(log_line(
                    "would",
                    format!("WOULD RUN: {}", crate::plan::quote_argv(&step.argv)),
                ));
            }
        }
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
        let Some(pending) = self.pending.as_ref() else {
            return Err(CoreError::Rejected {
                reason: "There is no plan to run.".to_string(),
            });
        };
        if pending.preview.plan_hash != plan_hash_value {
            return Err(CoreError::Rejected {
                reason: "That plan code was not issued by Flashwright.".to_string(),
            });
        }
        if self.clock.unix_ms() > pending.body.expires_unix_ms {
            return Err(CoreError::Rejected {
                reason: "The plan has expired. Build it again.".to_string(),
            });
        }
        if self.running {
            return Err(CoreError::Rejected {
                reason: "A job is already running.".to_string(),
            });
        }
        let info = self.transport.info(&pending.body.serial)?;
        if info.serial != pending.body.serial
            || info.fingerprint != pending.body.fingerprint
            || info.active_slot != Some(pending.body.active_slot)
            || info.bootloader_unlocked != pending.body.bootloader_unlocked
        {
            return Err(CoreError::Rejected {
                reason: "The phone changed after the plan was built.".to_string(),
            });
        }
        if self.firmware.as_ref().map(|fw| fw.sha256.as_str())
            != Some(pending.body.firmware_sha256.as_str())
        {
            return Err(CoreError::Rejected {
                reason: "The package changed after the plan was built.".to_string(),
            });
        }
        let steps = pending.preview.steps.clone();
        let build_id = pending.preview.build_id.clone();
        let target = pending.preview.target_slot;
        let source = pending.body.active_slot;
        let root_version = info.root_tool_version.clone();
        self.running = true;
        self.phase = Phase::Flash;
        if fail_patched_flash {
            self.running = false;
            self.phase = Phase::Recovery;
            self.job = JobView {
                state: "failed".to_string(),
                progress: 70,
                status_line: "The update stopped.".to_string(),
                lines: vec![log_line("error", "Flash of the patched image failed. The phone was not rebooted.")],
                result_title: "The update stopped".to_string(),
                result_body: "The patched image was not written. Pick a recovery option. Each option is its own checked plan.".to_string(),
                recovery: recovery_options(target, source),
                cancel_mode: "after_step".to_string(),
            };
            self.notice = None;
            return self.snapshot();
        }
        let mut lines = Vec::new();
        for step in &steps {
            lines.push(log_line("run", crate::plan::quote_argv(&step.argv)));
        }
        lines.push(log_line(
            "ok",
            format!("Updated to {build_id}. Root is working (root tool {root_version})."),
        ));
        self.running = false;
        self.phase = Phase::Done;
        self.job = JobView {
            state: "succeeded".to_string(),
            progress: 100,
            status_line: "Finished".to_string(),
            lines,
            result_title: format!("Updated to {build_id}"),
            result_body: format!("Root is working (root tool {root_version})."),
            recovery: Vec::new(),
            cancel_mode: "immediate".to_string(),
        };
        self.notice = None;
        let _ = steps;
        self.snapshot()
    }

    pub fn cancel_mode_for_current(&self) -> &'static str {
        if self.phase == Phase::Flash && self.running {
            "after_step"
        } else {
            "immediate"
        }
    }

    /// Records that an allow-listed page was opened. The address stays in Rust.
    pub fn note_link(&mut self, url_id: &str) -> Result<Snapshot, CoreError> {
        let _url = external_url(url_id)?;
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
        self.phase = match self.phase {
            Phase::Choose => Phase::Connect,
            Phase::Firmware => Phase::Choose,
            Phase::Review => Phase::Firmware,
            Phase::Connect | Phase::Flash | Phase::Done | Phase::Recovery => self.phase,
        };
        self.notice = None;
        self.snapshot()
    }

    pub fn recovery_plan(&mut self, option_id: &str) -> Result<Snapshot, CoreError> {
        if self.phase != Phase::Recovery {
            return Err(CoreError::Rejected {
                reason: "Recovery plans are only available from the recovery page.".to_string(),
            });
        }
        let known = ["retry", "stock", "switch_back", "restore", "leave"];
        if !known.contains(&option_id) {
            return Err(CoreError::message("Unknown recovery option."));
        }
        let now = self.clock.unix_ms();
        let body = PlanBody {
            schema: crate::plan::SCHEMA,
            plan_id: self.next_id(now),
            nonce_hex: self.next_nonce(),
            created_unix_ms: now,
            expires_unix_ms: now + FIFTEEN_MIN_MS,
            serial: self.selected.clone().unwrap_or_default(),
            codename: "harbor".to_string(),
            fingerprint: "recovery".to_string(),
            spl: option_id.to_string(),
            active_slot: Slot::A,
            target_slot: Slot::B,
            bootloader_unlocked: true,
            route: Route::Ota,
            firmware_name: option_id.to_string(),
            firmware_sha256: FIXTURE_SHA256.to_string(),
            target_partition: Partition::InitBoot,
            backup_set_id: BACKUP_SET_ID.to_string(),
            gates: self.pass_gates(),
            steps: Vec::new(),
        };
        let hash = plan_hash(&body)?;
        self.notice = Some(Notice {
            level: "info".to_string(),
            message: format!(
                "Recovery option “{option_id}” is plan {}.",
                plan_code(&hash)
            ),
            gates: Vec::new(),
        });
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

    fn phone_codename(&self) -> String {
        self.selected
            .as_ref()
            .and_then(|serial| self.transport.info(serial).ok())
            .map(|info| info.codename)
            .unwrap_or_default()
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
        vec![
            gate(
                "G01",
                "block",
                "pass",
                "Platform tools",
                "37.0.1 is on the allow list.",
            ),
            gate(
                "G02",
                "block",
                "pass",
                "One phone",
                "One authorised phone in the device state.",
            ),
            gate(
                "G03",
                "block",
                "pass",
                "Unlocked",
                "The bootloader is unlocked.",
            ),
            gate(
                "G04",
                "block",
                "pass",
                "Phone match",
                "The package matches this phone.",
            ),
            gate(
                "G05",
                "block",
                "pass",
                "Checksum",
                "The pasted SHA-256 matches the file.",
            ),
            gate(
                "G06",
                "block",
                "pass",
                "Full package",
                "The package is a full A/B update.",
            ),
            gate(
                "G07",
                "block",
                "pass",
                "No downgrade",
                "The package is newer than the phone.",
            ),
            gate(
                "G08",
                "block",
                "pass",
                "Patch level",
                "The image patch level matches the package.",
            ),
            gate(
                "G09",
                "block",
                "pass",
                "Patched image",
                "The patched image matches the stock image that was prepared.",
            ),
            gate(
                "G10",
                "block",
                "pass",
                "Root tool",
                "The on-device root tool is new enough.",
            ),
            gate("G11", "block", "pass", "Battery", "Battery is 82%."),
            gate(
                "G12",
                "block",
                "pass",
                "Disk space",
                "The working disk has enough free space.",
            ),
            gate(
                "G13",
                "block",
                "pass",
                "Phone space",
                "The phone has enough free space.",
            ),
            gate(
                "G14",
                "block",
                "pass",
                "Backup",
                "A verified backup set exists.",
            ),
            gate(
                "G15",
                "block",
                "pass",
                "Partition",
                "The target partition exists and is large enough.",
            ),
            gate(
                "G16",
                "block",
                "pass",
                "Keep data",
                "The plan does not wipe data or turn off verification.",
            ),
            gate(
                "G17",
                "ack",
                "pass",
                "USB driver",
                "The driver probe is OK.",
            ),
            gate(
                "G18",
                "block",
                "pass",
                "Bootloader",
                "The package bootloader is not older.",
            ),
            gate(
                "G19",
                "ack",
                "pass",
                "Minimum bootloader",
                "The phone bootloader meets the fixture minimum.",
            ),
            gate(
                "G20",
                "block",
                "pass",
                "No waiting update",
                "No system update is waiting on the phone.",
            ),
        ]
    }

    fn pass_gates(&self) -> Vec<GateView> {
        self.base_gates()
    }
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

fn mark_fail(gates: &mut [GateView], id: &str, evidence: &str) {
    if let Some(gate) = gates.iter_mut().find(|gate| gate.id == id) {
        gate.status = "fail".to_string();
        gate.evidence = evidence.to_string();
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

pub fn external_url(id: &str) -> Result<&'static str, CoreError> {
    match id {
        "platform_tools" => Ok("https://developer.android.com/tools/releases/platform-tools"),
        "usb_driver" => Ok("https://developer.android.com/studio/run/win-usb"),
        "firmware_full" => Ok("https://developers.google.com/android/ota"),
        "firmware_factory" => Ok("https://developers.google.com/android/images"),
        _ => Err(CoreError::Rejected {
            reason: "That link is not on the allow list.".to_string(),
        }),
    }
}
