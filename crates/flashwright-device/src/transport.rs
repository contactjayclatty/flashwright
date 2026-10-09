// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Talks to one phone through adb and fastboot.
//!
//! Read methods take a serial. Write methods also take a [`WriteToken`].
//! A missing device during a reboot is not treated as unplugged until the
//! wait deadline: the phone disappears from `adb devices` while it reboots.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use flashwright_proc::{fastboot_flash_ok, CommandRunner, Invocation, RunResult};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::argv::{self, devices_long, shell};
use crate::catalog::{AliasTable, DeviceTable};
use crate::info::{assemble_from_adb, assemble_from_fastboot, AdbTexts};
use crate::parse::{merge_scans, parse_adb_devices, parse_fastboot_devices, parse_mode_token};
use crate::{
    DeviceError, DeviceInfo, Mode, Partition, RebootTarget, ScanEntry, Slot, TransportConfig,
    WaitOutcome, WaitTarget, WriteToken, MAX_PARALLEL_PROBES,
};

/// Concrete transport over a [`CommandRunner`].
pub struct PlatformToolsTransport<R: CommandRunner> {
    runner: Arc<R>,
    adb: PathBuf,
    fastboot: PathBuf,
    config: TransportConfig,
}

impl<R: CommandRunner> Clone for PlatformToolsTransport<R> {
    fn clone(&self) -> Self {
        Self {
            runner: Arc::clone(&self.runner),
            adb: self.adb.clone(),
            fastboot: self.fastboot.clone(),
            config: self.config.clone(),
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
        }
    }

    pub fn config(&self) -> &TransportConfig {
        &self.config
    }

    pub async fn list(&self) -> Result<Vec<ScanEntry>, DeviceError> {
        let adb = self
            .run(&self.adb, devices_long(), self.config.command_timeout)
            .await?;
        if !adb.success_exit() {
            return Err(command_failed("adb devices -l", &adb));
        }
        let fastboot = self
            .run(&self.fastboot, devices_long(), self.config.command_timeout)
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

    pub async fn state(&self, serial: &str) -> Result<Mode, DeviceError> {
        let args = argv::with_serial(serial, &["get-state"]);
        let result = self
            .run(&self.adb, args, self.config.command_timeout)
            .await?;
        if result.success_exit() {
            let token = result.stdout_text();
            if let Some(mode) = parse_mode_token(token.trim()) {
                return Ok(mode);
            }
        }
        let listed = self
            .run(&self.fastboot, devices_long(), self.config.command_timeout)
            .await?;
        if !listed.success_exit() {
            return Err(command_failed("fastboot devices -l", &listed));
        }
        let rows =
            parse_fastboot_devices(&format!("{}{}", listed.stdout_text(), listed.stderr_text()));
        rows.into_iter()
            .find(|row| row.serial == serial)
            .map(|row| row.mode)
            .ok_or_else(|| DeviceError::NotConnected {
                serial: serial.to_string(),
            })
    }

    pub async fn getprops(&self, serial: &str) -> Result<crate::parse::PropMap, DeviceError> {
        let result = self.shell_required(serial, &["getprop".into()]).await?;
        Ok(crate::parse::parse_getprop(&result.stdout_text()))
    }

    pub async fn getvar_all(&self, serial: &str) -> Result<crate::parse::PropMap, DeviceError> {
        let args = argv::with_serial(serial, &["getvar", "all"]);
        let result = self
            .run(&self.fastboot, args, self.config.prop_timeout)
            .await?;
        if !result.success_exit() {
            return Err(command_failed("fastboot getvar all", &result));
        }
        Ok(crate::parse::parse_getvar(
            &result.stdout_text(),
            &result.stderr_text(),
        ))
    }

    pub async fn getvar(&self, serial: &str, name: &str) -> Result<Option<String>, DeviceError> {
        let args = argv::with_serial(serial, &["getvar", name]);
        let result = self
            .run(&self.fastboot, args, self.config.prop_timeout)
            .await?;
        if !result.success_exit() {
            return Err(command_failed("fastboot getvar", &result));
        }
        let vars = crate::parse::parse_getvar(&result.stdout_text(), &result.stderr_text());
        Ok(vars.get(name).cloned())
    }

    pub async fn shell_read(
        &self,
        serial: &str,
        argv: &[String],
    ) -> Result<RunResult, DeviceError> {
        let args = shell(serial, argv);
        self.run(&self.adb, args, self.config.command_timeout).await
    }

    /// `adb exec-out`. The bytes are the full stdout buffer.
    ///
    /// The process runner already streams the child pipes. This read-class
    /// helper returns them as one buffer so callers do not have to own a stream.
    pub async fn exec_out_read(
        &self,
        serial: &str,
        argv: &[String],
    ) -> Result<Vec<u8>, DeviceError> {
        let mut args = vec!["-s".into(), serial.into(), "exec-out".into()];
        args.extend(argv.iter().cloned());
        let result = self
            .run(&self.adb, args, self.config.command_timeout)
            .await?;
        if !result.success_exit() {
            return Err(command_failed("adb exec-out", &result));
        }
        Ok(result.stdout)
    }

    pub async fn pull(&self, serial: &str, remote: &str, local: &Path) -> Result<(), DeviceError> {
        let args = vec![
            "-s".into(),
            serial.into(),
            "pull".into(),
            remote.into(),
            local.display().to_string(),
        ];
        let result = self
            .run(&self.adb, args, self.config.command_timeout)
            .await?;
        if !result.success_exit() {
            return Err(command_failed("adb pull", &result));
        }
        Ok(())
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
            Mode::Unauthorized | Mode::Offline | Mode::NoPermissions => {
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

    pub async fn reboot(
        &self,
        _token: &WriteToken,
        serial: &str,
        from: Mode,
        to: RebootTarget,
    ) -> Result<WaitOutcome, DeviceError> {
        let step = transition(&self.config, from, to)?;
        let program = if step.use_adb {
            &self.adb
        } else {
            &self.fastboot
        };
        let args = argv::with_serial(serial, step.args);
        let result = self.run(program, args, self.config.command_timeout).await?;
        if !result.success_exit() {
            return Err(command_failed("reboot", &result));
        }
        self.wait_for(serial, step.wait, step.timeout).await
    }

    pub async fn fastboot_flash(
        &self,
        _token: &WriteToken,
        serial: &str,
        slot: Slot,
        partition: Partition,
        image: &Path,
    ) -> Result<(), DeviceError> {
        let args = argv::flash_args(serial, slot, partition, image)?;
        let result = self
            .run(&self.fastboot, args, self.config.command_timeout)
            .await?;
        if fastboot_flash_ok(&result) {
            Ok(())
        } else {
            Err(command_failed("fastboot flash", &result))
        }
    }

    pub async fn fastboot_set_active(
        &self,
        _token: &WriteToken,
        serial: &str,
        slot: Slot,
    ) -> Result<(), DeviceError> {
        let args = argv::set_active_args(serial, slot);
        self.fastboot_ok("fastboot set-active", args).await
    }

    pub async fn fastboot_update(
        &self,
        _token: &WriteToken,
        serial: &str,
        slot: Slot,
        package: &Path,
    ) -> Result<(), DeviceError> {
        let args = argv::update_args(serial, slot, package);
        self.fastboot_ok("fastboot update", args).await
    }

    pub async fn shell_write(
        &self,
        _token: &WriteToken,
        serial: &str,
        argv: &[String],
    ) -> Result<(), DeviceError> {
        let result = self.shell_read(serial, argv).await?;
        if result.success_exit() {
            Ok(())
        } else {
            Err(command_failed("adb shell", &result))
        }
    }

    pub async fn sideload(
        &self,
        _token: &WriteToken,
        serial: &str,
        package: &Path,
    ) -> Result<(), DeviceError> {
        let args = vec![
            "-s".into(),
            serial.into(),
            "sideload".into(),
            package.display().to_string(),
        ];
        let result = self
            .run(&self.adb, args, self.config.command_timeout)
            .await?;
        if result.success_exit() {
            Ok(())
        } else {
            Err(command_failed("adb sideload", &result))
        }
    }

    pub async fn push(
        &self,
        _token: &WriteToken,
        serial: &str,
        local: &Path,
        remote: &str,
    ) -> Result<(), DeviceError> {
        let args = vec![
            "-s".into(),
            serial.into(),
            "push".into(),
            local.display().to_string(),
            remote.into(),
        ];
        let result = self
            .run(&self.adb, args, self.config.command_timeout)
            .await?;
        if result.success_exit() {
            Ok(())
        } else {
            Err(command_failed("adb push", &result))
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
        let props = self.shell_required(serial, &["getprop".into()]).await?;
        let su_args = shell(serial, &["su".into(), "-c".into(), "id".into()]);
        let su = self.run(&self.adb, su_args, self.config.su_timeout).await?;
        let magisk_version = self
            .optional_shell(serial, &["magisk".into(), "-v".into()])
            .await?;
        let magisk_code_text = self
            .optional_shell(serial, &["magisk".into(), "-V".into()])
            .await?;
        let dumpsys_package = self
            .optional_shell(
                serial,
                &[
                    "dumpsys".into(),
                    "package".into(),
                    "com.topjohnwu.magisk".into(),
                ],
            )
            .await?;
        let battery = self
            .optional_shell(serial, &["dumpsys".into(), "battery".into()])
            .await?;
        let init_boot_ls = self
            .run(
                &self.adb,
                shell(
                    serial,
                    &["ls".into(), "/dev/block/by-name/init_boot_a".into()],
                ),
                self.config.command_timeout,
            )
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
        let args = argv::with_serial(serial, &["getvar", "all"]);
        let result = self
            .run(&self.fastboot, args, self.config.prop_timeout)
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

    async fn shell_required(
        &self,
        serial: &str,
        argv: &[String],
    ) -> Result<RunResult, DeviceError> {
        let result = self
            .run(&self.adb, shell(serial, argv), self.config.prop_timeout)
            .await?;
        if result.success_exit() {
            Ok(result)
        } else {
            Err(command_failed("adb shell getprop", &result))
        }
    }

    async fn optional_shell(
        &self,
        serial: &str,
        argv: &[String],
    ) -> Result<Option<String>, DeviceError> {
        match self.optional_run(shell(serial, argv)).await? {
            Some(result) if result.success_exit() => Ok(Some(result.stdout_text())),
            _ => Ok(None),
        }
    }

    async fn optional_run(&self, args: Vec<String>) -> Result<Option<RunResult>, DeviceError> {
        let result = self
            .run(&self.adb, args, self.config.command_timeout)
            .await?;
        Ok(Some(result))
    }

    async fn fastboot_ok(&self, command: &str, args: Vec<String>) -> Result<(), DeviceError> {
        let result = self
            .run(&self.fastboot, args, self.config.command_timeout)
            .await?;
        if fastboot_flash_ok(&result) {
            Ok(())
        } else {
            Err(command_failed(command, &result))
        }
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
        let args = shell(serial, &["getprop".into(), "sys.boot_completed".into()]);
        let result = self.run(&self.adb, args, self.config.prop_timeout).await?;
        if !result.success_exit() {
            return Ok(false);
        }
        Ok(result.stdout_text().trim() == "1")
    }

    async fn run(
        &self,
        program: &Path,
        args: Vec<String>,
        timeout: Duration,
    ) -> Result<RunResult, DeviceError> {
        self.runner
            .run(Invocation::tied(program, args, timeout))
            .await
            .map_err(DeviceError::from)
    }
}

struct Transition {
    use_adb: bool,
    args: &'static [&'static str],
    wait: WaitTarget,
    timeout: Duration,
}

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

/// Read and write operations the session façade is built on.
pub trait DeviceTransport: Send + Sync {
    fn list(&self)
        -> impl std::future::Future<Output = Result<Vec<ScanEntry>, DeviceError>> + Send;
    fn state(
        &self,
        serial: &str,
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
    fn reboot(
        &self,
        token: &WriteToken,
        serial: &str,
        from: Mode,
        to: RebootTarget,
    ) -> impl std::future::Future<Output = Result<WaitOutcome, DeviceError>> + Send;
    fn fastboot_flash(
        &self,
        token: &WriteToken,
        serial: &str,
        slot: Slot,
        partition: Partition,
        image: &Path,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    fn fastboot_set_active(
        &self,
        token: &WriteToken,
        serial: &str,
        slot: Slot,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    fn fastboot_update(
        &self,
        token: &WriteToken,
        serial: &str,
        slot: Slot,
        package: &Path,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    fn shell_write(
        &self,
        token: &WriteToken,
        serial: &str,
        argv: &[String],
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    fn sideload(
        &self,
        token: &WriteToken,
        serial: &str,
        package: &Path,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
    fn push(
        &self,
        token: &WriteToken,
        serial: &str,
        local: &Path,
        remote: &str,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send;
}

impl<R: CommandRunner> DeviceTransport for PlatformToolsTransport<R> {
    fn list(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<ScanEntry>, DeviceError>> + Send {
        PlatformToolsTransport::list(self)
    }

    fn state(
        &self,
        serial: &str,
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

    fn reboot(
        &self,
        token: &WriteToken,
        serial: &str,
        from: Mode,
        to: RebootTarget,
    ) -> impl std::future::Future<Output = Result<WaitOutcome, DeviceError>> + Send {
        PlatformToolsTransport::reboot(self, token, serial, from, to)
    }

    fn fastboot_flash(
        &self,
        token: &WriteToken,
        serial: &str,
        slot: Slot,
        partition: Partition,
        image: &Path,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send {
        PlatformToolsTransport::fastboot_flash(self, token, serial, slot, partition, image)
    }

    fn fastboot_set_active(
        &self,
        token: &WriteToken,
        serial: &str,
        slot: Slot,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send {
        PlatformToolsTransport::fastboot_set_active(self, token, serial, slot)
    }

    fn fastboot_update(
        &self,
        token: &WriteToken,
        serial: &str,
        slot: Slot,
        package: &Path,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send {
        PlatformToolsTransport::fastboot_update(self, token, serial, slot, package)
    }

    fn shell_write(
        &self,
        token: &WriteToken,
        serial: &str,
        argv: &[String],
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send {
        PlatformToolsTransport::shell_write(self, token, serial, argv)
    }

    fn sideload(
        &self,
        token: &WriteToken,
        serial: &str,
        package: &Path,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send {
        PlatformToolsTransport::sideload(self, token, serial, package)
    }

    fn push(
        &self,
        token: &WriteToken,
        serial: &str,
        local: &Path,
        remote: &str,
    ) -> impl std::future::Future<Output = Result<(), DeviceError>> + Send {
        PlatformToolsTransport::push(self, token, serial, local, remote)
    }
}
