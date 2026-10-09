// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Classify a Windows device node as a Google adb or bootloader interface.
//!
//! Phase 1 detects and explains a missing driver. It does not install one.
//! The VID/PID map is unverified until a SetupAPI capture is recorded.

mod classify;
mod table;

#[cfg(windows)]
mod host_windows;

pub use classify::{
    classify, ids_from_instance, DriverStatus, RawUsbDevice, UsbInterface, UsbReport,
    DRIVER_GUIDANCE,
};
pub use table::{UsbError, UsbTable};

/// Result of asking the host which Google devices are present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostProbe {
    Reports(Vec<UsbReport>),
    UnsupportedHost,
}

/// Enumerate present devices on Windows. Other hosts return [`HostProbe::UnsupportedHost`].
pub fn probe_host(table: &UsbTable) -> Result<HostProbe, UsbError> {
    #[cfg(windows)]
    {
        let raw = host_windows::enumerate_present()?;
        let reports = raw
            .iter()
            .filter_map(|device| classify(device, table))
            .collect();
        Ok(HostProbe::Reports(reports))
    }
    #[cfg(not(windows))]
    {
        let _ = table;
        Ok(HostProbe::UnsupportedHost)
    }
}
