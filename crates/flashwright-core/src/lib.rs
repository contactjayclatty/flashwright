// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Session façade for the device layer.
//!
//! This crate selects tools, scans, and reads one phone. It does not flash,
//! and it does not own the wizard. A blocked platform-tools build is refused
//! before any device command runs.

use std::path::Path;
use std::sync::Arc;

use flashwright_device::{
    AliasTable, DeviceInfo, DeviceTable, PlatformToolsTransport, ScanEntry, TransportConfig,
};
use flashwright_proc::CommandRunner;
use flashwright_tools::{
    assess_server, evaluate_installation, probe_adb_server, restart_adb_server, HostKind,
    PlatformToolsPolicy, ServerStatus, ToolBinaryNames, ToolsError, ToolsReport, ToolsVerdict,
    DEFAULT_ADB_PORT,
};
use flashwright_winusb::{classify, probe_host, HostProbe, RawUsbDevice, UsbReport, UsbTable};
use thiserror::Error;
use tokio::sync::broadcast;

const EVENT_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("platform-tools have not been located")]
    ToolsMissing,

    #[error("no device is selected")]
    NoSelection,

    #[error("{0}")]
    ToolsBlocked(String),

    #[error("device {serial} is not in the latest scan")]
    NotInScan { serial: String },

    #[error(transparent)]
    Tools(#[from] ToolsError),

    #[error(transparent)]
    Device(#[from] flashwright_device::DeviceError),

    #[error(transparent)]
    Usb(#[from] flashwright_winusb::UsbError),
}

/// Minimal events for a later UI. `v` is the payload version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    Log { v: u32, message: String },
    DeviceChanged { v: u32, serial: String },
}

pub struct Session<R: CommandRunner> {
    runner: Arc<R>,
    config: TransportConfig,
    policy: PlatformToolsPolicy,
    names: ToolBinaryNames,
    aliases: AliasTable,
    devices: DeviceTable,
    usb: UsbTable,
    transport: Option<PlatformToolsTransport<R>>,
    report: Option<ToolsReport>,
    selected: Option<String>,
    events: broadcast::Sender<EngineEvent>,
}

impl<R: CommandRunner + 'static> Session<R> {
    pub fn new(runner: Arc<R>) -> Result<Self, CoreError> {
        Self::with_config(runner, TransportConfig::production())
    }

    pub fn with_config(runner: Arc<R>, config: TransportConfig) -> Result<Self, CoreError> {
        let (events, _) = broadcast::channel(32);
        Ok(Self {
            runner,
            config,
            policy: PlatformToolsPolicy::embedded()?,
            names: ToolBinaryNames::for_host(),
            aliases: AliasTable::embedded()?,
            devices: DeviceTable::embedded()?,
            usb: UsbTable::embedded()?,
            transport: None,
            report: None,
            selected: None,
            events,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    pub fn tools(&self) -> Option<&ToolsReport> {
        self.report.as_ref()
    }

    pub fn allows_writes(&self) -> bool {
        self.report
            .as_ref()
            .is_some_and(|report| report.verdict.allows_writes())
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub async fn locate_tools(
        &mut self,
        dir: &Path,
        host: HostKind,
    ) -> Result<ToolsReport, CoreError> {
        let report =
            evaluate_installation(dir, &self.names, &self.policy, self.runner.as_ref(), host)
                .await?;
        self.transport = Some(PlatformToolsTransport::new(
            Arc::clone(&self.runner),
            report.adb.clone(),
            report.fastboot.clone(),
            self.config.clone(),
        ));
        let _ = self.events.send(EngineEvent::Log {
            v: EVENT_VERSION,
            message: format!(
                "platform-tools {} ({})",
                report.version,
                verdict_label(&report.verdict)
            ),
        });
        self.report = Some(report.clone());
        Ok(report)
    }

    /// Scan phones. A missing or block-listed tool set fails before adb runs.
    pub async fn scan(&self) -> Result<Vec<ScanEntry>, CoreError> {
        let report = self.report.as_ref().ok_or(CoreError::ToolsMissing)?;
        if !report.verdict.allows_scan() {
            return Err(CoreError::ToolsBlocked(
                report.verdict.message().to_string(),
            ));
        }
        let transport = self.transport.as_ref().ok_or(CoreError::ToolsMissing)?;
        Ok(transport.list().await?)
    }

    pub fn select_device(&mut self, serial: &str, scan: &[ScanEntry]) -> Result<(), CoreError> {
        if !scan.iter().any(|entry| entry.serial == serial) {
            return Err(CoreError::NotInScan {
                serial: serial.to_string(),
            });
        }
        self.selected = Some(serial.to_string());
        let _ = self.events.send(EngineEvent::DeviceChanged {
            v: EVENT_VERSION,
            serial: serial.to_string(),
        });
        Ok(())
    }

    pub async fn device_info(&self) -> Result<DeviceInfo, CoreError> {
        let serial = self.selected.as_deref().ok_or(CoreError::NoSelection)?;
        let transport = self.transport.as_ref().ok_or(CoreError::ToolsMissing)?;
        Ok(transport
            .device_info(serial, &self.aliases, &self.devices)
            .await?)
    }

    pub fn assess_drivers(&self, raws: &[RawUsbDevice]) -> Vec<UsbReport> {
        raws.iter()
            .filter_map(|raw| classify(raw, &self.usb))
            .collect()
    }

    pub fn probe_drivers(&self) -> Result<HostProbe, CoreError> {
        Ok(probe_host(&self.usb)?)
    }

    pub async fn check_adb_server(&self) -> Result<ServerStatus, CoreError> {
        let report = self.report.as_ref().ok_or(CoreError::ToolsMissing)?;
        let observation = probe_adb_server(DEFAULT_ADB_PORT).await;
        Ok(assess_server(&report.version, &observation))
    }

    pub async fn confirm_adb_restart(&self, user_confirmed: bool) -> Result<(), CoreError> {
        let report = self.report.as_ref().ok_or(CoreError::ToolsMissing)?;
        restart_adb_server(self.runner.as_ref(), &report.adb, user_confirmed).await?;
        Ok(())
    }
}

fn verdict_label(verdict: &ToolsVerdict) -> &'static str {
    if verdict.allows_writes() {
        "allowed"
    } else if verdict.allows_scan() {
        "scan only"
    } else {
        "blocked"
    }
}
