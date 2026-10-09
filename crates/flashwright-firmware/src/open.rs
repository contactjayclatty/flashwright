// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Open a factory zip or a full A/B OTA zip and extract init_boot or boot.
//! This writes only into the caller's output directory. It does not talk to a phone.

use std::path::{Path, PathBuf};

use crate::catalog::{AliasTable, DeviceTable};
use crate::check::{self, MAX_IMAGE};
use crate::error::FirmwareError;
use crate::factory;
use crate::hashutil::hex_encode;
use crate::metadata;
use crate::payload;
use crate::select::{self, StockPartition};
use crate::space::{self, working_need};
use crate::ziputil::{self, ListedEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageKind {
    Factory,
    Ota,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedPackage {
    pub kind: PackageKind,
    pub codename: String,
    pub package_sha256: String,
    pub partition: StockPartition,
    pub image_path: PathBuf,
    pub image_sha256: String,
    pub image_sha1: String,
    pub payload_partition_hash: Option<String>,
    pub security_patch: Option<String>,
    pub fingerprint: Option<String>,
    pub post_timestamp: Option<u64>,
    pub post_build: Option<String>,
}

pub struct OpenRequest {
    pub path: PathBuf,
    pub output_dir: PathBuf,
    pub published_sha256: Option<String>,
    pub expected_codename: Option<String>,
    pub device_build_timestamp: Option<u64>,
    pub device_security_patch: Option<String>,
    pub on_hash_progress: Option<Box<dyn FnMut(u64) + Send>>,
}

/// One partition is extracted. The worker ceiling is min(4, the host parallelism).
pub fn extraction_workers() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(4)
}

pub async fn open_package(mut request: OpenRequest) -> Result<OpenedPackage, FirmwareError> {
    check::extension_ok(&request.path)?;
    let path_for_hash = request.path.clone();
    let mut progress = request.on_hash_progress.take();
    let package_sha256 =
        tokio::task::spawn_blocking(move || check::hash_with(&path_for_hash, &mut progress))
            .await
            .map_err(|_| FirmwareError::Archive("a firmware task stopped".into()))??;
    check::filename_fragment_ok(&request.path, &package_sha256)?;
    check::published_ok(request.published_sha256.as_deref(), &package_sha256)?;
    check::reject_region(&check::file_label(&request.path))?;
    let filename_codename = check::codename_from_filename(&request.path)?;
    check::reject_region(&filename_codename)?;

    let entries = ziputil::list_entries(&request.path)?;
    let kind = classify(&entries)?;
    let opened = match kind {
        PackageKind::Ota => extract_ota(&request, &filename_codename).await?,
        PackageKind::Factory => {
            let path = request.path.clone();
            let output = request.output_dir.clone();
            let expected = request.expected_codename.clone();
            let timestamp = request.device_build_timestamp;
            let patch = request.device_security_patch.clone();
            let filename_codename = filename_codename.clone();
            tokio::task::spawn_blocking(move || {
                let image = factory::extract_factory(
                    &path,
                    &output,
                    &filename_codename,
                    expected.as_deref(),
                    timestamp,
                    patch.as_deref(),
                )?;
                Ok(Extracted {
                    codename: image.codename,
                    partition: image.partition,
                    image_path: image.image_path,
                    checked: image.checked,
                    payload_hash: None,
                    post_timestamp: image.post_timestamp,
                    post_build: image.post_build,
                })
            })
            .await
            .map_err(|_| FirmwareError::Archive("a firmware task stopped".into()))??
        }
    };
    Ok(OpenedPackage {
        kind,
        package_sha256,
        codename: opened.codename,
        partition: opened.partition,
        image_path: opened.image_path,
        image_sha256: opened.checked.image_sha256,
        image_sha1: opened.checked.image_sha1,
        payload_partition_hash: opened.payload_hash,
        security_patch: opened.checked.security_patch,
        fingerprint: opened.checked.fingerprint,
        post_timestamp: opened.post_timestamp,
        post_build: opened.post_build,
    })
}

struct Extracted {
    codename: String,
    partition: StockPartition,
    image_path: PathBuf,
    checked: crate::check::ImageCheck,
    payload_hash: Option<String>,
    post_timestamp: Option<u64>,
    post_build: Option<String>,
}

fn classify(entries: &[ListedEntry]) -> Result<PackageKind, FirmwareError> {
    if let Some(payload) = payload::payload_entry(entries) {
        if payload.compression != zip::CompressionMethod::Stored {
            return Err(FirmwareError::CompressedPayload);
        }
        return Ok(PackageKind::Ota);
    }
    let images = entries
        .iter()
        .filter(|entry| ziputil::is_image_zip(&entry.name))
        .count();
    let scripts = entries.iter().any(|entry| {
        let base = ziputil::base_name(&entry.name);
        base == "flash-all.sh" || base == ziputil::windows_flash_name()
    });
    if scripts && images == 1 {
        Ok(PackageKind::Factory)
    } else {
        Err(FirmwareError::FactoryLayout)
    }
}

async fn extract_ota(
    request: &OpenRequest,
    filename_codename: &str,
) -> Result<Extracted, FirmwareError> {
    let (aliases, devices) = tables()?;
    let entries = ziputil::list_entries(&request.path)?;
    let meta_entry = entries
        .iter()
        .find(|entry| is_metadata(&entry.name))
        .ok_or(FirmwareError::NotFullOta)?;
    let meta_text = read_entry_text(&request.path, meta_entry)?;
    check::reject_region(&meta_text)?;
    let meta = metadata::parse_metadata(&meta_text);
    check::full_ab_ota(&meta)?;
    let stated = meta
        .pre_device
        .as_deref()
        .map(metadata::board_names)
        .unwrap_or_default();
    let codename = check::agree_codename(
        &aliases,
        filename_codename,
        &stated,
        request.expected_codename.as_deref(),
    )?;
    check::reject_region(&codename)?;
    check::downgrade_ok(
        &meta,
        request.device_build_timestamp,
        request.device_security_patch.as_deref(),
    )?;

    let payload_entry = payload::payload_entry(&entries).ok_or(FirmwareError::NotFullOta)?;
    let offset = payload::payload_offset(&request.path, payload_entry)?;
    let manifest = payload::read_manifest(&request.path, offset)?;
    let names = payload::partition_names(&manifest);
    let partition = select::select_partition(&names, devices.has_init_boot(&codename))?;
    let record = payload::partition_record(&manifest, partition.as_str())?;
    if record.size > MAX_IMAGE {
        return Err(FirmwareError::Archive(
            "the image is larger than Flashwright will extract".into(),
        ));
    }
    std::fs::create_dir_all(&request.output_dir).map_err(FirmwareError::io)?;
    space::ensure_free_space(&request.output_dir, working_need(0, record.size))?;
    let image_path = request.output_dir.join(partition.file_name());
    payload::extract_partition(&request.path, partition.as_str(), &image_path).await?;
    let checked = match check::check_extracted_image(
        &image_path,
        partition,
        &meta,
        Some(&record.hash),
        Some(record.size),
    ) {
        Ok(checked) => checked,
        Err(err) => {
            let _ = std::fs::remove_file(&image_path);
            return Err(err);
        }
    };
    Ok(Extracted {
        codename,
        partition,
        image_path,
        checked,
        payload_hash: Some(hex_encode(&record.hash)),
        post_timestamp: meta.post_timestamp,
        post_build: meta.post_build,
    })
}

fn is_metadata(name: &str) -> bool {
    name == "META-INF/com/android/metadata" || name.ends_with("/META-INF/com/android/metadata")
}

fn read_entry_text(path: &Path, entry: &ListedEntry) -> Result<String, FirmwareError> {
    const CAP: u64 = 1024 * 1024;
    if entry.size > CAP {
        return Err(FirmwareError::Archive(
            "a metadata entry is too large".into(),
        ));
    }
    let mut archive = ziputil::open_zip(path)?;
    let mut file = archive.by_index(entry.index).map_err(ziputil::map_zip)?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut file, &mut text).map_err(ziputil::map_read)?;
    Ok(text)
}

fn tables() -> Result<(AliasTable, DeviceTable), FirmwareError> {
    let aliases = AliasTable::embedded()
        .map_err(|_| FirmwareError::Archive("the device table could not be read".into()))?;
    let devices = DeviceTable::embedded()
        .map_err(|_| FirmwareError::Archive("the device table could not be read".into()))?;
    Ok((aliases, devices))
}
