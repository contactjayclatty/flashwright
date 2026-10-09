// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Stock backup of both slots of the boot image and of vbmeta.
//!
//! Each partition is hashed on the phone, then read. The host hash must equal
//! the phone hash. A truncated read is not a complete backup. Bytes are written
//! under a partial directory and renamed into place with a manifest.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha1::{Digest as Sha1Digest, Sha1};
use sha2::Sha256;

use crate::cmd::{ByNameRoot, DeviceSerial, ExecOutSuRead, ReadCmd, SuRead, MAX_BLOCK_LEN};
use crate::device::{Partition, PlatformToolsTransport, Slot};
use crate::proc::CommandRunner;
use crate::timeouts;

use super::evaluate::GateBlock;

const SCHEMA: &str = "flashwright.backup/1";

/// A verified backup set bound to one phone, slot, and partition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupSet {
    pub set_id: String,
    pub manifest_sha256: String,
    pub serial_sha256: String,
    pub slot: Slot,
    pub partition: Partition,
    pub dir: PathBuf,
}

impl BackupSet {
    #[cfg(test)]
    pub(crate) fn bound(
        set_id: impl Into<String>,
        manifest_sha256: impl Into<String>,
        serial: &str,
        slot: Slot,
        partition: Partition,
    ) -> Self {
        Self {
            set_id: set_id.into(),
            manifest_sha256: manifest_sha256.into(),
            serial_sha256: sha256_hex(serial.as_bytes()),
            slot,
            partition,
            dir: PathBuf::new(),
        }
    }

    pub fn matches_plan(&self, serial: &str, slot: Slot, partition: Partition) -> bool {
        !self.set_id.is_empty()
            && !self.manifest_sha256.is_empty()
            && self.serial_sha256 == sha256_hex(serial.as_bytes())
            && self.slot == slot
            && self.partition == partition
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupState {
    Verified(BackupSet),
    Blocked(GateBlock),
}

#[derive(serde::Serialize)]
struct ManifestItem {
    partition: String,
    slot: String,
    file: String,
    size: u64,
    sha256: String,
    sha1: String,
    device_sha256: String,
    source: &'static str,
}

#[derive(serde::Serialize)]
struct ManifestFile {
    schema: &'static str,
    set_id: String,
    serial_sha256: String,
    items: Vec<ManifestItem>,
}

struct Captured {
    partition: Partition,
    slot: Slot,
    file: String,
    size: u64,
    sha256: String,
    sha1: String,
    device_sha256: String,
}

/// Read both slots of `partition` and of vbmeta.
///
/// Each image is streamed to disk, read a second time, and checked against
/// `sha256sum` on the phone. The three digests must agree. A factory image is
/// not the reference, so a rooted phone passes.
pub async fn capture_stock<R: CommandRunner>(
    transport: &PlatformToolsTransport<R>,
    serial: &DeviceSerial,
    slot: Slot,
    partition: Partition,
    dest: &Path,
) -> Result<BackupSet, GateBlock> {
    if partition == Partition::Vbmeta {
        return Err(block(
            "G14",
            "vbmeta is read with the boot image, not alone.",
        ));
    }
    let serial_sha256 = sha256_hex(serial.as_str().as_bytes());
    let set_id = sha256_hex(
        format!(
            "{serial_sha256}:{}:{}:4",
            slot.as_str(),
            partition.fastboot_name()
        )
        .as_bytes(),
    );
    let partial = dest.join(format!("{set_id}.partial"));
    if partial.exists() {
        fs::remove_dir_all(&partial).map_err(|err| block("G14", err.to_string()))?;
    }
    fs::create_dir_all(&partial).map_err(|err| block("G14", err.to_string()))?;
    let mut captured = Vec::new();
    for part in [partition, Partition::Vbmeta] {
        for part_slot in [Slot::A, Slot::B] {
            match stream_one(transport, serial, part, part_slot, &partial).await {
                Ok(item) => captured.push(item),
                Err(err) => {
                    let _ = fs::remove_dir_all(&partial);
                    return Err(err);
                }
            }
        }
    }
    finish_set(
        serial_sha256,
        set_id,
        slot,
        partition,
        dest,
        partial,
        captured,
    )
}

async fn stream_one<R: CommandRunner>(
    transport: &PlatformToolsTransport<R>,
    serial: &DeviceSerial,
    partition: Partition,
    slot: Slot,
    dir: &Path,
) -> Result<Captured, GateBlock> {
    let device_sha = device_sha256(transport, serial, partition, slot).await?;
    let file = format!("{}_{}.img", partition.fastboot_name(), slot.as_str());
    let path = dir.join(&file);
    let first = pull_block(transport, serial, partition, slot).await?;
    let (sha256, sha1) = write_chunks(&path, &first)?;
    let second = pull_block(transport, serial, partition, slot).await?;
    let second_sha = hash_chunks(&second).0;
    if sha256 != second_sha || sha256 != device_sha {
        return Err(block(
            "G14",
            format!("The {partition} reads do not match the phone's SHA-256."),
        ));
    }
    Ok(Captured {
        partition,
        slot,
        file,
        size: first.len() as u64,
        sha256,
        sha1,
        device_sha256: device_sha,
    })
}

async fn pull_block<R: CommandRunner>(
    transport: &PlatformToolsTransport<R>,
    serial: &DeviceSerial,
    partition: Partition,
    slot: Slot,
) -> Result<Vec<u8>, GateBlock> {
    let cmd = ReadCmd::ExecOutSu(ExecOutSuRead::CatBlock {
        serial: serial.clone(),
        root: ByNameRoot::ByName,
        partition,
        slot,
    });
    let budget = timeouts::read_budget(&cmd);
    if budget.timeout.as_secs() < 60 || budget.watchdog.is_none() {
        return Err(block(
            "G14",
            "The block read timeout is shorter than the catalogue.",
        ));
    }
    let result = transport
        .run_read(cmd)
        .await
        .map_err(|err| block("G14", format!("The {partition} read failed: {err}")))?;
    if result.stdout_truncated {
        return Err(block("G14", format!("The {partition} read was truncated.")));
    }
    if !result.success_exit() {
        return Err(block(
            "G14",
            format!("The {partition} read did not finish successfully."),
        ));
    }
    if result.stdout.is_empty() {
        return Err(block("G14", format!("The {partition} read was empty.")));
    }
    if over_catalogue_cap(result.stdout.len() as u64) {
        return Err(block(
            "G14",
            format!("The {partition} read is larger than the catalogue allows."),
        ));
    }
    Ok(result.stdout)
}

async fn device_sha256<R: CommandRunner>(
    transport: &PlatformToolsTransport<R>,
    serial: &DeviceSerial,
    partition: Partition,
    slot: Slot,
) -> Result<String, GateBlock> {
    let cmd = ReadCmd::Su(SuRead::Sha256Block {
        serial: serial.clone(),
        root: ByNameRoot::ByName,
        partition,
        slot,
    });
    let result = transport
        .run_read(cmd)
        .await
        .map_err(|err| block("G14", format!("The phone SHA-256 read failed: {err}")))?;
    if result.stdout_truncated || !result.success_exit() {
        return Err(block(
            "G14",
            "The phone SHA-256 read was truncated or failed.",
        ));
    }
    parse_sha256(&result.stdout_text()).ok_or_else(|| {
        block(
            "G14",
            "The phone did not return a SHA-256 for the partition.",
        )
    })
}

fn finish_set(
    serial_sha256: String,
    set_id: String,
    slot: Slot,
    partition: Partition,
    dest: &Path,
    partial: PathBuf,
    captured: Vec<Captured>,
) -> Result<BackupSet, GateBlock> {
    let mut items = Vec::new();
    let mut sums = String::new();
    for item in &captured {
        sums.push_str(&format!("{}  {}\n", item.sha256, item.file));
        items.push(ManifestItem {
            partition: item.partition.fastboot_name().to_string(),
            slot: item.slot.as_str().to_string(),
            file: item.file.clone(),
            size: item.size,
            sha256: item.sha256.clone(),
            sha1: item.sha1.clone(),
            device_sha256: item.device_sha256.clone(),
            source: "device-root-read",
        });
    }
    fs::write(partial.join("SHA256SUMS"), &sums).map_err(|err| block("G14", err.to_string()))?;
    let manifest = ManifestFile {
        schema: SCHEMA,
        set_id: set_id.clone(),
        serial_sha256: serial_sha256.clone(),
        items,
    };
    let body = serde_json::to_vec_pretty(&manifest).map_err(|err| block("G14", err.to_string()))?;
    fs::write(partial.join("manifest.json"), &body).map_err(|err| block("G14", err.to_string()))?;
    let final_dir = dest.join(&set_id);
    if final_dir.exists() {
        fs::remove_dir_all(&final_dir).map_err(|err| block("G14", err.to_string()))?;
    }
    fs::rename(&partial, &final_dir).map_err(|err| block("G14", err.to_string()))?;
    Ok(BackupSet {
        set_id,
        manifest_sha256: sha256_hex(&body),
        serial_sha256,
        slot,
        partition,
        dir: final_dir,
    })
}

fn write_chunks(path: &Path, bytes: &[u8]) -> Result<(String, String), GateBlock> {
    let mut file = File::create(path).map_err(|err| block("G14", err.to_string()))?;
    let mut sha256 = Sha256::new();
    let mut sha1 = Sha1::new();
    for chunk in bytes.chunks(64 * 1024) {
        file.write_all(chunk)
            .map_err(|err| block("G14", err.to_string()))?;
        sha256.update(chunk);
        sha1.update(chunk);
    }
    file.sync_all()
        .map_err(|err| block("G14", err.to_string()))?;
    Ok((hex(sha256.finalize()), hex(sha1.finalize())))
}

fn over_catalogue_cap(len: u64) -> bool {
    len > MAX_BLOCK_LEN
}

fn hash_chunks(bytes: &[u8]) -> (String, String) {
    let mut sha256 = Sha256::new();
    let mut sha1 = Sha1::new();
    for chunk in bytes.chunks(64 * 1024) {
        sha256.update(chunk);
        sha1.update(chunk);
    }
    (hex(sha256.finalize()), hex(sha1.finalize()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes))
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn parse_sha256(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?.trim();
    if token.len() == 64 && token.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(token.to_ascii_lowercase())
    } else {
        None
    }
}

fn block(id: &'static str, reason: impl Into<String>) -> GateBlock {
    GateBlock {
        id,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::cmd::DeviceSerial;
    use crate::device::{PlatformToolsTransport, Slot, TransportConfig};
    use crate::proc::{ScriptedResponse, ScriptedRunner};

    fn serial() -> DeviceSerial {
        DeviceSerial::try_from("synth-komodo-1").unwrap()
    }

    fn adb_name() -> &'static str {
        if cfg!(windows) {
            "adb.exe"
        } else {
            "adb"
        }
    }

    fn tool(name: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!(r"C:\flashwright-test\{name}"))
        } else {
            PathBuf::from(format!("/opt/flashwright-test/{name}"))
        }
    }

    fn scripted(runner: Arc<ScriptedRunner>) -> PlatformToolsTransport<ScriptedRunner> {
        PlatformToolsTransport::new(
            runner,
            tool(adb_name()),
            tool(if cfg!(windows) {
                "fastboot.exe"
            } else {
                "fastboot"
            }),
            TransportConfig::for_tests(),
        )
    }

    fn rooted_bytes(partition: Partition, slot: Slot) -> Vec<u8> {
        format!("rooted-{}-{}", partition.fastboot_name(), slot.as_str()).into_bytes()
    }

    fn answer(call: &crate::proc::RecordedCall) -> ScriptedResponse {
        let joined = call.args.join(" ");
        let partition = if joined.contains("vbmeta") {
            Partition::Vbmeta
        } else {
            Partition::InitBoot
        };
        let slot = if joined.contains("_b") {
            Slot::B
        } else {
            Slot::A
        };
        let bytes = rooted_bytes(partition, slot);
        if joined.contains("sha256sum") {
            ScriptedResponse::ok(format!("{}  block\n", sha256_hex(&bytes)))
        } else {
            ScriptedResponse::ok(bytes)
        }
    }

    #[tokio::test]
    async fn rooted_phone_matches_its_own_sha_and_writes_both_slots() {
        let runner = Arc::new(ScriptedRunner::new());
        runner.on_fn(adb_name(), &["-s", "synth-komodo-1"], |call, _hit| {
            answer(call)
        });
        let transport = scripted(Arc::clone(&runner));
        let dest = std::env::temp_dir().join(format!("flashwright-backup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dest);
        let set = capture_stock(&transport, &serial(), Slot::B, Partition::InitBoot, &dest)
            .await
            .unwrap();
        assert!(set.matches_plan("synth-komodo-1", Slot::B, Partition::InitBoot));
        assert!(!set.matches_plan("other-phone", Slot::B, Partition::InitBoot));
        assert!(!set.matches_plan("synth-komodo-1", Slot::A, Partition::InitBoot));
        assert!(!set.matches_plan("synth-komodo-1", Slot::B, Partition::Boot));
        let manifest = fs::read_to_string(set.dir.join("manifest.json")).unwrap();
        assert!(manifest.contains("flashwright.backup/1"));
        assert!(manifest.contains(&set.serial_sha256));
        assert!(!manifest.contains("synth-komodo-1"));
        for name in [
            "init_boot_a.img",
            "init_boot_b.img",
            "vbmeta_a.img",
            "vbmeta_b.img",
        ] {
            assert!(set.dir.join(name).is_file(), "{name}");
        }
        assert!(set.dir.join("SHA256SUMS").is_file());
        assert!(!dest.join(format!("{}.partial", set.set_id)).exists());
        let cat = runner
            .calls()
            .into_iter()
            .find(|call| call.args.iter().any(|arg| arg.contains("cat")))
            .unwrap();
        assert!(cat.timeout.as_secs() >= 60);
        assert!(cat.watchdog.is_some_and(|wait| wait.as_secs() >= 60));
        let budget = crate::timeouts::read_budget(&crate::cmd::ReadCmd::ExecOutSu(
            crate::cmd::ExecOutSuRead::CatBlock {
                serial: serial(),
                root: crate::cmd::ByNameRoot::ByName,
                partition: Partition::InitBoot,
                slot: Slot::A,
            },
        ));
        assert_eq!(cat.timeout, budget.timeout);
        assert_eq!(cat.watchdog, budget.watchdog);
        let cats = runner
            .calls()
            .into_iter()
            .filter(|call| call.args.iter().any(|arg| arg.contains("cat")))
            .count();
        assert_eq!(cats, 8);
        let _ = fs::remove_dir_all(&dest);
    }

    #[tokio::test]
    async fn a_truncated_or_mismatched_read_blocks() {
        let runner = Arc::new(ScriptedRunner::new());
        runner.on_fn(adb_name(), &["-s", "synth-komodo-1"], |call, _hit| {
            let joined = call.args.join(" ");
            if joined.contains("sha256sum") {
                ScriptedResponse::ok(format!("{}  block\n", "ab".repeat(32)))
            } else {
                ScriptedResponse::ok(b"short".to_vec()).truncated()
            }
        });
        let transport = scripted(runner);
        let dest =
            std::env::temp_dir().join(format!("flashwright-backup-trunc-{}", std::process::id()));
        let err = capture_stock(&transport, &serial(), Slot::A, Partition::InitBoot, &dest)
            .await
            .unwrap_err();
        assert_eq!(err.id, "G14");
        assert!(err.reason.contains("truncated"));
        let _ = fs::remove_dir_all(&dest);
    }

    #[tokio::test]
    async fn a_second_read_that_differs_from_the_phone_blocks() {
        let runner = Arc::new(ScriptedRunner::new());
        runner.on_fn(adb_name(), &["-s", "synth-komodo-1"], |call, hit| {
            let joined = call.args.join(" ");
            if joined.contains("exec-out") && hit == 2 {
                return ScriptedResponse::ok(b"not-the-same-bytes".to_vec());
            }
            answer(call)
        });
        let transport = scripted(runner);
        let dest = std::env::temp_dir().join(format!(
            "flashwright-backup-mismatch-{}",
            std::process::id()
        ));
        let err = capture_stock(&transport, &serial(), Slot::A, Partition::InitBoot, &dest)
            .await
            .unwrap_err();
        assert_eq!(err.id, "G14");
        assert!(err.reason.contains("SHA-256"));
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn catalogue_cap_rejects_an_oversized_length() {
        assert!(!over_catalogue_cap(MAX_BLOCK_LEN));
        assert!(over_catalogue_cap(MAX_BLOCK_LEN + 1));
    }
}
