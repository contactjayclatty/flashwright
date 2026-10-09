// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Open a factory zip or a full A/B OTA zip and extract init_boot or boot.
//! The package `File` is the only handle: hashing and extraction never open
//! the path again. This writes only into the caller's output directory.

use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::path::PathBuf;

use zip::ZipArchive;

use crate::catalog::{AliasTable, DeviceTable};
use crate::check::{self, GateAck, MAX_IMAGE, MAX_METADATA};
use crate::error::FirmwareError;
use crate::facts::DeviceFacts;
use crate::hashutil::{self, hex_encode};
use crate::metadata::{self, PackageMeta};
use crate::payload::{self, PayloadView};
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
    /// `YYYY-MM` from the boot header `os_patch_level`.
    pub image_security_patch: Option<String>,
    /// `ro.build.date.utc` from the image `build.prop`, when that file was present.
    pub image_build_date_utc: Option<u64>,
    /// Gates whose image value could not be read. An acknowledgement is not a pass.
    pub acks: Vec<GateAck>,
}

pub struct OpenRequest {
    /// Already open. The path is not opened again.
    pub package: File,
    /// File name only, used for the codename and the checksum fragment.
    pub file_name: String,
    pub output_dir: PathBuf,
    /// Google's published package SHA-256. Empty is refused.
    pub published_sha256: String,
    /// Codename, build date, and security patch taken from the phone.
    pub device: DeviceFacts,
    pub on_hash_progress: Option<Box<dyn FnMut(u64) + Send>>,
}

/// One partition is extracted. The worker ceiling is min(4, the host parallelism).
pub fn extraction_workers() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(4)
}

pub async fn open_package(request: OpenRequest) -> Result<OpenedPackage, FirmwareError> {
    tokio::task::spawn_blocking(move || open_package_sync(request))
        .await
        .map_err(|_| FirmwareError::Archive("a firmware task stopped".into()))?
}

fn open_package_sync(mut request: OpenRequest) -> Result<OpenedPackage, FirmwareError> {
    check::extension_ok(&request.file_name)?;
    let mut progress = request.on_hash_progress.take();
    let mut package = request.package;
    let package_sha256 = match progress.as_mut() {
        Some(callback) => hashutil::sha256_reader(&mut package, Some(callback.as_mut()))?,
        None => hashutil::sha256_reader(&mut package, None)?,
    };
    package
        .seek(SeekFrom::Start(0))
        .map_err(FirmwareError::io)?;
    check::filename_fragment_ok(&request.file_name, &package_sha256)?;
    check::published_ok(Some(request.published_sha256.as_str()), &package_sha256)?;
    check::reject_region(&request.file_name)?;
    let filename_codename = check::codename_from_filename(&request.file_name)?;
    check::reject_region(&filename_codename)?;

    let mut archive = ZipArchive::new(package).map_err(ziputil::map_zip)?;
    let entries = ziputil::list_archive(&mut archive)?;
    let kind = classify(&entries)?;
    let opened = match kind {
        PackageKind::Ota => extract_ota(
            archive,
            &entries,
            &request.output_dir,
            &filename_codename,
            &request.device,
        )?,
        PackageKind::Factory => {
            let image = crate::factory::extract_factory(
                archive,
                &entries,
                &request.output_dir,
                &filename_codename,
                &request.device,
            )?;
            Extracted {
                codename: image.codename,
                partition: image.partition,
                image_path: image.image_path,
                checked: image.checked,
                payload_hash: None,
                post_timestamp: image.post_timestamp,
                post_build: image.post_build,
            }
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
        image_security_patch: opened.checked.image_security_patch,
        image_build_date_utc: opened.checked.image_build_date_utc,
        acks: opened.checked.acks,
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

fn extract_ota(
    mut archive: ZipArchive<File>,
    entries: &[ListedEntry],
    output_dir: &std::path::Path,
    filename_codename: &str,
    device: &DeviceFacts,
) -> Result<Extracted, FirmwareError> {
    let (aliases, devices) = tables()?;
    let meta_entry = entries
        .iter()
        .find(|entry| is_metadata(&entry.name))
        .ok_or(FirmwareError::NotFullOta)?;
    let meta_text = read_entry_text(&mut archive, meta_entry)?;
    check::reject_region(&meta_text)?;
    let meta = metadata::parse_metadata(&meta_text);
    check::full_ab_ota(&meta)?;
    let stated = meta
        .pre_device
        .as_deref()
        .map(metadata::board_names)
        .unwrap_or_default();
    let codename = check::agree_codename(&aliases, filename_codename, &stated, &device.codename)?;
    check::reject_region(&codename)?;

    let payload_entry = payload::payload_entry(entries).ok_or(FirmwareError::NotFullOta)?;
    let header_start = payload_entry.header_start;
    let data_start = payload_entry.data_start;
    let mut package = archive.into_inner();
    let offset = match data_start {
        Some(start) => start,
        None => ziputil::stored_data_range(&mut package, header_start)?.0,
    };
    let view = payload::read_manifest(&mut package, offset)?;
    let names = payload::partition_names(&view.manifest);
    let partition = select::select_partition(&names, devices.has_init_boot(&codename))?;
    let record = payload::partition_record(&view.manifest, partition.as_str())?;
    if record.size > MAX_IMAGE {
        return Err(FirmwareError::Archive(
            "the image is larger than Flashwright will extract".into(),
        ));
    }
    std::fs::create_dir_all(output_dir).map_err(FirmwareError::io)?;
    space::ensure_free_space(output_dir, working_need(0, record.size))?;
    let image_path = output_dir.join(partition.file_name());
    let checked = write_and_check(
        &mut package,
        &view,
        &ImageWrite {
            partition,
            image_path: &image_path,
            meta: &meta,
            expected_hash: &record.hash,
            expected_size: record.size,
            device,
        },
    )?;
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

struct ImageWrite<'a> {
    partition: StockPartition,
    image_path: &'a std::path::Path,
    meta: &'a PackageMeta,
    expected_hash: &'a [u8],
    expected_size: u64,
    device: &'a DeviceFacts,
}

fn write_and_check(
    package: &mut File,
    view: &PayloadView,
    job: &ImageWrite<'_>,
) -> Result<crate::check::ImageCheck, FirmwareError> {
    let mut output = ziputil::create_output(job.image_path)?;
    let result = (|| {
        payload::extract_replace(package, view, job.partition.as_str(), &mut output)?;
        output.seek(SeekFrom::Start(0)).map_err(FirmwareError::io)?;
        check::check_image_file(
            &mut output,
            job.partition,
            job.meta,
            Some(job.expected_hash),
            Some(job.expected_size),
            job.device,
        )
    })();
    match result {
        Ok(checked) => Ok(checked),
        Err(err) => {
            drop(output);
            let _ = std::fs::remove_file(job.image_path);
            Err(err)
        }
    }
}

fn is_metadata(name: &str) -> bool {
    name == "META-INF/com/android/metadata" || name.ends_with("/META-INF/com/android/metadata")
}

fn read_entry_text(
    archive: &mut ZipArchive<File>,
    entry: &ListedEntry,
) -> Result<String, FirmwareError> {
    if entry.size > MAX_METADATA {
        return Err(FirmwareError::Archive(
            "a metadata entry is too large".into(),
        ));
    }
    let mut file = archive.by_index(entry.index).map_err(ziputil::map_zip)?;
    let bytes = ziputil::read_limited(
        &mut file,
        entry.size,
        MAX_METADATA,
        "a metadata entry is too large",
    )?;
    String::from_utf8(bytes)
        .map_err(|_| FirmwareError::Archive("a metadata entry could not be read".into()))
}

fn tables() -> Result<(AliasTable, DeviceTable), FirmwareError> {
    let aliases = AliasTable::embedded()
        .map_err(|_| FirmwareError::Archive("the device table could not be read".into()))?;
    let devices = DeviceTable::embedded()
        .map_err(|_| FirmwareError::Archive("the device table could not be read".into()))?;
    Ok((aliases, devices))
}
