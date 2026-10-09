// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Talks to one phone through adb and fastboot.
//!
//! Read methods take a serial. Write methods also take a [`WriteToken`].
//! A missing device during a reboot is not treated as unplugged until the
//! wait deadline: the phone disappears from `adb devices` while it reboots.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::cmd::{AdbHostRead, CatalogueCommand, DeviceSerial, FastbootRead, FastbootVar, ReadCmd};
use crate::exe::{adb_server_matches, resolve_tool, SharedReadLocks, VerifiedExe};
use crate::proc::{CommandRunner, RunLimits, RunResult};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::device::catalog::{AliasTable, DeviceTable};
use crate::device::info::{assemble_from_adb, assemble_from_fastboot, AdbTexts};
use crate::device::parse::{
    merge_scans, parse_adb_devices, parse_fastboot_devices, parse_mode_token,
};
use crate::device::{
    DeviceError, DeviceInfo, Mode, Partition, RebootTarget, ScanEntry, Slot, TransportConfig,
    WaitOutcome, WaitTarget, WriteToken, MAX_PARALLEL_PROBES,
};

/// Concrete transport over a [`CommandRunner`].
struct ArmedRun {
    plan_hash: String,
    run_id: u128,
    serial: String,
    pending: VecDeque<Vec<u8>>,
}

pub struct PlatformToolsTransport<R: CommandRunner> {
    runner: Arc<R>,
    adb: PathBuf,
    fastboot: PathBuf,
    config: TransportConfig,
    active: Arc<Mutex<Option<ArmedRun>>>,
    verified: Arc<Mutex<Option<VerifiedTools>>>,
    writes_allowed: Arc<Mutex<bool>>,
}

struct VerifiedTools {
    adb: VerifiedExe,
    fastboot: VerifiedExe,
    listener: Option<crate::exe::ListenerImage>,
}

impl<R: CommandRunner> Clone for PlatformToolsTransport<R> {
    fn clone(&self) -> Self {
        Self {
            runner: Arc::clone(&self.runner),
            adb: self.adb.clone(),
            fastboot: self.fastboot.clone(),
            config: self.config.clone(),
            active: Arc::clone(&self.active),
            verified: Arc::clone(&self.verified),
            writes_allowed: Arc::clone(&self.writes_allowed),
        }
    }
}

impl<R: CommandRunner> PlatformToolsTransport<R> {
    pub fn new(
        runner: Arc<R>,
        adb: impl Into<PathBuf>,
        fastboot: impl Into<PathBuf>,
        config: TransportConfig,
    ) -> Self {
        Self {
            runner,
            adb: adb.into(),
            fastboot: fastboot.into(),
            config,
            active: Arc::new(Mutex::new(None)),
            verified: Arc::new(Mutex::new(None)),
            writes_allowed: Arc::new(Mutex::new(false)),
        }
    }

    /// Record whether the platform-tools verdict allows a write.
    pub(crate) fn note_tools_verdict(&self, writes_allowed: bool) {
        *self.writes_allowed.lock().expect("tools verdict") = writes_allowed;
    }

    pub(crate) fn writes_allowed(&self) -> bool {
        *self.writes_allowed.lock().expect("tools verdict")
    }

    /// Install hashes measured at import. Writes re-check them while the files stay open.
    pub(crate) fn install_verified(
        &self,
        adb: VerifiedExe,
        fastboot: VerifiedExe,
        listener: Option<crate::exe::ListenerImage>,
    ) {
        *self.verified.lock().expect("verified tools") = Some(VerifiedTools {
            adb,
            fastboot,
            listener,
        });
    }

    /// A directory watcher calls this when a managed platform-tools file changes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn invalidate_plan(&self) {
        *self.active.lock().expect("armed run") = None;
    }

    pub(crate) fn arm(&self, plan: &crate::token::ConfirmedPlan) {
        *self.active.lock().expect("armed run") = Some(ArmedRun {
            plan_hash: plan.plan_hash().to_string(),
            run_id: plan.run_id().get(),
            serial: plan.serial().to_string(),
            pending: plan.steps().iter().cloned().collect(),
        });
    }

    pub async fn run_read(&self, cmd: crate::cmd::ReadCmd) -> Result<RunResult, DeviceError> {
        let rendered =
            crate::cmd::read_argv(&cmd).map_err(|err| DeviceError::Message(err.to_string()))?;
        let budget = crate::timeouts::read_budget(&cmd);
        let program = self.program_for(rendered.tool);
        let command = CatalogueCommand::from_rendered(rendered);
        self.run_tool(program, command, RunLimits::from_budget(&budget))
            .await
    }

    pub async fn stream_read(
        &self,
        cmd: crate::cmd::ReadCmd,
    ) -> Result<Vec<crate::proc::StreamLine>, DeviceError> {
        Ok(self.run_read(cmd).await?.lines)
    }

    pub(crate) async fn run_write(
        &self,
        token: &WriteToken,
        cmd: crate::cmd::WriteCmd,
    ) -> Result<RunResult, DeviceError> {
        let locks = self.reverify_before_write(&cmd)?;
        self.authorize(token, &cmd)?;
        let rendered =
            crate::cmd::write_argv(&cmd).map_err(|err| DeviceError::Message(err.to_string()))?;
        let size = write_size(&cmd);
        let budget = crate::timeouts::write_budget(&cmd, size, 1.0)
            .map_err(|_| DeviceError::Message("timeout multiplier is out of range".into()))?;
        let program = self.program_for(rendered.tool);
        let command = CatalogueCommand::from_rendered(rendered);
        let result = self
            .run_tool(program, command, RunLimits::from_budget(&budget))
            .await;
        drop(locks);
        result
    }

    fn reverify_before_write(
        &self,
        cmd: &crate::cmd::WriteCmd,
    ) -> Result<Option<SharedReadLocks>, DeviceError> {
        if !self.writes_allowed() {
            return Err(DeviceError::Message(
                "G21 blocked: no verified platform-tools are installed".into(),
            ));
        }
        let guard = self.verified.lock().expect("verified tools");
        let Some(tools) = guard.as_ref() else {
            return Err(DeviceError::Message(
                "G21 blocked: no verified platform-tools are installed".into(),
            ));
        };
        let paths = vec![
            tools.adb.path().to_path_buf(),
            tools.fastboot.path().to_path_buf(),
        ];
        let mut locks =
            SharedReadLocks::hold(&paths).map_err(|err| DeviceError::Message(err.to_string()))?;
        let hashes = locks
            .hashes()
            .map_err(|err| DeviceError::Message(err.to_string()))?;
        if hashes.first().map(String::as_str) != Some(tools.adb.sha256())
            || hashes.get(1).map(String::as_str) != Some(tools.fastboot.sha256())
        {
            return Err(DeviceError::Message(
                "platform-tools changed on disk before the write".into(),
            ));
        }
        if cmd.uses_adb() {
            adb_server_matches(&tools.adb, tools.listener.as_ref())
                .map_err(DeviceError::Message)?;
        }
        Ok(Some(locks))
    }

    fn authorize(&self, token: &WriteToken, cmd: &crate::cmd::WriteCmd) -> Result<(), DeviceError> {
        if token.serial() != cmd.serial().as_str() || !crate::token::run_is_open(token.run_id()) {
            return Err(DeviceError::Message(
                "write token does not match this step".into(),
            ));
        }
        let mut guard = self.active.lock().expect("armed run");
        let Some(active) = guard.as_mut() else {
            return Err(DeviceError::Message("no confirmed plan is running".into()));
        };
        if token.plan_hash() != active.plan_hash
            || token.run_id() != active.run_id
            || token.serial() != active.serial
        {
            return Err(DeviceError::Message(
                "write token does not match this plan".into(),
            ));
        }
        let bytes = serde_jcs::to_vec(cmd).map_err(|err| DeviceError::Message(err.to_string()))?;
        if active.pending.pop_front().as_ref() != Some(&bytes) {
            return Err(DeviceError::Message(
                "write command is not the next plan step".into(),
            ));
        }
        Ok(())
    }

    pub fn config(&self) -> &TransportConfig {
        &self.config
    }

    pub async fn list(&self) -> Result<Vec<ScanEntry>, DeviceError> {
        let adb = self
            .run_read(ReadCmd::AdbHost(AdbHostRead::Devices))
            .await?;
        if !adb.success_exit() {
            return Err(command_failed("adb devices -l", &adb));
        }
        let fastboot = self
            .run_read(ReadCmd::Fastboot(FastbootRead::Devices))
            .await?;
        if !fastboot.success_exit() {
            return Err(command_failed("fastboot devices -l", &fastboot));
        }
        let adb_rows = parse_adb_devices(&adb.stdout_text());
        let fastboot_rows = parse_fastboot_devices(&format!(
            "{}{}",
            fastboot.stdout_text(),
            fastboot.stderr_text()
        ));
        Ok(merge_scans(adb_rows, fastboot_rows))
    }

    pub async fn state(&self, serial: &DeviceSerial) -> Result<Mode, DeviceError> {
        let result = self
            .run_read(ReadCmd::AdbHost(AdbHostRead::GetState {
                serial: serial.clone(),
            }))
            .await?;
        if result.success_exit() {
            let token = result.stdout_text();
            if let Some(mode) = parse_mode_token(token.trim()) {
                return Ok(mode);
            }
        }
        let listed = self
            .run_read(ReadCmd::Fastboot(FastbootRead::Devices))
            .await?;
        if !listed.success_exit() {
            return Err(command_failed("fastboot devices -l", &listed));
        }
        let rows =
            parse_fastboot_devices(&format!("{}{}", listed.stdout_text(), listed.stderr_text()));
        rows.into_iter()
            .find(|row| row.serial == serial.as_str())
            .map(|row| row.mode)
            .ok_or_else(|| DeviceError::NotConnected {
                serial: serial.as_str().to_string(),
            })
    }

    pub async fn getprops(
        &self,
        serial: &str,
    ) -> Result<crate::device::parse::PropMap, DeviceError> {
        let result = self
            .run_read(crate::cmd::ReadCmd::AdbShell(
                crate::cmd::AdbShellRead::GetpropAll {
                    serial: catalogue_serial(serial)?,
                },
            ))
            .await?;
        Ok(crate::device::parse::parse_getprop(&result.stdout_text()))
    }

    pub async fn getvar_all(
        &self,
        serial: &DeviceSerial,
    ) -> Result<crate::device::parse::PropMap, DeviceError> {
        let result = self
            .run_read(ReadCmd::Fastboot(FastbootRead::GetvarAll {
                serial: serial.clone(),
            }))
            .await?;
        if !result.success_exit() {
            return Err(command_failed("fastboot getvar all", &result));
        }
        Ok(crate::device::parse::parse_getvar(
            &result.stdout_text(),
            &result.stderr_text(),
        ))
    }

    pub async fn getvar(
        &self,
        serial: &DeviceSerial,
        var: FastbootVar,
    ) -> Result<Option<String>, DeviceError> {
        let name = var.as_str();
        let result = self
            .run_read(ReadCmd::Fastboot(FastbootRead::Getvar {
                serial: serial.clone(),
                var,
            }))
            .await?;
        if !result.success_exit() {
            return Err(command_failed("fastboot getvar", &result));
        }
        let vars = crate::device::parse::parse_getvar(&result.stdout_text(), &result.stderr_text());
        Ok(vars.get(&name).cloned())
    }

    pub async fn device_info(
        &self,
        serial: &str,
        aliases: &AliasTable,
        devices: &DeviceTable,
    ) -> Result<DeviceInfo, DeviceError> {
        let rows = self.list().await?;
        let entry = rows
            .into_iter()
            .find(|row| row.serial == serial)
            .ok_or_else(|| DeviceError::NotConnected {
                serial: serial.to_string(),
            })?;
        match entry.mode {
            Mode::Adb | Mode::Recovery | Mode::Sideload | Mode::Rescue => {
                self.info_from_adb(serial, entry.mode, entry.transport_id, aliases, devices)
                    .await
            }
            Mode::Fastboot | Mode::Fastbootd => {
                self.info_from_fastboot(serial, entry.mode, entry.transport_id, aliases, devices)
                    .await
            }
            Mode::Unauthorized | Mode::Authorizing | Mode::Offline | Mode::NoPermissions => {
                Err(DeviceError::NeedsUser {
                    message: entry
                        .mode
                        .guidance()
                        .unwrap_or("the phone needs attention before a scan can continue")
                        .to_string(),
                })
            }
            Mode::Unrecognized => Err(DeviceError::Message(format!(
                "device {serial} is in an unrecognised state ({})",
                entry.raw_state
            ))),
        }
    }

    pub async fn wait_for(
        &self,
        serial: &str,
        target: WaitTarget,
        timeout: Duration,
    ) -> Result<WaitOutcome, DeviceError> {
        let deadline = Instant::now() + timeout;
        loop {
            let mode = self.observe(serial).await?;
            if let Some(mode) = mode {
                if self.condition_met(serial, target, mode).await? {
                    return Ok(WaitOutcome::Reached);
                }
            }
            if Instant::now() >= deadline {
                return Ok(deadline_outcome(mode, target));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::time::sleep(self.config.poll_interval.min(remaining)).await;
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn reboot(
        &self,
        _token: &WriteToken,
        serial: &str,
        from: Mode,
        to: RebootTarget,
    ) -> Result<WaitOutcome, DeviceError> {
        let step = transition(&self.config, from, to)?;
        debug_assert!(!step.args.is_empty());
        let serial_ty = catalogue_serial(serial)?;
        let cmd = if step.use_adb {
            crate::cmd::WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Reboot {
                serial: serial_ty,
                mode: to,
            })
        } else {
            crate::cmd::WriteCmd::Fastboot(crate::cmd::FastbootWrite::Reboot {
                serial: serial_ty,
                mode: to,
            })
        };
        let result = self.run_write(_token, cmd).await?;
        let text = format!("{}{}", result.stdout_text(), result.stderr_text());
        let verdict = if step.use_adb {
            crate::parse::parse_adb_reboot(&text)
        } else {
            crate::parse::parse_fastboot_reboot(&text)
        };
        if !result.success_exit() || !matches!(verdict, crate::parse::Verdict::Ok) {
            return Err(command_failed("reboot", &result));
        }
        self.wait_for(serial, step.wait, step.timeout).await
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn fastboot_flash(
        &self,
        _token: &WriteToken,
        serial: &str,
        slot: Slot,
        partition: Partition,
        image: &Path,
    ) -> Result<(), DeviceError> {
        let serial_ty = catalogue_serial(serial)?;
        let cmd = crate::cmd::WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash {
            serial: serial_ty,
            slot,
            partition,
            image: crate::cmd::ImageRef::new(0, image.display().to_string(), image_len(image)),
        });
        let result = self.run_write(_token, cmd).await?;
        let text = format!("{}{}", result.stdout_text(), result.stderr_text());
        if result.success_exit()
            && crate::parse::parse_flash(&text, partition, slot) == crate::parse::Verdict::Ok
        {
            Ok(())
        } else {
            Err(command_failed("fastboot flash", &result))
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn fastboot_set_active(
        &self,
        token: &WriteToken,
        serial: &str,
        slot: Slot,
    ) -> Result<(), DeviceError> {
        let serial_ty = catalogue_serial(serial)?;
        let cmd = crate::cmd::WriteCmd::Fastboot(crate::cmd::FastbootWrite::SetActive {
            serial: serial_ty.clone(),
            slot,
        });
        let result = self.run_write(token, cmd).await?;
        let text = format!("{}{}", result.stdout_text(), result.stderr_text());
        if !result.success_exit()
            || crate::parse::parse_set_active(&text, slot) != crate::parse::Verdict::Ok
        {
            return Err(command_failed("fastboot set-active", &result));
        }
        let current = self
            .run_read(crate::cmd::ReadCmd::Fastboot(
                crate::cmd::FastbootRead::Getvar {
                    serial: serial_ty,
                    var: crate::cmd::FastbootVar::CurrentSlot,
                },
            ))
            .await?;
        let value = getvar_value(&current, "current-slot");
        if current.success_exit() && value.as_deref() == Some(slot.as_str()) {
            Ok(())
        } else {
            Err(command_failed("fastboot getvar current-slot", &current))
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn fastboot_update(
        &self,
        token: &WriteToken,
        serial: &str,
        slot: Slot,
        source: Slot,
        package: &Path,
    ) -> Result<(), DeviceError> {
        let serial_ty = catalogue_serial(serial)?;
        let cmd = crate::cmd::WriteCmd::Fastboot(crate::cmd::FastbootWrite::Update {
            serial: serial_ty,
            slot,
            package: crate::cmd::ImageRef::new(
                0,
                package.display().to_string(),
                image_len(package),
            ),
        });
        let result = self.run_write(token, cmd).await?;
        let text = format!("{}{}", result.stdout_text(), result.stderr_text());
        if result.success_exit()
            && crate::parse::parse_update(&text, slot, source) == crate::parse::Verdict::Ok
        {
            Ok(())
        } else {
            Err(command_failed("fastboot update", &result))
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn sideload(
        &self,
        token: &WriteToken,
        serial: &str,
        package: &Path,
        target: Slot,
        source: Slot,
    ) -> Result<(), DeviceError> {
        let serial_ty = catalogue_serial(serial)?;
        let cmd = crate::cmd::WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload {
            serial: serial_ty.clone(),
            package: crate::cmd::ImageRef::new(
                0,
                package.display().to_string(),
                image_len(package),
            ),
        });
        let result = self.run_write(token, cmd).await?;
        let text = format!("{}{}", result.stdout_text(), result.stderr_text());
        let verdict = crate::parse::parse_sideload(&text);
        if !result.success_exit()
            || !matches!(
                verdict,
                crate::parse::Verdict::Ok | crate::parse::Verdict::Uncertain { .. }
            )
        {
            return Err(command_failed("adb sideload", &result));
        }
        self.confirm_sideload_slot(&serial_ty, target, source).await
    }

    #[cfg_attr(not(test), allow(dead_code))]
    async fn confirm_sideload_slot(
        &self,
        serial: &crate::cmd::DeviceSerial,
        target: Slot,
        source: Slot,
    ) -> Result<(), DeviceError> {
        let current = self
            .run_read(crate::cmd::ReadCmd::Fastboot(
                crate::cmd::FastbootRead::Getvar {
                    serial: serial.clone(),
                    var: crate::cmd::FastbootVar::CurrentSlot,
                },
            ))
            .await?;
        let unbootable = self
            .run_read(crate::cmd::ReadCmd::Fastboot(
                crate::cmd::FastbootRead::Getvar {
                    serial: serial.clone(),
                    var: crate::cmd::FastbootVar::SlotUnbootable(target),
                },
            ))
            .await?;
        let current_slot = getvar_value(&current, "current-slot").unwrap_or_default();
        let unbootable_name = format!("slot-unbootable:{}", target.as_str());
        let unbootable_value = getvar_value(&unbootable, &unbootable_name).unwrap_or_default();
        match crate::parse::sideload_post_state(&current_slot, &unbootable_value, target, source) {
            crate::parse::Verdict::Ok => Ok(()),
            crate::parse::Verdict::Failed {
                reason: crate::parse::FailReason::OtaNotApplied,
            } => Err(DeviceError::Message(
                "the update did not change the slot".into(),
            )),
            _ => Err(DeviceError::Message(
                "sideload post-state check failed".into(),
            )),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn push(
        &self,
        token: &WriteToken,
        serial: &str,
        local: &Path,
        dst: crate::cmd::WorkFile,
    ) -> Result<(), DeviceError> {
        let serial_ty = catalogue_serial(serial)?;
        let cmd = crate::cmd::WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Push {
            serial: serial_ty,
            src: crate::cmd::HostRef::Image(crate::cmd::ImageRef::new(
                0,
                local.display().to_string(),
                image_len(local),
            )),
            dst,
        });
        let result = self.run_write(token, cmd).await?;
        let text = format!("{}{}", result.stdout_text(), result.stderr_text());
        if result.success_exit() && crate::parse::parse_push(&text) == crate::parse::Verdict::Ok {
            Ok(())
        } else {
            Err(command_failed("adb push", &result))
        }
    }

    async fn optional_read(&self, cmd: crate::cmd::ReadCmd) -> Result<Option<String>, DeviceError> {
        match self.run_read(cmd).await {
            Ok(result) if result.success_exit() => Ok(Some(result.stdout_text())),
            Ok(_) => Ok(None),
            Err(DeviceError::Process(crate::proc::ProcError::NoScript { .. })) => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn info_from_adb(
        &self,
        serial: &str,
        mode: Mode,
        transport_id: Option<String>,
        aliases: &AliasTable,
        devices: &DeviceTable,
    ) -> Result<DeviceInfo, DeviceError> {
        let serial_ty = catalogue_serial(serial)?;
        let props = self
            .run_read(crate::cmd::ReadCmd::AdbShell(
                crate::cmd::AdbShellRead::GetpropAll {
                    serial: serial_ty.clone(),
                },
            ))
            .await?;
        let su = self
            .run_read(crate::cmd::ReadCmd::Su(crate::cmd::SuRead::Id {
                serial: serial_ty.clone(),
            }))
            .await?;
        let magisk_version = self
            .optional_read(crate::cmd::ReadCmd::Su(crate::cmd::SuRead::MagiskVersion {
                serial: serial_ty.clone(),
            }))
            .await?;
        let magisk_code_text = self
            .optional_read(crate::cmd::ReadCmd::Su(
                crate::cmd::SuRead::MagiskVersionCode {
                    serial: serial_ty.clone(),
                },
            ))
            .await?;
        let dumpsys_package = self
            .optional_read(crate::cmd::ReadCmd::AdbShell(
                crate::cmd::AdbShellRead::DumpsysPackage {
                    serial: serial_ty.clone(),
                    package: crate::cmd::PackageName::magisk_app(),
                },
            ))
            .await?;
        let battery = self
            .optional_read(crate::cmd::ReadCmd::AdbShell(
                crate::cmd::AdbShellRead::DumpsysBattery {
                    serial: serial_ty.clone(),
                },
            ))
            .await?;
        let init_boot_ls = self
            .run_read(crate::cmd::ReadCmd::AdbShell(
                crate::cmd::AdbShellRead::LsBlockByName {
                    serial: serial_ty,
                    root: crate::cmd::ByNameRoot::ByName,
                    partition: Partition::InitBoot,
                    slot: Slot::A,
                },
            ))
            .await?;
        Ok(assemble_from_adb(
            serial,
            mode,
            transport_id,
            AdbTexts {
                props: props.stdout_text(),
                su,
                magisk_version,
                magisk_code_text,
                dumpsys_package,
                battery,
                init_boot_ls: Some(init_boot_ls),
            },
            aliases,
            devices,
        ))
    }

    async fn info_from_fastboot(
        &self,
        serial: &str,
        mode: Mode,
        transport_id: Option<String>,
        aliases: &AliasTable,
        devices: &DeviceTable,
    ) -> Result<DeviceInfo, DeviceError> {
        let serial_ty = catalogue_serial(serial)?;
        let result = self
            .run_read(ReadCmd::Fastboot(FastbootRead::GetvarAll {
                serial: serial_ty,
            }))
            .await?;
        if !result.success_exit() {
            return Err(command_failed("fastboot getvar all", &result));
        }
        Ok(assemble_from_fastboot(
            serial,
            mode,
            transport_id,
            &result.stdout_text(),
            &result.stderr_text(),
            aliases,
            devices,
        ))
    }

    async fn observe(&self, serial: &str) -> Result<Option<Mode>, DeviceError> {
        let rows = self.list().await?;
        Ok(rows
            .into_iter()
            .find(|row| row.serial == serial)
            .map(|row| row.mode))
    }

    async fn condition_met(
        &self,
        serial: &str,
        target: WaitTarget,
        mode: Mode,
    ) -> Result<bool, DeviceError> {
        match target {
            WaitTarget::FastbootFamily => Ok(mode.is_fastboot_family()),
            WaitTarget::Mode(expected) => Ok(mode == expected),
            WaitTarget::SystemBooted => {
                if mode != Mode::Adb {
                    return Ok(false);
                }
                Ok(self.boot_completed(serial).await?)
            }
        }
    }

    async fn boot_completed(&self, serial: &str) -> Result<bool, DeviceError> {
        let serial_ty = match catalogue_serial(serial) {
            Ok(serial) => serial,
            Err(_) => return Ok(false),
        };
        let name = match crate::cmd::PropName::try_from("sys.boot_completed") {
            Ok(name) => name,
            Err(_) => return Ok(false),
        };
        let result = self
            .run_read(crate::cmd::ReadCmd::AdbShell(
                crate::cmd::AdbShellRead::Getprop {
                    serial: serial_ty,
                    name,
                },
            ))
            .await?;
        if !result.success_exit() {
            return Ok(false);
        }
        Ok(result.stdout_text().trim() == "1")
    }

    fn program_for(&self, tool: crate::cmd::Tool) -> &Path {
        match tool {
            crate::cmd::Tool::Adb => &self.adb,
            crate::cmd::Tool::Fastboot => &self.fastboot,
        }
    }

    fn exe_for(&self, program: &Path) -> Result<VerifiedExe, DeviceError> {
        let guard = self.verified.lock().expect("verified tools");
        if let Some(tools) = guard.as_ref() {
            if tools.adb.path() == program {
                return Ok(tools.adb.clone());
            }
            if tools.fastboot.path() == program {
                return Ok(tools.fastboot.clone());
            }
        }
        drop(guard);
        resolve_tool(program).map_err(|err| DeviceError::Message(err.to_string()))
    }

    async fn run_tool(
        &self,
        program: &Path,
        command: CatalogueCommand,
        limits: RunLimits,
    ) -> Result<RunResult, DeviceError> {
        let exe = self.exe_for(program)?;
        if exe.trust() != crate::exe::Trust::Scripted {
            crate::exe::recheck_allow_list(&exe)
                .map_err(|err| DeviceError::Message(err.to_string()))?;
        }
        self.runner
            .run(&exe, &command, limits)
            .await
            .map_err(DeviceError::from)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
struct Transition {
    use_adb: bool,
    args: &'static [&'static str],
    wait: WaitTarget,
    timeout: Duration,
}

#[cfg_attr(not(test), allow(dead_code))]
fn transition(
    config: &TransportConfig,
    from: Mode,
    to: RebootTarget,
) -> Result<Transition, DeviceError> {
    let step = match (from, to) {
        (Mode::Adb, RebootTarget::Bootloader) => Transition {
            use_adb: true,
            args: &["reboot", "bootloader"],
            wait: WaitTarget::FastbootFamily,
            timeout: config.adb_to_bootloader,
        },
        (Mode::Adb, RebootTarget::Sideload) => Transition {
            use_adb: true,
            args: &["reboot", "sideload"],
            wait: WaitTarget::Mode(Mode::Sideload),
            timeout: config.adb_to_sideload,
        },
        (Mode::Fastboot | Mode::Fastbootd, RebootTarget::Bootloader) => Transition {
            use_adb: false,
            args: &["reboot", "bootloader"],
            wait: WaitTarget::FastbootFamily,
            timeout: config.bootloader_to_bootloader,
        },
        (Mode::Fastboot | Mode::Fastbootd, RebootTarget::System) => Transition {
            use_adb: false,
            args: &["reboot"],
            wait: WaitTarget::SystemBooted,
            timeout: config.bootloader_to_system,
        },
        (Mode::Sideload, RebootTarget::Bootloader) => Transition {
            use_adb: true,
            args: &["reboot", "bootloader"],
            wait: WaitTarget::FastbootFamily,
            timeout: config.sideload_to_bootloader,
        },
        _ => {
            return Err(DeviceError::UnsupportedTransition {
                from: from.to_string(),
                to: to.to_string(),
            });
        }
    };
    Ok(step)
}

fn deadline_outcome(mode: Option<Mode>, target: WaitTarget) -> WaitOutcome {
    match mode {
        None => WaitOutcome::Disappeared,
        Some(actual) => match target {
            WaitTarget::SystemBooted if actual == Mode::Adb => WaitOutcome::TimedOut,
            _ => WaitOutcome::WrongMode { actual },
        },
    }
}

fn command_failed(command: &str, result: &RunResult) -> DeviceError {
    DeviceError::CommandFailed {
        command: command.to_string(),
        exit: result.exit_code,
        detail: output_detail(result),
    }
}

fn output_detail(result: &RunResult) -> String {
    if result.timed_out {
        return "timed out".into();
    }
    let text = if result.stderr.is_empty() {
        result.stdout_text()
    } else {
        result.stderr_text()
    };
    let line = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("no output");
    let mut detail = line.trim().to_string();
    if detail.len() > 240 {
        detail.truncate(240);
    }
    detail
}

/// Probe up to four phones at once. Results follow `serials` order.
pub async fn probe_many<R>(
    transport: &PlatformToolsTransport<R>,
    serials: &[String],
    aliases: &AliasTable,
    devices: &DeviceTable,
) -> Vec<Result<DeviceInfo, DeviceError>>
where
    R: CommandRunner + 'static,
{
    let semaphore = Arc::new(Semaphore::new(MAX_PARALLEL_PROBES));
    let mut tasks = JoinSet::new();
    for (index, serial) in serials.iter().cloned().enumerate() {
        let transport = transport.clone();
        let aliases = aliases.clone();
        let devices = devices.clone();
        let semaphore = Arc::clone(&semaphore);
        tasks.spawn(async move {
            let permit = semaphore.acquire().await.expect("probe semaphore");
            let info = transport.device_info(&serial, &aliases, &devices).await;
            drop(permit);
            (index, info)
        });
    }
    let mut indexed = Vec::with_capacity(serials.len());
    while let Some(joined) = tasks.join_next().await {
        indexed.push(joined.expect("probe task"));
    }
    indexed.sort_by_key(|(index, _)| *index);
    indexed.into_iter().map(|(_, info)| info).collect()
}

/// Read operations a window can call.
///
/// There is no write trait. The only public write entry point is
/// [`crate::wizard::WizardSession::confirm_and_run`], which mints a
/// [`crate::WriteToken`] internally. Write methods on the transport stay
/// crate-private and take `&WriteToken`.
pub trait DeviceTransport: Send + Sync {
    fn list(&self)
        -> impl std::future::Future<Output = Result<Vec<ScanEntry>, DeviceError>> + Send;
    fn state(
        &self,
        serial: &DeviceSerial,
    ) -> impl std::future::Future<Output = Result<Mode, DeviceError>> + Send;
    fn device_info(
        &self,
        serial: &str,
        aliases: &AliasTable,
        devices: &DeviceTable,
    ) -> impl std::future::Future<Output = Result<DeviceInfo, DeviceError>> + Send;
    fn wait_for(
        &self,
        serial: &str,
        target: WaitTarget,
        timeout: Duration,
    ) -> impl std::future::Future<Output = Result<WaitOutcome, DeviceError>> + Send;
}

impl<R: CommandRunner> DeviceTransport for PlatformToolsTransport<R> {
    fn list(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<ScanEntry>, DeviceError>> + Send {
        PlatformToolsTransport::list(self)
    }

    fn state(
        &self,
        serial: &DeviceSerial,
    ) -> impl std::future::Future<Output = Result<Mode, DeviceError>> + Send {
        PlatformToolsTransport::state(self, serial)
    }

    fn device_info(
        &self,
        serial: &str,
        aliases: &AliasTable,
        devices: &DeviceTable,
    ) -> impl std::future::Future<Output = Result<DeviceInfo, DeviceError>> + Send {
        PlatformToolsTransport::device_info(self, serial, aliases, devices)
    }

    fn wait_for(
        &self,
        serial: &str,
        target: WaitTarget,
        timeout: Duration,
    ) -> impl std::future::Future<Output = Result<WaitOutcome, DeviceError>> + Send {
        PlatformToolsTransport::wait_for(self, serial, target, timeout)
    }
}

fn getvar_value(result: &RunResult, name: &str) -> Option<String> {
    crate::device::parse::parse_getvar(&result.stdout_text(), &result.stderr_text())
        .get(name)
        .cloned()
}

fn catalogue_serial(serial: &str) -> Result<crate::cmd::DeviceSerial, DeviceError> {
    crate::cmd::DeviceSerial::try_from(serial).map_err(|err| DeviceError::Message(err.to_string()))
}

#[cfg_attr(not(test), allow(dead_code))]
fn image_len(path: &Path) -> u64 {
    std::fs::metadata(path)
        .map(|meta| meta.len())
        .unwrap_or(8 * 1024 * 1024)
}

fn write_size(cmd: &crate::cmd::WriteCmd) -> u64 {
    match cmd {
        crate::cmd::WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash { image, .. }) => {
            image.size_bytes()
        }
        crate::cmd::WriteCmd::Fastboot(crate::cmd::FastbootWrite::Update { package, .. })
        | crate::cmd::WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload { package, .. }) => {
            package.size_bytes()
        }
        crate::cmd::WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Push { src, .. }) => match src {
            crate::cmd::HostRef::Image(image) => image.size_bytes(),
            crate::cmd::HostRef::Asset(_) => 0,
        },
        _ => 0,
    }
}
