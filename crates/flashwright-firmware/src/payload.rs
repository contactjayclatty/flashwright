// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Read a payload.bin manifest with the AOSP proto and extract one partition
//! with payload-dumper-rust. Extraction streams the stored zip entry.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use payload_dumper::payload::payload_dumper::{dump_partition, NoOpReporter};
use payload_dumper::payload::payload_parser::parse_local_zip_payload;
use payload_dumper::readers::local_zip_reader::LocalAsyncZipPayloadReader;
use prost::Message;

use crate::error::FirmwareError;
use crate::proto::chromeos_update_engine::{DeltaArchiveManifest, PartitionUpdate};
use crate::ziputil::{self, ListedEntry};

const MAX_MANIFEST: u64 = 16 * 1024 * 1024;

pub struct PayloadPartition {
    pub size: u64,
    pub hash: Vec<u8>,
}

pub fn payload_entry(entries: &[ListedEntry]) -> Option<&ListedEntry> {
    entries
        .iter()
        .find(|entry| !entry.is_dir && ziputil::base_name(&entry.name) == "payload.bin")
}

pub fn payload_offset(path: &Path, entry: &ListedEntry) -> Result<u64, FirmwareError> {
    if entry.compression != zip::CompressionMethod::Stored {
        return Err(FirmwareError::CompressedPayload);
    }
    if let Some(start) = entry.data_start {
        return Ok(start);
    }
    let mut file = File::open(path).map_err(FirmwareError::io)?;
    let (start, _) = ziputil::stored_data_range(&mut file, entry.header_start)?;
    Ok(start)
}

pub fn read_manifest(path: &Path, offset: u64) -> Result<DeltaArchiveManifest, FirmwareError> {
    let mut file = File::open(path).map_err(FirmwareError::io)?;
    file.seek(SeekFrom::Start(offset))
        .map_err(FirmwareError::io)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).map_err(FirmwareError::io)?;
    if &magic != b"CrAU" {
        return Err(FirmwareError::Truncated);
    }
    let version = read_u64(&mut file)?;
    if version != 2 {
        return Err(FirmwareError::Archive("unsupported payload version".into()));
    }
    let manifest_size = read_u64(&mut file)?;
    if manifest_size > MAX_MANIFEST {
        return Err(FirmwareError::Archive(
            "the payload manifest is too large".into(),
        ));
    }
    let _signature_size = read_u32(&mut file)?;
    let mut bytes = vec![0u8; manifest_size as usize];
    file.read_exact(&mut bytes).map_err(FirmwareError::io)?;
    DeltaArchiveManifest::decode(bytes.as_slice()).map_err(|_| FirmwareError::Truncated)
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

fn find_partition<'a>(
    manifest: &'a DeltaArchiveManifest,
    name: &str,
) -> Option<&'a PartitionUpdate> {
    manifest
        .partitions
        .iter()
        .find(|partition| partition.partition_name == name)
}

pub async fn extract_partition(
    zip_path: &Path,
    partition: &str,
    output: &Path,
) -> Result<(), FirmwareError> {
    let reader = LocalAsyncZipPayloadReader::new(zip_path.to_path_buf())
        .await
        .map_err(map_payload)?;
    let (manifest, data_offset, _zip_info) = parse_local_zip_payload(zip_path.to_path_buf())
        .await
        .map_err(map_payload)?;
    let block_size = u64::from(manifest.block_size.unwrap_or(4096));
    let found = manifest
        .partitions
        .iter()
        .find(|item| item.partition_name == partition)
        .ok_or(FirmwareError::NoBootImage)?;
    if let Err(err) = dump_partition(
        found,
        data_offset,
        block_size,
        output.to_path_buf(),
        &reader,
        &NoOpReporter,
        None,
    )
    .await
    {
        let _ = std::fs::remove_file(output);
        return Err(map_payload(err));
    }
    Ok(())
}

fn map_payload(err: impl std::fmt::Display) -> FirmwareError {
    let text = err.to_string().to_ascii_lowercase();
    if text.contains("compressed") {
        FirmwareError::CompressedPayload
    } else {
        FirmwareError::Archive("the payload could not be read".into())
    }
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
