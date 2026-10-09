// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use prost::Message;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::bootimg::synthetic_boot;
use crate::hashutil::{sha256_bytes, sha256_file};
use crate::open::{open_package, OpenRequest};
use crate::proto::chromeos_update_engine::{
    DeltaArchiveManifest, Extent, InstallOperation, PartitionInfo, PartitionUpdate,
};
use crate::ziputil::windows_flash_name;
use crate::{FirmwareError, PackageKind, StockPartition};

const PATCH: &str = "2026-10-01";
const FINGERPRINT: &str = "google/komodo/komodo:17/TEST/1:user/release-keys";
const TIMESTAMP: u64 = 1_750_000_000;

fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fw-m2-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn align_boot(image: Vec<u8>) -> Vec<u8> {
    let footer = image[image.len() - 64..].to_vec();
    let mut body = image[..image.len() - 64].to_vec();
    let total = image.len().div_ceil(4096) * 4096;
    body.resize(total - 64, 0);
    body.extend_from_slice(&footer);
    body
}

fn boot(partition: &str, patch: &str, fingerprint: &str) -> Vec<u8> {
    align_boot(synthetic_boot(partition, patch, fingerprint))
}

fn payload_bytes(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut blobs = Vec::new();
    let mut partitions = Vec::new();
    let mut cursor = 0u64;
    for (name, image) in parts {
        partitions.push(PartitionUpdate {
            partition_name: (*name).to_string(),
            new_partition_info: Some(PartitionInfo {
                size: Some(image.len() as u64),
                hash: Some(sha256_bytes(image).to_vec()),
            }),
            operations: vec![InstallOperation {
                r#type: 0,
                data_offset: Some(cursor),
                data_length: Some(image.len() as u64),
                dst_extents: vec![Extent {
                    start_block: Some(0),
                    num_blocks: Some((image.len() / 4096) as u64),
                }],
                ..Default::default()
            }],
            ..Default::default()
        });
        blobs.extend_from_slice(image);
        cursor += image.len() as u64;
    }
    let manifest = DeltaArchiveManifest {
        block_size: Some(4096),
        minor_version: Some(0),
        partitions,
        ..Default::default()
    };
    let encoded = manifest.encode_to_vec();
    let mut out = Vec::new();
    out.extend_from_slice(b"CrAU");
    out.extend_from_slice(&2u64.to_be_bytes());
    out.extend_from_slice(&(encoded.len() as u64).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&encoded);
    out.extend_from_slice(&blobs);
    out
}

fn stored() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn ota_zip(meta: &str, payload: &[u8], compress_payload: bool) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut cursor);
    writer
        .start_file("META-INF/com/android/metadata", stored())
        .unwrap();
    writer.write_all(meta.as_bytes()).unwrap();
    let method = if compress_payload {
        deflated()
    } else {
        stored()
    };
    writer.start_file("payload.bin", method).unwrap();
    writer.write_all(payload).unwrap();
    writer.finish().unwrap();
    cursor.into_inner()
}

fn factory_zip(info: &str, image_name: &str, image: &[u8], deflate_inner: bool) -> Vec<u8> {
    let mut inner_cursor = Cursor::new(Vec::new());
    let mut inner = ZipWriter::new(&mut inner_cursor);
    inner.start_file("android-info.txt", stored()).unwrap();
    inner.write_all(info.as_bytes()).unwrap();
    inner.start_file(image_name, stored()).unwrap();
    inner.write_all(image).unwrap();
    inner.finish().unwrap();
    let inner_bytes = inner_cursor.into_inner();

    let mut outer_cursor = Cursor::new(Vec::new());
    let mut outer = ZipWriter::new(&mut outer_cursor);
    outer.start_file("flash-all.sh", stored()).unwrap();
    outer.write_all(b"#!/bin/sh\n").unwrap();
    outer.start_file(windows_flash_name(), stored()).unwrap();
    outer.write_all(b"echo\n").unwrap();
    let method = if deflate_inner { deflated() } else { stored() };
    outer.start_file("image-device-test.zip", method).unwrap();
    outer.write_all(&inner_bytes).unwrap();
    outer.finish().unwrap();
    outer_cursor.into_inner()
}

fn meta(device: &str, patch: &str, fingerprint: &str, extra: &str) -> String {
    format!(
        "ota-type=AB\npre-device={device}\npost-security-patch-level={patch}\npost-build={fingerprint}\npost-timestamp={TIMESTAMP}\n{extra}"
    )
}

fn seal(dir: &Path, prefix: &str, bytes: &[u8]) -> (PathBuf, String) {
    let pending = dir.join("pending.zip");
    std::fs::write(&pending, bytes).unwrap();
    let hash = sha256_file(&pending, None).unwrap();
    let dest = dir.join(format!("{prefix}-{}.zip", &hash[..8]));
    std::fs::rename(&pending, &dest).unwrap();
    (dest, hash)
}

fn assert_no_images(dir: &Path) {
    if !dir.exists() {
        return;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        assert_ne!(path.extension().and_then(|ext| ext.to_str()), Some("img"));
    }
}

async fn run(
    path: &Path,
    out: &Path,
    published: Option<String>,
    codename: Option<&str>,
    timestamp: Option<u64>,
    patch: Option<&str>,
) -> Result<crate::OpenedPackage, FirmwareError> {
    open_package(OpenRequest {
        path: path.to_path_buf(),
        output_dir: out.to_path_buf(),
        published_sha256: published,
        expected_codename: codename.map(str::to_string),
        device_build_timestamp: timestamp,
        device_security_patch: patch.map(str::to_string),
        on_hash_progress: None,
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ota_extracts_init_boot_and_checks_the_payload_hash() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let bytes = ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, false);
    let root = scratch();
    let (path, hash) = seal(&root, "komodo-ota", &bytes);
    let out = root.join("out");
    let opened = run(&path, &out, Some(hash.clone()), Some("komodo"), None, None)
        .await
        .unwrap();
    assert_eq!(opened.kind, PackageKind::Ota);
    assert_eq!(opened.partition, StockPartition::InitBoot);
    assert_eq!(opened.package_sha256, hash);
    assert_eq!(
        opened.payload_partition_hash.as_deref(),
        Some(opened.image_sha256.as_str())
    );
    assert!(out.join("init_boot.img").is_file());
    assert!(!out.join("payload.bin").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oriole_extracts_boot() {
    let image = boot(
        "boot",
        PATCH,
        "google/oriole/oriole:14/TEST/1:user/release-keys",
    );
    let payload = payload_bytes(&[("boot", &image)]);
    let text = meta(
        "oriole",
        PATCH,
        "google/oriole/oriole:14/TEST/1:user/release-keys",
        "",
    );
    let root = scratch();
    let (path, _) = seal(&root, "oriole-ota", &ota_zip(&text, &payload, false));
    let opened = run(&path, &root.join("out"), None, Some("oriole"), None, None)
        .await
        .unwrap();
    assert_eq!(opened.partition, StockPartition::Boot);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn komodo_rejects_a_boot_only_package() {
    let image = boot("boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("boot", &image)]);
    let root = scratch();
    let (path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, false),
    );
    let err = run(&path, &root.join("out"), None, Some("komodo"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::PartitionMismatch));
    assert_no_images(&root.join("out"));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_codename_writes_no_image() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let root = scratch();
    let (path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(&meta("shiba", PATCH, FINGERPRINT, ""), &payload, false),
    );
    let err = run(&path, &root.join("out"), None, Some("komodo"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::CodenameMismatch));
    assert_no_images(&root.join("out"));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn alias_eos_matches_aurora() {
    let fingerprint = "google/aurora/aurora:17/TEST/1:user/release-keys";
    let image = boot("init_boot", PATCH, fingerprint);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let root = scratch();
    let (path, _) = seal(
        &root,
        "aurora-ota",
        &ota_zip(&meta("eos", PATCH, fingerprint, ""), &payload, false),
    );
    let opened = run(&path, &root.join("out"), None, Some("aurora"), None, None)
        .await
        .unwrap();
    assert_eq!(opened.codename, "aurora");
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn published_hash_and_filename_fragment() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let bytes = ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, false);
    let root = scratch();
    let (path, hash) = seal(&root, "komodo-ota", &bytes);
    let bad = "ab".repeat(32);
    let err = run(
        &path,
        &root.join("out"),
        Some(bad),
        Some("komodo"),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, FirmwareError::PublishedHash));
    let renamed = root.join("komodo-ota-00000000.zip");
    std::fs::rename(&path, &renamed).unwrap();
    let err = run(
        &renamed,
        &root.join("out2"),
        None,
        Some("komodo"),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, FirmwareError::FilenameFragment));
    let _ = hash;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incremental_and_non_ab_are_refused() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let root = scratch();
    let incremental = meta("komodo", PATCH, FINGERPRINT, "pre-build=older\n");
    let (path, _) = seal(&root, "komodo-ota", &ota_zip(&incremental, &payload, false));
    let err = run(&path, &root.join("out"), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::IncrementalOta));
    let block = "ota-type=BLOCK\npre-device=komodo\npost-timestamp=1\n";
    let (path, _) = seal(&root, "komodo-block", &ota_zip(block, &payload, false));
    let err = run(&path, &root.join("out2"), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::NotFullOta));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downgrade_is_refused() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let root = scratch();
    let (path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, false),
    );
    let err = run(
        &path,
        &root.join("out"),
        None,
        Some("komodo"),
        Some(TIMESTAMP + 1),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, FirmwareError::Downgrade));
    let err = run(
        &path,
        &root.join("out2"),
        None,
        Some("komodo"),
        None,
        Some("2026-11-01"),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, FirmwareError::Downgrade));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn avb_properties_must_match_the_package() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let root = scratch();
    let mismatched_patch = meta("komodo", "2026-12-01", FINGERPRINT, "");
    let (path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(&mismatched_patch, &payload, false),
    );
    let err = run(&path, &root.join("out"), None, Some("komodo"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::SecurityPatchMismatch));
    assert_no_images(&root.join("out"));
    let mismatched_print = meta(
        "komodo",
        PATCH,
        "google/komodo/komodo:17/OTHER/1:user/release-keys",
        "",
    );
    let (path, _) = seal(
        &root,
        "komodo-print",
        &ota_zip(&mismatched_print, &payload, false),
    );
    let err = run(&path, &root.join("out2"), None, Some("komodo"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::FingerprintMismatch));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tampered_payload_fails_the_metadata_hash() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let mut payload = payload_bytes(&[("init_boot", &image)]);
    let index = payload
        .windows(8)
        .position(|window| window == b"ANDROID!")
        .unwrap();
    payload[index + 20] ^= 0xff;
    let root = scratch();
    let (path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, false),
    );
    let err = run(&path, &root.join("out"), None, Some("komodo"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::PartitionHash));
    assert_no_images(&root.join("out"));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compressed_payload_is_refused() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let root = scratch();
    let (path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, true),
    );
    let err = run(&path, &root.join("out"), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::CompressedPayload));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn factory_stored_and_deflated_match_the_ota_image() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let info = format!("require board=komodo\nsecurity-patch-level={PATCH}\n");
    let root = scratch();
    let (stored_path, _) = seal(
        &root,
        "komodo-test-factory",
        &factory_zip(&info, "init_boot.img", &image, false),
    );
    let stored_out = root.join("stored");
    let stored_opened = run(&stored_path, &stored_out, None, Some("komodo"), None, None)
        .await
        .unwrap();
    assert_eq!(stored_opened.kind, PackageKind::Factory);
    assert_eq!(stored_opened.partition, StockPartition::InitBoot);
    assert!(!stored_out.join("image.zip").exists());

    let (deflated_path, _) = seal(
        &root,
        "komodo-deflated-factory",
        &factory_zip(&info, "init_boot.img", &image, true),
    );
    let deflated = run(
        &deflated_path,
        &root.join("deflated"),
        None,
        Some("komodo"),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(stored_opened.image_sha256, deflated.image_sha256);

    let payload = payload_bytes(&[("init_boot", &image)]);
    let (ota_path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, false),
    );
    let ota = run(
        &ota_path,
        &root.join("ota"),
        None,
        Some("komodo"),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(stored_opened.image_sha256, ota.image_sha256);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn factory_crc_failure_is_reported() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let info = format!("require board=komodo\nsecurity-patch-level={PATCH}\n");
    let mut bytes = factory_zip(&info, "init_boot.img", &image, false);
    let index = bytes
        .windows(8)
        .position(|window| window == b"ANDROID!")
        .unwrap();
    bytes[index + 20] ^= 0xff;
    let root = scratch();
    let (path, _) = seal(&root, "komodo-test-factory", &bytes);
    let err = run(&path, &root.join("out"), None, Some("komodo"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::Crc));
    assert_no_images(&root.join("out"));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn archive_kinds_are_rejected() {
    let root = scratch();
    let (path, _) = seal(&root, "komodo-ota", b"this is not a zip archive!!!!");
    let err = run(&path, &root.join("out"), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::Truncated));
    let tgz = root.join("komodo.tgz");
    std::fs::write(&tgz, b"nope").unwrap();
    let err = run(&tgz, &root.join("out"), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::UnsupportedArchive));
    let other = root.join("notes.txt");
    std::fs::write(&other, b"nope").unwrap();
    let err = run(&other, &root.join("out"), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::NotZip));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restricted_regions_are_refused_before_extract() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let root = scratch();
    let (path, _) = seal(
        &root,
        "komodo-lu0-ota",
        &ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, false),
    );
    let err = run(&path, &root.join("out"), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::RestrictedRegion));
    assert_no_images(&root.join("out"));
    let (path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(
            &meta("komodo", PATCH, FINGERPRINT, "channel=fips\n"),
            &payload,
            false,
        ),
    );
    let err = run(&path, &root.join("out2"), None, None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FirmwareError::RestrictedRegion));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hash_progress_is_reported() {
    let image = boot("init_boot", PATCH, FINGERPRINT);
    let payload = payload_bytes(&[("init_boot", &image)]);
    let root = scratch();
    let (path, _) = seal(
        &root,
        "komodo-ota",
        &ota_zip(&meta("komodo", PATCH, FINGERPRINT, ""), &payload, false),
    );
    let marks = Arc::new(AtomicU32::new(0));
    let marks_task = Arc::clone(&marks);
    open_package(OpenRequest {
        path,
        output_dir: root.join("out"),
        published_sha256: None,
        expected_codename: Some("komodo".to_string()),
        device_build_timestamp: None,
        device_security_patch: None,
        on_hash_progress: Some(Box::new(move |_| {
            marks_task.fetch_add(1, Ordering::Relaxed);
        })),
    })
    .await
    .unwrap();
    assert!(marks.load(Ordering::Relaxed) >= 1);
    assert!(crate::extraction_workers() <= 4);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn komodo_is_an_init_boot_device() {
    let devices = crate::catalog::DeviceTable::embedded().unwrap();
    assert_eq!(devices.has_init_boot("komodo"), Some(true));
    assert_eq!(devices.has_init_boot("oriole"), Some(false));
}
