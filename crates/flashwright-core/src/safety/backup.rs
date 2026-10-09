// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Stock init_boot backup.
//!
//! The bytes come from the typed read catalogue (`exec-out` of `cat` on the
//! block device). They are stored with a SHA-256, compared to the factory
//! image, and compared again to a second read.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cmd::{ByNameRoot, DeviceSerial, ExecOutSuRead, ReadCmd};
use crate::device::{Partition, PlatformToolsTransport, Slot};
use crate::proc::CommandRunner;

use super::evaluate::GateBlock;

/// Factory `init_boot` bytes. The image extractor implements this.
pub trait FactoryInitBoot {
    fn codename(&self) -> &str;
    fn init_boot(&self) -> &[u8];
}

/// In-memory factory image for tests and for a caller that already holds the bytes.
#[derive(Clone, Debug)]
pub struct BytesInitBoot {
    pub codename: String,
    pub bytes: Vec<u8>,
}

impl FactoryInitBoot for BytesInitBoot {
    fn codename(&self) -> &str {
        &self.codename
    }

    fn init_boot(&self) -> &[u8] {
        &self.bytes
    }
}

/// A stored stock image. The bytes stay private so a later check uses [`Self::matches`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitBootRecord {
    pub slot: Slot,
    pub sha256: String,
    pub len: usize,
    bytes: Vec<u8>,
}

impl InitBootRecord {
    pub fn from_bytes(slot: Slot, bytes: Vec<u8>) -> Self {
        let sha256 = sha256_hex(&bytes);
        let len = bytes.len();
        Self {
            slot,
            sha256,
            len,
            bytes,
        }
    }

    pub fn matches(&self, other: &[u8]) -> bool {
        self.bytes == other && sha256_hex(&self.bytes) == self.sha256
    }

    /// Write the image and a `sha256sum` line next to it.
    pub fn write_to(&self, dir: &Path) -> Result<PathBuf, String> {
        fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        let name = format!("init_boot_{}.img", self.slot.as_str());
        let path = dir.join(&name);
        fs::write(&path, &self.bytes).map_err(|err| err.to_string())?;
        let sum = dir.join(format!("{name}.sha256"));
        fs::write(&sum, format!("{}  {name}\n", self.sha256)).map_err(|err| err.to_string())?;
        let read_back = fs::read(&path).map_err(|err| err.to_string())?;
        if read_back != self.bytes {
            return Err("stored init_boot does not match the bytes just written".into());
        }
        Ok(path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupState {
    Verified(InitBootRecord),
    Blocked(GateBlock),
}

pub async fn pull_stock_init_boot<R: CommandRunner>(
    transport: &PlatformToolsTransport<R>,
    serial: &DeviceSerial,
    slot: Slot,
    expected_codename: &str,
    factory: &dyn FactoryInitBoot,
) -> Result<InitBootRecord, GateBlock> {
    if !same_device(expected_codename, factory.codename()) {
        return Err(block(
            "G04",
            format!(
                "The factory image is for {}, and this phone is {expected_codename}.",
                factory.codename()
            ),
        ));
    }
    let first = pull(transport, serial, slot).await?;
    let factory_bytes = factory.init_boot();
    if first != factory_bytes {
        return Err(block(
            "G14",
            "The init_boot read from the phone does not match the factory image.",
        ));
    }
    let record = InitBootRecord {
        slot,
        sha256: sha256_hex(&first),
        len: first.len(),
        bytes: first,
    };
    if !record.matches(factory_bytes) {
        return Err(block(
            "G14",
            "The stored init_boot hash does not match the factory image.",
        ));
    }
    let second = pull(transport, serial, slot).await?;
    if !record.matches(&second) {
        return Err(block(
            "G14",
            "Reading init_boot again did not match the stored backup.",
        ));
    }
    Ok(record)
}

fn same_device(expected: &str, factory: &str) -> bool {
    let tables = super::tables::tables();
    tables.aliases.canonical(expected) == tables.aliases.canonical(factory)
}

async fn pull<R: CommandRunner>(
    transport: &PlatformToolsTransport<R>,
    serial: &DeviceSerial,
    slot: Slot,
) -> Result<Vec<u8>, GateBlock> {
    let cmd = ReadCmd::ExecOutSu(ExecOutSuRead::CatBlock {
        serial: serial.clone(),
        root: ByNameRoot::ByName,
        partition: Partition::InitBoot,
        slot,
    });
    let result = transport
        .run_read(cmd)
        .await
        .map_err(|err| block("G14", format!("The init_boot read failed: {err}")))?;
    if !result.success_exit() {
        return Err(block(
            "G14",
            "The init_boot read did not finish successfully.",
        ));
    }
    if result.stdout.is_empty() {
        return Err(block("G14", "The init_boot read was empty."));
    }
    Ok(result.stdout)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
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

    fn factory(bytes: &[u8]) -> BytesInitBoot {
        BytesInitBoot {
            codename: "komodo".into(),
            bytes: bytes.to_vec(),
        }
    }

    #[tokio::test]
    async fn stock_init_boot_matches_the_factory_image_and_a_second_read() {
        let stock = b"synthetic-komodo-init-boot".to_vec();
        let runner = Arc::new(ScriptedRunner::new());
        let seen = stock.clone();
        runner.on_fn(
            adb_name(),
            &["-s", "synth-komodo-1", "exec-out"],
            move |_call, _hit| ScriptedResponse::ok(seen.clone()),
        );
        let transport = scripted(Arc::clone(&runner));
        let record =
            pull_stock_init_boot(&transport, &serial(), Slot::A, "komodo", &factory(&stock))
                .await
                .unwrap();
        assert_eq!(record.len, stock.len());
        assert_eq!(record.sha256, sha256_hex(&stock));
        assert_eq!(record.sha256.len(), 64);
        assert!(record.matches(&stock));
        assert_eq!(runner.calls().len(), 2);
        assert!(runner.calls().iter().all(|call| {
            call.args.first().is_some_and(|arg| arg == "-s")
                && call.args.get(2).is_some_and(|arg| arg == "exec-out")
                && !call.args.iter().any(|arg| arg == "flash")
        }));

        let dir = std::env::temp_dir().join(format!("flashwright-init-boot-{}", record.sha256));
        let path = record.write_to(&dir).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), stock);
        let sum = std::fs::read_to_string(dir.join("init_boot_a.img.sha256")).unwrap();
        assert_eq!(sum, format!("{}  init_boot_a.img\n", record.sha256));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_factory_or_reread_mismatch_blocks_before_anything_is_stored() {
        let stock = b"synthetic-komodo-init-boot".to_vec();
        let runner = Arc::new(ScriptedRunner::new());
        runner.on(
            adb_name(),
            &["-s", "synth-komodo-1", "exec-out"],
            ScriptedResponse::ok(stock.clone()),
        );
        let transport = scripted(Arc::clone(&runner));
        let mut other = stock.clone();
        other[0] = other[0].wrapping_add(1);
        let err = pull_stock_init_boot(&transport, &serial(), Slot::A, "komodo", &factory(&other))
            .await
            .unwrap_err();
        assert_eq!(err.id, "G14");
        assert!(err.reason.contains("factory"));
        assert_eq!(runner.calls().len(), 1);

        let wrong = BytesInitBoot {
            codename: "shiba".into(),
            bytes: stock.clone(),
        };
        let before = runner.calls().len();
        let err = pull_stock_init_boot(&transport, &serial(), Slot::A, "komodo", &wrong)
            .await
            .unwrap_err();
        assert_eq!(err.id, "G04");
        assert_eq!(runner.calls().len(), before);

        let first = stock.clone();
        let second = other.clone();
        let runner = Arc::new(ScriptedRunner::new());
        runner.on_fn(
            adb_name(),
            &["-s", "synth-komodo-1", "exec-out"],
            move |_call, hit| {
                let bytes = if hit == 0 {
                    first.clone()
                } else {
                    second.clone()
                };
                ScriptedResponse::ok(bytes)
            },
        );
        let transport = scripted(runner);
        let err = pull_stock_init_boot(&transport, &serial(), Slot::A, "komodo", &factory(&stock))
            .await
            .unwrap_err();
        assert_eq!(err.id, "G14");
        assert!(err.reason.contains("again"));
    }
}
