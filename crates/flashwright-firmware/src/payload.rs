// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Read a payload.bin manifest with the AOSP proto and copy REPLACE bytes
//! from the package file that is already open.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

use prost::Message;

use crate::check::MAX_IMAGE;
use crate::error::FirmwareError;
use crate::proto::chromeos_update_engine::{DeltaArchiveManifest, PartitionUpdate};
use crate::ziputil::{self, ListedEntry};

const MAX_MANIFEST: u64 = 16 * 1024 * 1024;
const IMAGE_TOO_BIG: &str = "the image is larger than Flashwright will extract";
const MANIFEST_TOO_BIG: &str = "the payload manifest is too large";

pub struct PayloadPartition {
    pub size: u64,
    pub hash: Vec<u8>,
}

pub struct PayloadView {
    pub manifest: DeltaArchiveManifest,
    pub data_offset: u64,
}

pub fn payload_entry(entries: &[ListedEntry]) -> Option<&ListedEntry> {
    entries
        .iter()
        .find(|entry| !entry.is_dir && ziputil::base_name(&entry.name) == "payload.bin")
}

/// `payload_dumper` 0.8.4 stays a dependency of this crate. Extraction uses the
/// open package handle, so the crate's path-based reader is not called.
#[allow(dead_code)]
fn payload_dumper_crate_is_linked() -> usize {
    std::mem::size_of::<payload_dumper::payload::payload_dumper::NoOpReporter>()
}

pub fn read_manifest(file: &mut File, offset: u64) -> Result<PayloadView, FirmwareError> {
    file.seek(SeekFrom::Start(offset))
        .map_err(FirmwareError::io)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).map_err(FirmwareError::io)?;
    if &magic != b"CrAU" {
        return Err(FirmwareError::Truncated);
    }
    let version = read_u64(file)?;
    if version != 2 {
        return Err(FirmwareError::Archive("unsupported payload version".into()));
    }
    let manifest_size = read_u64(file)?;
    if manifest_size > MAX_MANIFEST {
        return Err(FirmwareError::Archive(MANIFEST_TOO_BIG.into()));
    }
    let signature_size = u64::from(read_u32(file)?);
    let mut bytes = vec![0u8; manifest_size as usize];
    file.read_exact(&mut bytes).map_err(FirmwareError::io)?;
    let manifest =
        DeltaArchiveManifest::decode(bytes.as_slice()).map_err(|_| FirmwareError::Truncated)?;
    let data_offset = offset
        .saturating_add(24)
        .saturating_add(manifest_size)
        .saturating_add(signature_size);
    Ok(PayloadView {
        manifest,
        data_offset,
    })
}

pub fn partition_names(manifest: &DeltaArchiveManifest) -> Vec<String> {
    manifest
        .partitions
        .iter()
        .map(|partition| partition.partition_name.clone())
        .collect()
}

pub fn partition_record(
    manifest: &DeltaArchiveManifest,
    name: &str,
) -> Result<PayloadPartition, FirmwareError> {
    let partition = find_partition(manifest, name).ok_or(FirmwareError::NoBootImage)?;
    let info = partition
        .new_partition_info
        .as_ref()
        .ok_or(FirmwareError::PartitionHash)?;
    let size = info.size.ok_or(FirmwareError::PartitionHash)?;
    let hash = info.hash.clone().ok_or(FirmwareError::PartitionHash)?;
    if hash.len() != 32 {
        return Err(FirmwareError::PartitionHash);
    }
    Ok(PayloadPartition { size, hash })
}

pub fn extract_replace(
    package: &mut File,
    view: &PayloadView,
    name: &str,
    output: &mut File,
) -> Result<(), FirmwareError> {
    let partition = find_partition(&view.manifest, name).ok_or(FirmwareError::NoBootImage)?;
    let block_size = u64::from(view.manifest.block_size.unwrap_or(4096));
    if block_size == 0 || block_size > 1024 * 1024 {
        return Err(FirmwareError::Archive(
            "the payload could not be read".into(),
        ));
    }
    let mut written = 0u64;
    for operation in &partition.operations {
        if operation.r#type != 0 {
            return Err(FirmwareError::Archive(
                "the payload could not be read".into(),
            ));
        }
        let data_length = operation.data_length.unwrap_or(0);
        if data_length > MAX_IMAGE || written.saturating_add(data_length) > MAX_IMAGE {
            return Err(FirmwareError::Archive(IMAGE_TOO_BIG.into()));
        }
        let data_offset = operation.data_offset.unwrap_or(0);
        package
            .seek(SeekFrom::Start(
                view.data_offset.saturating_add(data_offset),
            ))
            .map_err(FirmwareError::io)?;
        let mut blob = vec![0u8; data_length as usize];
        package.read_exact(&mut blob).map_err(FirmwareError::io)?;
        let mut consumed = 0usize;
        if operation.dst_extents.is_empty() {
            return Err(FirmwareError::Archive(
                "the payload could not be read".into(),
            ));
        }
        for extent in &operation.dst_extents {
            let start = extent
                .start_block
                .ok_or_else(|| FirmwareError::Archive("the payload could not be read".into()))?;
            if start == u64::MAX {
                return Err(FirmwareError::Archive(
                    "the payload could not be read".into(),
                ));
            }
            let blocks = extent
                .num_blocks
                .ok_or_else(|| FirmwareError::Archive("the payload could not be read".into()))?;
            let nbytes = blocks.saturating_mul(block_size);
            if nbytes > MAX_IMAGE || consumed as u64 + nbytes > data_length {
                return Err(FirmwareError::Archive(
                    "the payload could not be read".into(),
                ));
            }
            let end = consumed + nbytes as usize;
            if written.saturating_add(nbytes) > MAX_IMAGE {
                return Err(FirmwareError::Archive(IMAGE_TOO_BIG.into()));
            }
            output
                .seek(SeekFrom::Start(start.saturating_mul(block_size)))
                .map_err(FirmwareError::io)?;
            output
                .write_all(&blob[consumed..end])
                .map_err(FirmwareError::io)?;
            written = written.saturating_add(nbytes);
            consumed = end;
        }
    }
    output.flush().map_err(FirmwareError::io)?;
    Ok(())
}

fn find_partition<'a>(
    manifest: &'a DeltaArchiveManifest,
    name: &str,
) -> Option<&'a PartitionUpdate> {
    manifest
        .partitions
        .iter()
        .find(|partition| partition.partition_name == name)
}

fn read_u64(file: &mut File) -> Result<u64, FirmwareError> {
    let mut bytes = [0u8; 8];
    file.read_exact(&mut bytes).map_err(FirmwareError::io)?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_u32(file: &mut File) -> Result<u32, FirmwareError> {
    let mut bytes = [0u8; 4];
    file.read_exact(&mut bytes).map_err(FirmwareError::io)?;
    Ok(u32::from_be_bytes(bytes))
}
