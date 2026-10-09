// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Read-only check of a patched init_boot image.
//!
//! This crate parses bytes. It does not spawn a process, and it does not
//! invoke magiskboot. Every buffer is capped.

mod cpio;
mod error;
mod header;
mod ramdisk;

pub use error::BootError;
pub use ramdisk::RamdiskFormat;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::cpio::{config_sha1, walk};
use crate::header::layout;
use crate::ramdisk::decompress;

const BINDING_SCHEMA: &str = "flashwright.bootimg.v1";

/// What the parser found, bound to one plan hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundInspection {
    pub header_version: u32,
    pub ramdisk_format: RamdiskFormat,
    pub config_sha1: String,
    pub patched_sha256: String,
    pub plan_binding: String,
}

/// Parse `patched`, require Magisk's `init` and `.backup/.magisk`, and
/// compare that stock SHA-1 with the stock image hash sealed into the plan.
pub fn inspect_patched_init_boot(
    patched: &[u8],
    stock_sha1: &str,
    stock_sha256: &str,
    plan_hash: &str,
) -> Result<BoundInspection, BootError> {
    if !hex_len(stock_sha1, 40) || !hex_len(stock_sha256, 64) || !plan_hash_ok(plan_hash) {
        return Err(BootError::Binding);
    }
    let parsed = layout(patched)?;
    let ramdisk = patched
        .get(parsed.ramdisk..parsed.ramdisk + parsed.ramdisk_len)
        .ok_or(BootError::Header)?;
    let (format, archive) = decompress(ramdisk)?;
    let found = walk(&archive)?;
    let sha1 = config_sha1(&found.config)?;
    if !sha1.eq_ignore_ascii_case(stock_sha1) {
        return Err(BootError::StockMismatch);
    }
    let patched_sha256 = hex(Sha256::digest(patched));
    if patched_sha256.eq_ignore_ascii_case(stock_sha256) {
        return Err(BootError::StockMismatch);
    }
    let plan_binding = bind(
        plan_hash,
        parsed.header_version,
        format.as_str(),
        &sha1,
        stock_sha1,
        stock_sha256,
        &patched_sha256,
    )?;
    Ok(BoundInspection {
        header_version: parsed.header_version,
        ramdisk_format: format,
        config_sha1: sha1,
        patched_sha256,
        plan_binding,
    })
}

fn bind(
    plan_hash: &str,
    header_version: u32,
    ramdisk_format: &str,
    config_sha1: &str,
    stock_sha1: &str,
    stock_sha256: &str,
    patched_sha256: &str,
) -> Result<String, BootError> {
    let body = BindingBody {
        schema: BINDING_SCHEMA,
        plan_hash,
        header_version,
        ramdisk_format,
        config_sha1,
        stock_sha1,
        stock_sha256,
        patched_sha256,
    };
    let bytes = serde_jcs::to_vec(&body).map_err(|_| BootError::Binding)?;
    Ok(hex(Sha256::digest(bytes)))
}

fn plan_hash_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn hex_len(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

#[derive(Serialize)]
struct BindingBody<'a> {
    schema: &'static str,
    plan_hash: &'a str,
    header_version: u32,
    ramdisk_format: &'a str,
    config_sha1: &'a str,
    stock_sha1: &'a str,
    stock_sha256: &'a str,
    patched_sha256: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const STOCK_SHA256: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const PLAN: &str = "flp1-test";

    #[test]
    fn gzip_v4_init_boot_matches_the_stock_sha1() {
        let image = boot_image(4, &gzip_ramdisk(&cpio_with(SHA1, true)));
        let report = inspect_patched_init_boot(&image, SHA1, STOCK_SHA256, PLAN).unwrap();
        assert_eq!(report.header_version, 4);
        assert_eq!(report.ramdisk_format, RamdiskFormat::Gzip);
        assert_eq!(report.config_sha1, SHA1);
        assert_ne!(report.patched_sha256, STOCK_SHA256);
        let other = inspect_patched_init_boot(&image, SHA1, STOCK_SHA256, "flp1-other").unwrap();
        assert_ne!(report.plan_binding, other.plan_binding);
    }

    #[test]
    fn lz4_legacy_and_raw_cpio_and_v3() {
        let legacy = boot_image(3, &lz4_legacy_ramdisk(&cpio_with(SHA1, true)));
        let report = inspect_patched_init_boot(&legacy, SHA1, STOCK_SHA256, PLAN).unwrap();
        assert_eq!(report.ramdisk_format, RamdiskFormat::Lz4Legacy);
        assert_eq!(report.header_version, 3);
        let raw = boot_image(4, &cpio_with(SHA1, true));
        let report = inspect_patched_init_boot(&raw, SHA1, STOCK_SHA256, PLAN).unwrap();
        assert_eq!(report.ramdisk_format, RamdiskFormat::Cpio);
        let dotted = cpio_newc(&[
            ("./.backup/.magisk", format!("SHA1={SHA1}\n").into_bytes()),
            ("./init", b"magisk-init".to_vec()),
        ]);
        let report =
            inspect_patched_init_boot(&boot_image(4, &dotted), SHA1, STOCK_SHA256, PLAN).unwrap();
        assert_eq!(report.config_sha1, SHA1);
    }

    #[test]
    fn lz4_frame_ramdisk_is_accepted() {
        let image = boot_image(4, &lz4_frame_ramdisk(&cpio_with(SHA1, true)));
        let report = inspect_patched_init_boot(&image, SHA1, STOCK_SHA256, PLAN).unwrap();
        assert_eq!(report.ramdisk_format, RamdiskFormat::Lz4);
    }

    #[test]
    fn missing_init_wrong_sha1_and_unknown_format_fail() {
        let no_init = boot_image(4, &gzip_ramdisk(&cpio_with(SHA1, false)));
        assert_eq!(
            inspect_patched_init_boot(&no_init, SHA1, STOCK_SHA256, PLAN).unwrap_err(),
            BootError::NoMagiskInit
        );
        let wrong = boot_image(
            4,
            &gzip_ramdisk(&cpio_with("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", true)),
        );
        assert_eq!(
            inspect_patched_init_boot(&wrong, SHA1, STOCK_SHA256, PLAN).unwrap_err(),
            BootError::StockMismatch
        );
        let xz = boot_image(4, b"\xfd7zXZ\x00");
        assert_eq!(
            inspect_patched_init_boot(&xz, SHA1, STOCK_SHA256, PLAN).unwrap_err(),
            BootError::UnsupportedRamdisk
        );
    }

    #[test]
    fn huge_sizes_do_not_allocate() {
        let mut image = vec![0u8; 64];
        image[..8].copy_from_slice(b"ANDROID!");
        image[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        image[20..24].copy_from_slice(&1584u32.to_le_bytes());
        image[40..44].copy_from_slice(&4u32.to_le_bytes());
        assert!(inspect_patched_init_boot(&image, SHA1, STOCK_SHA256, PLAN).is_err());
        assert_eq!(
            inspect_patched_init_boot(b"not a boot image", SHA1, STOCK_SHA256, PLAN).unwrap_err(),
            BootError::Header
        );
        assert_eq!(
            inspect_patched_init_boot(
                &boot_image(4, &gzip_ramdisk(&cpio_with(SHA1, true))),
                "zz",
                STOCK_SHA256,
                PLAN
            )
            .unwrap_err(),
            BootError::Binding
        );
    }

    fn boot_image(version: u32, ramdisk: &[u8]) -> Vec<u8> {
        let header_size: u32 = if version >= 4 { 1584 } else { 1580 };
        let mut header = vec![0u8; header_size as usize];
        header[..8].copy_from_slice(b"ANDROID!");
        header[12..16].copy_from_slice(&(ramdisk.len() as u32).to_le_bytes());
        header[20..24].copy_from_slice(&header_size.to_le_bytes());
        header[40..44].copy_from_slice(&version.to_le_bytes());
        let mut image = header;
        image.resize(4096, 0);
        image.extend_from_slice(ramdisk);
        let pad = (4096 - (ramdisk.len() % 4096)) % 4096;
        image.resize(image.len() + pad, 0);
        image
    }

    fn cpio_with(sha1: &str, with_init: bool) -> Vec<u8> {
        let config = format!("KEEPVERITY=true\nSHA1={sha1}\n");
        let mut files = vec![(".backup/.magisk", config.into_bytes())];
        if with_init {
            files.push(("init", b"magisk-init".to_vec()));
        }
        cpio_newc(&files)
    }

    fn cpio_newc(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (index, (name, data)) in files.iter().enumerate() {
            push_entry(&mut out, index as u32 + 1, name, data);
        }
        push_entry(&mut out, 0, "TRAILER!!!", b"");
        out
    }

    fn push_entry(out: &mut Vec<u8>, ino: u32, name: &str, data: &[u8]) {
        let name_bytes = {
            let mut bytes = name.as_bytes().to_vec();
            bytes.push(0);
            bytes
        };
        let mut header = String::from("070701");
        let fields = [
            ino,
            0o100755,
            0,
            0,
            1,
            0,
            data.len() as u32,
            0,
            0,
            0,
            0,
            name_bytes.len() as u32,
            0,
        ];
        for field in fields {
            header.push_str(&format!("{field:08x}"));
        }
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&name_bytes);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }

    fn gzip_ramdisk(bytes: &[u8]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn lz4_legacy_ramdisk(bytes: &[u8]) -> Vec<u8> {
        let compressed = lz4_flex::block::compress(bytes);
        let mut out = Vec::new();
        out.extend_from_slice(&0x184C_2102u32.to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
        out
    }

    fn lz4_frame_ramdisk(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = lz4_flex::frame::FrameEncoder::new(Vec::new());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }
}
