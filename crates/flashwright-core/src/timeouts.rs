// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Size-aware timeouts. The formula is `max(min, base + ceil(size / floor))`.
//! A multiplier in `1.0..=4.0` can only lengthen the result.

use std::time::Duration;

use serde::Deserialize;

use crate::cmd::{ExecOutSuRead, ReadCmd, SuRead, WriteCmd};
use crate::device::Partition;

const TABLE: &str = include_str!("../../../data/timeouts.toml");

#[derive(Clone, Debug, PartialEq)]
pub struct StepBudget {
    pub timeout: Duration,
    pub watchdog: Option<Duration>,
    /// Quiet window after a sideload percent line. Other steps leave this empty.
    pub finalising: Option<Duration>,
}

#[derive(Debug, Deserialize)]
struct File {
    read: ReadRow,
    reboot: FixedRow,
    set_active: FixedRow,
    flash_boot: SizedRow,
    flash_bootloader: SizedRow,
    flash_radio: SizedRow,
    update: SizedRow,
    sideload: SideloadRow,
    push: SizedRow,
    pull: SizedRow,
    patch_script: FixedRow,
}

#[derive(Debug, Deserialize)]
struct ReadRow {
    shell_s: u64,
    su_id_s: u64,
}

#[derive(Debug, Deserialize)]
struct FixedRow {
    base_s: u64,
    min_s: u64,
    watchdog_s: u64,
}

#[derive(Debug, Deserialize)]
struct SizedRow {
    base_s: u64,
    floor_bps: u64,
    min_s: u64,
    watchdog_s: u64,
}

#[derive(Debug, Deserialize)]
struct SideloadRow {
    base_s: u64,
    floor_bps: u64,
    min_s: u64,
    watchdog_s: u64,
    finalising_s: u64,
}

pub fn read_budget(cmd: &ReadCmd) -> StepBudget {
    if is_block_read(cmd) {
        return pull_budget(0, 1.0).expect("pull multiplier");
    }
    let file = load();
    let seconds = if matches!(cmd, ReadCmd::Su(SuRead::Id { .. })) {
        file.read.su_id_s
    } else {
        file.read.shell_s
    };
    StepBudget {
        timeout: Duration::from_secs(seconds),
        watchdog: None,
        finalising: None,
    }
}

fn is_block_read(cmd: &ReadCmd) -> bool {
    matches!(
        cmd,
        ReadCmd::ExecOutSu(ExecOutSuRead::CatBlock { .. })
            | ReadCmd::Su(SuRead::Sha256Block { .. } | SuRead::Sha256BlockPrefix { .. })
    )
}

/// Uncompressed `.img` members of an update zip. `None` when the path is not a zip.
pub fn uncompressed_image_bytes(path: &std::path::Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut total = 0u64;
    let mut saw_image = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).ok()?;
        let Ok(name) = entry.name() else {
            continue;
        };
        if name.to_ascii_lowercase().ends_with(".img") {
            saw_image = true;
            total = total.saturating_add(entry.size());
        }
    }
    saw_image.then_some(total)
}

/// Bytes the update timeout uses: the uncompressed images when the package is a zip.
pub fn update_size_bytes(path: &std::path::Path, declared: u64) -> u64 {
    uncompressed_image_bytes(path).unwrap_or(declared)
}

pub fn write_budget(
    cmd: &WriteCmd,
    size_bytes: u64,
    multiplier: f64,
) -> Result<StepBudget, TimeoutError> {
    let scale = check_multiplier(multiplier)?;
    let file = load();
    let (timeout_s, watchdog_s) = match cmd {
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Flash { partition, .. }) => {
            let row = match partition {
                Partition::Boot | Partition::InitBoot => &file.flash_boot,
                Partition::Bootloader => &file.flash_bootloader,
                Partition::Radio => &file.flash_radio,
                Partition::Vbmeta => &file.flash_boot,
            };
            (sized(row, size_bytes), row.watchdog_s)
        }
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::Update { .. }) => {
            (sized(&file.update, size_bytes), file.update.watchdog_s)
        }
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload { .. }) => (
            sized_parts(
                file.sideload.base_s,
                file.sideload.floor_bps,
                file.sideload.min_s,
                size_bytes,
            ),
            file.sideload.watchdog_s,
        ),
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Push { .. }) => {
            (sized(&file.push, size_bytes), file.push.watchdog_s)
        }
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Reboot { .. })
        | WriteCmd::Fastboot(crate::cmd::FastbootWrite::Reboot { .. }) => {
            (fixed(&file.reboot), file.reboot.watchdog_s)
        }
        WriteCmd::Fastboot(crate::cmd::FastbootWrite::SetActive { .. }) => {
            (fixed(&file.set_active), file.set_active.watchdog_s)
        }
        WriteCmd::AdbShell(_) | WriteCmd::Su(_) => {
            (fixed(&file.patch_script), file.patch_script.watchdog_s)
        }
    };
    let finalising = match cmd {
        WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload { .. }) => {
            Some(Duration::from_secs(file.sideload.finalising_s))
        }
        _ => None,
    };
    let scaled = scale_seconds(timeout_s, scale);
    Ok(StepBudget {
        timeout: Duration::from_secs(scaled),
        watchdog: Some(Duration::from_secs(watchdog_s)),
        finalising,
    })
}

pub fn sideload_finalising() -> Duration {
    Duration::from_secs(load().sideload.finalising_s)
}

pub fn pull_budget(size_bytes: u64, multiplier: f64) -> Result<StepBudget, TimeoutError> {
    let scale = check_multiplier(multiplier)?;
    let row = &load().pull;
    Ok(StepBudget {
        timeout: Duration::from_secs(scale_seconds(sized(row, size_bytes), scale)),
        watchdog: Some(Duration::from_secs(row.watchdog_s)),
        finalising: None,
    })
}

pub fn compute_timeout(base_s: u64, floor_bps: u64, min_s: u64, size_bytes: u64) -> u64 {
    sized_parts(base_s, floor_bps, min_s, size_bytes)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeoutError;

fn check_multiplier(multiplier: f64) -> Result<f64, TimeoutError> {
    if (1.0..=4.0).contains(&multiplier) {
        Ok(multiplier)
    } else {
        Err(TimeoutError)
    }
}

fn scale_seconds(seconds: u64, multiplier: f64) -> u64 {
    let scaled = (seconds as f64) * multiplier;
    scaled.ceil() as u64
}

fn fixed(row: &FixedRow) -> u64 {
    row.min_s.max(row.base_s)
}

fn sized(row: &SizedRow, size_bytes: u64) -> u64 {
    sized_parts(row.base_s, row.floor_bps, row.min_s, size_bytes)
}

fn sized_parts(base_s: u64, floor_bps: u64, min_s: u64, size_bytes: u64) -> u64 {
    let extra = if floor_bps == 0 {
        0
    } else {
        size_bytes.div_ceil(floor_bps)
    };
    min_s.max(base_s.saturating_add(extra))
}

fn load() -> File {
    toml::from_str(TABLE).expect("timeouts.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_match_the_table() {
        let mib = 1024u64 * 1024;
        assert_eq!(compute_timeout(60, 4 * mib, 90, 8 * mib), 90);
        assert_eq!(compute_timeout(60, 4 * mib, 90, 64 * mib), 90);
        assert_eq!(compute_timeout(120, 4 * mib, 180, 15 * mib), 180);
        assert_eq!(compute_timeout(120, 4 * mib, 180, 110 * mib), 180);
        assert_eq!(compute_timeout(600, 3 * mib, 900, 4 * 1024 * mib), 1966);
        assert_eq!(
            compute_timeout(900, 2 * mib, 1200, (5 * 1024 * mib) / 2),
            2180
        );
        assert_eq!(compute_timeout(30, 4 * mib, 30, 64 * mib), 46);
        assert_eq!(compute_timeout(30, 4 * mib, 60, 64 * mib), 60);
        assert_eq!(compute_timeout(30, 0, 30, 0), 30);
    }

    #[test]
    fn block_reads_wait_at_least_a_minute_and_have_a_watchdog() {
        use crate::cmd::{ByNameRoot, DeviceSerial, ExecOutSuRead, ReadCmd, SuRead};
        use crate::device::{Partition, Slot};

        let serial = DeviceSerial::try_from("synth-komodo-1").unwrap();
        for cmd in [
            ReadCmd::ExecOutSu(ExecOutSuRead::CatBlock {
                serial: serial.clone(),
                root: ByNameRoot::ByName,
                partition: Partition::InitBoot,
                slot: Slot::A,
            }),
            ReadCmd::Su(SuRead::Sha256Block {
                serial: serial.clone(),
                root: ByNameRoot::ByName,
                partition: Partition::Vbmeta,
                slot: Slot::B,
            }),
        ] {
            let budget = read_budget(&cmd);
            assert!(budget.timeout.as_secs() >= 60);
            assert!(budget.watchdog.is_some_and(|wait| wait.as_secs() >= 60));
        }
    }

    #[test]
    fn update_timeout_grows_from_the_uncompressed_images() {
        let small = compute_timeout(600, 3 * 1024 * 1024, 900, 4 * 1024 * 1024 * 1024);
        let large = compute_timeout(600, 3 * 1024 * 1024, 900, 8 * 1024 * 1024 * 1024);
        assert!(large > small);

        let dir = std::env::temp_dir().join(format!("flashwright-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("update.zip");
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("boot.img", options).unwrap();
            std::io::Write::write_all(&mut writer, &vec![0u8; 4096]).unwrap();
            writer.start_file("vbmeta.img", options).unwrap();
            std::io::Write::write_all(&mut writer, &vec![7u8; 2048]).unwrap();
            writer.start_file("android-info.txt", options).unwrap();
            std::io::Write::write_all(&mut writer, b"require board=komodo").unwrap();
            writer.finish().unwrap();
        }
        std::fs::write(&path, cursor.into_inner()).unwrap();
        let packed = std::fs::metadata(&path).unwrap().len();
        let images = uncompressed_image_bytes(&path).unwrap();
        assert_eq!(images, 4096 + 2048);
        assert!(images > packed);
        assert_eq!(update_size_bytes(&path, packed), images);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sideload_keeps_a_finalising_window() {
        let mib = 1024u64 * 1024;
        let cmd = WriteCmd::AdbHost(crate::cmd::AdbHostWrite::Sideload {
            serial: crate::cmd::DeviceSerial::try_from("pixel1").unwrap(),
            package: crate::cmd::ImageRef::new(0, "/var/flashwright/ota.zip", (5 * 1024 * mib) / 2),
        });
        let budget = write_budget(&cmd, (5 * 1024 * mib) / 2, 1.0).unwrap();
        assert_eq!(budget.timeout, Duration::from_secs(2180));
        assert_eq!(budget.watchdog, Some(Duration::from_secs(300)));
        assert_eq!(budget.finalising, Some(Duration::from_secs(900)));
    }

    #[test]
    fn multiplier_lengthens_only() {
        assert!(check_multiplier(0.5).is_err());
        assert!(check_multiplier(4.1).is_err());
        assert_eq!(scale_seconds(90, 2.0), 180);
        assert_eq!(scale_seconds(90, 1.0), 90);
    }
}
