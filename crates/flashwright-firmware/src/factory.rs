// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Open a Pixel factory zip. A stored inner image zip is read as a sub-range.
//! A deflated inner image zip is streamed to the work directory in 64 KiB
//! buffers and removed afterwards.

use std::fs::File;
use std::path::{Path, PathBuf};

use zip::CompressionMethod;

use crate::catalog::{AliasTable, DeviceTable};
use crate::check::{self, ImageCheck, MAX_IMAGE, MAX_INNER_ZIP};
use crate::error::FirmwareError;
use crate::metadata::{self, PackageMeta};
use crate::select::{self, StockPartition};
use crate::space::{self, working_need};
use crate::ziputil::{self, FileWindow, ListedEntry};

pub struct FactoryImage {
    pub codename: String,
    pub partition: StockPartition,
    pub image_path: PathBuf,
    pub checked: ImageCheck,
    pub post_timestamp: Option<u64>,
    pub post_build: Option<String>,
}

pub fn extract_factory(
    path: &Path,
    output_dir: &Path,
    filename_codename: &str,
    expected_codename: Option<&str>,
    device_timestamp: Option<u64>,
    device_security_patch: Option<&str>,
) -> Result<FactoryImage, FirmwareError> {
    let (aliases, devices) = tables()?;
    let entries = ziputil::list_entries(path)?;
    let images: Vec<&ListedEntry> = entries
        .iter()
        .filter(|entry| ziputil::is_image_zip(&entry.name))
        .collect();
    let scripts = entries.iter().any(|entry| is_flash_script(&entry.name));
    if !scripts || images.len() != 1 {
        return Err(FirmwareError::FactoryLayout);
    }
    let inner = images[0];
    if inner.size > MAX_INNER_ZIP {
        return Err(FirmwareError::Archive(
            "the image zip is larger than Flashwright will unpack".into(),
        ));
    }
    let outer_meta = read_named_text(path, &entries, |name| {
        ziputil::base_name(name) == "android-info.txt"
    })?;

    if inner.compression == CompressionMethod::Stored {
        let start = match inner.data_start {
            Some(start) => start,
            None => {
                let mut file = File::open(path).map_err(FirmwareError::io)?;
                ziputil::stored_data_range(&mut file, inner.header_start)?.0
            }
        };
        let file = File::open(path).map_err(FirmwareError::io)?;
        let window = FileWindow::new(file, start, inner.compressed_size);
        let mut archive = zip::ZipArchive::new(window).map_err(ziputil::map_zip)?;
        finish_inner(
            &mut archive,
            &InnerJob {
                output_dir,
                aliases: &aliases,
                devices: &devices,
                filename_codename,
                expected_codename,
                device_timestamp,
                device_security_patch,
                outer_meta: outer_meta.as_deref(),
                inner_zip_bytes: 0,
            },
        )
    } else {
        std::fs::create_dir_all(output_dir).map_err(FirmwareError::io)?;
        space::ensure_free_space(output_dir, working_need(inner.size, MAX_IMAGE))?;
        let temp = ziputil::temp_inner(output_dir);
        stream_index(path, inner.index, &temp)?;
        let _remove = RemoveFile(temp.clone());
        let mut archive = ziputil::open_zip(&temp)?;
        finish_inner(
            &mut archive,
            &InnerJob {
                output_dir,
                aliases: &aliases,
                devices: &devices,
                filename_codename,
                expected_codename,
                device_timestamp,
                device_security_patch,
                outer_meta: outer_meta.as_deref(),
                inner_zip_bytes: inner.size,
            },
        )
    }
}

struct InnerJob<'a> {
    output_dir: &'a Path,
    aliases: &'a AliasTable,
    devices: &'a DeviceTable,
    filename_codename: &'a str,
    expected_codename: Option<&'a str>,
    device_timestamp: Option<u64>,
    device_security_patch: Option<&'a str>,
    outer_meta: Option<&'a str>,
    inner_zip_bytes: u64,
}

fn finish_inner<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    job: &InnerJob<'_>,
) -> Result<FactoryImage, FirmwareError> {
    let inner_entries = list_archive(archive)?;
    let inner_text = text_from_archive(archive, &inner_entries, |name| {
        ziputil::base_name(name) == "android-info.txt"
    })?;
    let meta = choose_meta(job.outer_meta, inner_text.as_deref());
    check::reject_region(&meta.raw)?;
    let stated = meta
        .board
        .as_deref()
        .map(metadata::board_names)
        .unwrap_or_default();
    let codename = check::agree_codename(
        job.aliases,
        job.filename_codename,
        &stated,
        job.expected_codename,
    )?;
    check::reject_region(&codename)?;
    check::downgrade_ok(&meta, job.device_timestamp, job.device_security_patch)?;
    let names = image_names(&inner_entries);
    let partition = select::select_partition(&names, job.devices.has_init_boot(&codename))?;
    let image_entry = inner_entries
        .iter()
        .find(|entry| ziputil::base_name(&entry.name) == partition.file_name())
        .ok_or(FirmwareError::NoBootImage)?;
    if image_entry.size > MAX_IMAGE {
        return Err(FirmwareError::Archive(
            "the image is larger than Flashwright will extract".into(),
        ));
    }
    std::fs::create_dir_all(job.output_dir).map_err(FirmwareError::io)?;
    space::ensure_free_space(
        job.output_dir,
        working_need(job.inner_zip_bytes, image_entry.size),
    )?;
    let image_path = job.output_dir.join(partition.file_name());
    let mut entry = archive
        .by_index(image_entry.index)
        .map_err(ziputil::map_zip)?;
    if let Err(err) = ziputil::stream_entry(&mut entry, &image_path) {
        let _ = std::fs::remove_file(&image_path);
        return Err(err);
    }
    drop(entry);
    let checked =
        check::check_extracted_image(&image_path, partition, &meta, None, Some(image_entry.size))?;
    Ok(FactoryImage {
        codename,
        partition,
        image_path,
        checked,
        post_timestamp: meta.post_timestamp,
        post_build: meta.post_build,
    })
}

fn choose_meta(outer: Option<&str>, inner: Option<&str>) -> PackageMeta {
    let inner_meta = inner.map(metadata::parse_metadata);
    let outer_meta = outer.map(metadata::parse_metadata);
    match (inner_meta, outer_meta) {
        (Some(inner), _) if inner.board.is_some() => inner,
        (_, Some(outer)) if outer.board.is_some() => outer,
        (Some(inner), _) => inner,
        (_, Some(outer)) => outer,
        _ => PackageMeta::default(),
    }
}

fn is_flash_script(name: &str) -> bool {
    let base = ziputil::base_name(name);
    base == "flash-all.sh" || base == ziputil::windows_flash_name()
}

fn image_names(entries: &[ListedEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| {
            let base = ziputil::base_name(&entry.name);
            let stem = base.strip_suffix(".img")?;
            if stem == "init_boot" || stem == "boot" {
                Some(stem.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn tables() -> Result<(AliasTable, DeviceTable), FirmwareError> {
    let aliases = AliasTable::embedded()
        .map_err(|_| FirmwareError::Archive("the device table could not be read".into()))?;
    let devices = DeviceTable::embedded()
        .map_err(|_| FirmwareError::Archive("the device table could not be read".into()))?;
    Ok((aliases, devices))
}

fn read_named_text(
    path: &Path,
    entries: &[ListedEntry],
    pred: impl Fn(&str) -> bool,
) -> Result<Option<String>, FirmwareError> {
    let Some(entry) = entries.iter().find(|entry| pred(&entry.name)) else {
        return Ok(None);
    };
    let mut archive = ziputil::open_zip(path)?;
    let mut file = archive.by_index(entry.index).map_err(ziputil::map_zip)?;
    read_capped(&mut file, entry.size)
}

fn text_from_archive<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    entries: &[ListedEntry],
    pred: impl Fn(&str) -> bool,
) -> Result<Option<String>, FirmwareError> {
    let Some(entry) = entries.iter().find(|entry| pred(&entry.name)) else {
        return Ok(None);
    };
    let mut file = archive.by_index(entry.index).map_err(ziputil::map_zip)?;
    read_capped(&mut file, entry.size)
}

fn read_capped<R: std::io::Read>(
    entry: &mut zip::read::ZipFile<'_, R>,
    size: u64,
) -> Result<Option<String>, FirmwareError> {
    const CAP: u64 = 1024 * 1024;
    if size > CAP {
        return Err(FirmwareError::Archive(
            "a metadata entry is too large".into(),
        ));
    }
    let mut text = String::new();
    std::io::Read::read_to_string(entry, &mut text).map_err(ziputil::map_read)?;
    Ok(Some(text))
}

fn list_archive<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<Vec<ListedEntry>, FirmwareError> {
    let mut out = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(ziputil::map_zip)?;
        out.push(ListedEntry {
            index,
            name: ziputil::entry_name(&entry)?,
            size: entry.size(),
            compressed_size: entry.compressed_size(),
            compression: entry.compression(),
            data_start: entry.data_start(),
            header_start: entry.header_start(),
            is_dir: entry.is_dir(),
        });
    }
    Ok(out)
}

fn stream_index(path: &Path, index: usize, dest: &Path) -> Result<(), FirmwareError> {
    let mut archive = ziputil::open_zip(path)?;
    let mut entry = archive.by_index(index).map_err(ziputil::map_zip)?;
    ziputil::stream_entry(&mut entry, dest)
}

struct RemoveFile(PathBuf);

impl Drop for RemoveFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
