// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Open a Pixel factory zip. A stored inner image zip is read as a sub-range
//! of the package file already open. A deflated inner image zip is streamed
//! to the work directory and removed afterwards. The codename check runs
//! before that copy.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use zip::CompressionMethod;
use zip::ZipArchive;

use crate::catalog::{AliasTable, DeviceTable};
use crate::check::{self, ImageCheck, MAX_IMAGE, MAX_INNER_ZIP, MAX_METADATA};
use crate::error::FirmwareError;
use crate::facts::DeviceFacts;
use crate::metadata::{self, PackageMeta};
use crate::select::{self, StockPartition};
use crate::space::{self, working_need};
use crate::ziputil::{self, FileWindow, ListedEntry};

const META_TOO_BIG: &str = "a metadata entry is too large";
const IMAGE_TOO_BIG: &str = "the image is larger than Flashwright will extract";
const INNER_TOO_BIG: &str = "the image zip is larger than Flashwright will unpack";

pub struct FactoryImage {
    pub codename: String,
    pub partition: StockPartition,
    pub image_path: PathBuf,
    pub checked: ImageCheck,
    pub post_timestamp: Option<u64>,
    pub post_build: Option<String>,
}

pub fn extract_factory(
    mut archive: ZipArchive<File>,
    entries: &[ListedEntry],
    output_dir: &Path,
    filename_codename: &str,
    device: &DeviceFacts,
) -> Result<FactoryImage, FirmwareError> {
    let (aliases, devices) = tables()?;
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
        return Err(FirmwareError::Archive(INNER_TOO_BIG.into()));
    }
    let outer_meta = text_named(&mut archive, entries, |name| {
        ziputil::base_name(name) == "android-info.txt"
    })?;
    check::filename_matches_device(&aliases, filename_codename, &device.codename)?;
    if let Some(text) = outer_meta.as_deref() {
        let meta = metadata::parse_metadata(text);
        if let Some(board) = meta.board.as_deref() {
            let stated = metadata::board_names(board);
            check::agree_codename(&aliases, filename_codename, &stated, &device.codename)?;
        }
    }

    if inner.compression == CompressionMethod::Stored {
        let header_start = inner.header_start;
        let data_start = inner.data_start;
        let compressed = inner.compressed_size;
        let mut file = archive.into_inner();
        let start = match data_start {
            Some(start) => start,
            None => ziputil::stored_data_range(&mut file, header_start)?.0,
        };
        let window = FileWindow::new(file, start, compressed);
        let mut inner_archive = ZipArchive::new(window).map_err(ziputil::map_zip)?;
        finish_inner(
            &mut inner_archive,
            &InnerJob {
                output_dir,
                aliases: &aliases,
                devices: &devices,
                filename_codename,
                device,
                outer_meta: outer_meta.as_deref(),
                inner_zip_bytes: 0,
            },
        )
    } else {
        std::fs::create_dir_all(output_dir).map_err(FirmwareError::io)?;
        space::ensure_free_space(output_dir, working_need(inner.size, MAX_IMAGE))?;
        let temp = ziputil::temp_inner(output_dir);
        let mut output = ziputil::create_output(&temp)?;
        let _remove = RemoveFile(temp);
        let index = inner.index;
        let declared = inner.size;
        {
            let mut entry = archive.by_index(index).map_err(ziputil::map_zip)?;
            ziputil::copy_capped(&mut entry, &mut output, MAX_INNER_ZIP, INNER_TOO_BIG)?;
            if declared > 0 && output.metadata().map_err(FirmwareError::io)?.len() != declared {
                return Err(FirmwareError::Truncated);
            }
        }
        output.seek(SeekFrom::Start(0)).map_err(FirmwareError::io)?;
        let mut inner_archive = ZipArchive::new(output).map_err(ziputil::map_zip)?;
        finish_inner(
            &mut inner_archive,
            &InnerJob {
                output_dir,
                aliases: &aliases,
                devices: &devices,
                filename_codename,
                device,
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
    device: &'a DeviceFacts,
    outer_meta: Option<&'a str>,
    inner_zip_bytes: u64,
}

fn finish_inner<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    job: &InnerJob<'_>,
) -> Result<FactoryImage, FirmwareError> {
    let inner_entries = ziputil::list_archive(archive)?;
    let inner_text = text_named(archive, &inner_entries, |name| {
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
        &job.device.codename,
    )?;
    check::reject_region(&codename)?;
    let names = image_names(&inner_entries);
    let partition = select::select_partition(&names, job.devices.has_init_boot(&codename))?;
    let image_entry = inner_entries
        .iter()
        .find(|entry| ziputil::base_name(&entry.name) == partition.file_name())
        .ok_or(FirmwareError::NoBootImage)?;
    if image_entry.size > MAX_IMAGE {
        return Err(FirmwareError::Archive(IMAGE_TOO_BIG.into()));
    }
    std::fs::create_dir_all(job.output_dir).map_err(FirmwareError::io)?;
    space::ensure_free_space(
        job.output_dir,
        working_need(job.inner_zip_bytes, image_entry.size),
    )?;
    let image_path = job.output_dir.join(partition.file_name());
    let mut output = ziputil::create_output(&image_path)?;
    let copy_result = (|| {
        let mut entry = archive
            .by_index(image_entry.index)
            .map_err(ziputil::map_zip)?;
        ziputil::copy_capped(&mut entry, &mut output, MAX_IMAGE, IMAGE_TOO_BIG)?;
        output.seek(SeekFrom::Start(0)).map_err(FirmwareError::io)?;
        check::check_image_file(
            &mut output,
            partition,
            &meta,
            None,
            Some(image_entry.size),
            job.device,
        )
    })();
    let checked = match copy_result {
        Ok(checked) => checked,
        Err(err) => {
            drop(output);
            let _ = std::fs::remove_file(&image_path);
            return Err(err);
        }
    };
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

fn text_named<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    entries: &[ListedEntry],
    pred: impl Fn(&str) -> bool,
) -> Result<Option<String>, FirmwareError> {
    let Some(entry) = entries.iter().find(|entry| pred(&entry.name)) else {
        return Ok(None);
    };
    if entry.size > MAX_METADATA {
        return Err(FirmwareError::Archive(META_TOO_BIG.into()));
    }
    let mut file = archive.by_index(entry.index).map_err(ziputil::map_zip)?;
    let bytes = ziputil::read_limited(&mut file, entry.size, MAX_METADATA, META_TOO_BIG)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| FirmwareError::Archive("a metadata entry could not be read".into()))?;
    Ok(Some(text))
}

struct RemoveFile(PathBuf);

impl Drop for RemoveFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
