// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Choose init_boot or boot. komodo is an init_boot device.

use crate::error::FirmwareError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StockPartition {
    InitBoot,
    Boot,
}

impl StockPartition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InitBoot => "init_boot",
            Self::Boot => "boot",
        }
    }

    pub fn file_name(self) -> String {
        format!("{}.img", self.as_str())
    }
}

pub fn select_partition(
    names: &[String],
    device_has_init_boot: Option<bool>,
) -> Result<StockPartition, FirmwareError> {
    let has_init = names.iter().any(|name| name == "init_boot");
    let has_boot = names.iter().any(|name| name == "boot");
    match device_has_init_boot {
        Some(true) => {
            if has_init {
                Ok(StockPartition::InitBoot)
            } else if has_boot {
                Err(FirmwareError::PartitionMismatch)
            } else {
                Err(FirmwareError::NoBootImage)
            }
        }
        Some(false) => {
            if has_init {
                Err(FirmwareError::PartitionMismatch)
            } else if has_boot {
                Ok(StockPartition::Boot)
            } else {
                Err(FirmwareError::NoBootImage)
            }
        }
        None => Err(FirmwareError::UnknownDevice),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn known_init_boot_device_requires_init_boot() {
        assert_eq!(
            select_partition(&names(&["boot", "init_boot"]), Some(true)).unwrap(),
            StockPartition::InitBoot
        );
        assert!(matches!(
            select_partition(&names(&["boot"]), Some(true)),
            Err(FirmwareError::PartitionMismatch)
        ));
    }

    #[test]
    fn older_device_requires_boot() {
        assert_eq!(
            select_partition(&names(&["boot"]), Some(false)).unwrap(),
            StockPartition::Boot
        );
        assert!(matches!(
            select_partition(&names(&["boot", "init_boot"]), Some(false)),
            Err(FirmwareError::PartitionMismatch)
        ));
    }

    #[test]
    fn unknown_device_is_blocked() {
        assert!(matches!(
            select_partition(&names(&["boot", "init_boot"]), None),
            Err(FirmwareError::UnknownDevice)
        ));
        assert!(matches!(
            select_partition(&names(&["boot"]), None),
            Err(FirmwareError::UnknownDevice)
        ));
        assert!(FirmwareError::UnknownDevice.to_string().contains("G24"));
    }
}
