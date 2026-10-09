// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Opt-in check against a factory image the caller already has.
//! Set FLASHWRIGHT_KOMODO_IMAGE to the zip path. CI leaves it unset.
//! Optional FLASHWRIGHT_KOMODO_SHA256 is the published package checksum.
//! Failure text stays generic.

use std::io::Read;
use std::path::PathBuf;

use flashwright_firmware::{open_package, OpenRequest, StockPartition};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn komodo_factory_when_configured() {
    let Ok(path) = std::env::var("FLASHWRIGHT_KOMODO_IMAGE") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let published = std::env::var("FLASHWRIGHT_KOMODO_SHA256")
        .ok()
        .filter(|value| !value.is_empty());
    let out = std::env::temp_dir().join(format!("fw-komodo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).expect("work directory");
    let opened = match open_package(OpenRequest {
        path: PathBuf::from(path),
        output_dir: out.clone(),
        published_sha256: published.clone(),
        expected_codename: Some("komodo".to_string()),
        device_build_timestamp: None,
        device_security_patch: None,
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
    if let Some(expected) = published {
        if !expected.eq_ignore_ascii_case(&opened.package_sha256) {
            panic!("published hash check failed");
        }
    }
    println!("komodo factory image: init_boot extracted and checks passed");
    let _ = std::fs::remove_dir_all(&out);
}
