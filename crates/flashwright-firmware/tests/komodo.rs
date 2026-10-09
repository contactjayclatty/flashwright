// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Opt-in check against a factory image the caller already has.
//! Set FW_PRIVATE_FIXTURES and FLASHWRIGHT_KOMODO_IMAGE. CI leaves both unset.
//! Optional FLASHWRIGHT_KOMODO_SHA256 is the published package checksum.
//! Failure text stays generic. Image values are not printed.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use flashwright_firmware::{open_package, DeviceFacts, OpenRequest, StockPartition};
use sha2::{Digest, Sha256};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn komodo_factory_when_configured() {
    let fixtures = std::env::var("FW_PRIVATE_FIXTURES")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let image = std::env::var("FLASHWRIGHT_KOMODO_IMAGE")
        .ok()
        .filter(|value| !value.trim().is_empty());
    if fixtures.is_none() || image.is_none() {
        println!("BLOCKED");
        return;
    }
    let path = image.unwrap();
    let mut package = File::open(&path).expect("factory image");
    let published = match std::env::var("FLASHWRIGHT_KOMODO_SHA256") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; 1024 * 1024];
            loop {
                let read = package.read(&mut buf).expect("factory image");
                if read == 0 {
                    break;
                }
                hasher.update(&buf[..read]);
            }
            package.seek(SeekFrom::Start(0)).expect("factory image");
            format!("{:x}", hasher.finalize())
        }
    };
    let file_name = std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("komodo.zip")
        .to_string();
    let out = std::env::temp_dir().join(format!("fw-komodo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).expect("work directory");
    let opened = match open_package(OpenRequest {
        package,
        file_name,
        output_dir: out.clone(),
        published_sha256: published,
        device: DeviceFacts {
            codename: "komodo".to_string(),
            build_date_utc: None,
            security_patch: None,
        },
        on_hash_progress: None,
    })
    .await
    {
        Ok(opened) => opened,
        Err(err) => panic!("komodo factory image did not open: {err}"),
    };
    if opened.partition != StockPartition::InitBoot {
        panic!("komodo factory image did not yield init_boot");
    }
    let mut magic = [0u8; 8];
    let mut file = std::fs::File::open(&opened.image_path).expect("extracted image");
    file.read_exact(&mut magic).expect("extracted image header");
    if &magic != b"ANDROID!" {
        panic!("extracted image is not a boot image");
    }
    println!("komodo factory image: init_boot extracted and checks passed");
    let _ = std::fs::remove_dir_all(&out);
}
