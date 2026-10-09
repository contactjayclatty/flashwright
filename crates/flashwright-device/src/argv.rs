// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Argument vectors for adb and fastboot. Each call is one vector.
//! Slot arguments come from [`Slot`], which has no "all" variant.

use std::path::Path;

use crate::{DeviceError, Partition, Slot};

pub fn devices_long() -> Vec<String> {
    vec!["devices".into(), "-l".into()]
}

pub fn with_serial(serial: &str, tail: &[&str]) -> Vec<String> {
    let mut args = Vec::with_capacity(tail.len() + 2);
    args.push("-s".into());
    args.push(serial.into());
    args.extend(tail.iter().map(|part| (*part).to_string()));
    args
}

pub fn shell(serial: &str, remote: &[String]) -> Vec<String> {
    let mut args = vec!["-s".into(), serial.into(), "shell".into()];
    args.extend(remote.iter().cloned());
    args
}

pub fn flash_args(
    serial: &str,
    slot: Slot,
    partition: Partition,
    image: &Path,
) -> Result<Vec<String>, DeviceError> {
    if !partition.is_writable() {
        return Err(DeviceError::ReadOnlyPartition {
            partition: partition.to_string(),
        });
    }
    Ok(vec![
        "-s".into(),
        serial.into(),
        "--slot".into(),
        slot.as_str().into(),
        "flash".into(),
        partition.fastboot_name().into(),
        image.display().to_string(),
    ])
}

pub fn set_active_args(serial: &str, slot: Slot) -> Vec<String> {
    vec![
        "-s".into(),
        serial.into(),
        format!("--set-active={}", slot.as_str()),
    ]
}

pub fn update_args(serial: &str, slot: Slot, package: &Path) -> Vec<String> {
    vec![
        "-s".into(),
        serial.into(),
        "--slot".into(),
        slot.as_str().into(),
        "--skip-reboot".into(),
        "update".into(),
        package.display().to_string(),
    ]
}

pub fn contains_slot_all(args: &[String]) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == "--slot" && pair[1] == "all")
        || args
            .iter()
            .any(|arg| arg == "--slot=all" || arg.contains("--slot all"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn flash_names_one_slot_and_rejects_vbmeta() {
        let image = PathBuf::from("/tmp/patched.img");
        for slot in [Slot::A, Slot::B] {
            for partition in [
                Partition::Boot,
                Partition::InitBoot,
                Partition::Bootloader,
                Partition::Radio,
            ] {
                let args = flash_args("SERIAL", slot, partition, &image).unwrap();
                assert!(!contains_slot_all(&args));
                assert!(args
                    .windows(2)
                    .any(|pair| pair[0] == "--slot" && pair[1] == slot.as_str()));
            }
        }
        let err = flash_args("SERIAL", Slot::A, Partition::Vbmeta, &image).unwrap_err();
        assert!(matches!(err, DeviceError::ReadOnlyPartition { .. }));
    }
}
