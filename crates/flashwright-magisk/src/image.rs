// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Seam for an init_boot or boot image extracted by the image pipeline.
//!
//! The image pipeline is built separately. Tests use [`SyntheticInitBoot`].

use flashwright_core::device::Partition;
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// Stock image the patch plan pushes. The image pipeline implements this.
pub trait ExtractedBootImage {
    fn partition(&self) -> Partition;
    fn bytes(&self) -> &[u8];
    fn sha256_hex(&self) -> &str;
    fn sha1_hex(&self) -> &str;

    fn size_bytes(&self) -> u64 {
        u64::try_from(self.bytes().len()).unwrap_or(u64::MAX)
    }
}

/// Synthetic boot image. The bytes start with the Android boot magic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntheticInitBoot {
    partition: Partition,
    bytes: Vec<u8>,
    sha256: String,
    sha1: String,
}

impl SyntheticInitBoot {
    /// Pixel 9 Pro XL stock stand-in. The partition is init_boot.
    pub fn komodo() -> Self {
        Self::from_bytes(
            Partition::InitBoot,
            b"ANDROID!flashwright-synthetic-komodo-init_boot",
        )
    }

    /// A boot image, used to show komodo rejects that partition.
    pub fn boot_fixture() -> Self {
        Self::from_bytes(Partition::Boot, b"ANDROID!flashwright-synthetic-boot")
    }

    pub fn from_bytes(partition: Partition, body: &[u8]) -> Self {
        Self {
            partition,
            sha256: hex(Sha256::digest(body)),
            sha1: hex(Sha1::digest(body)),
            bytes: body.to_vec(),
        }
    }
}

impl ExtractedBootImage for SyntheticInitBoot {
    fn partition(&self) -> Partition {
        self.partition
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn sha256_hex(&self) -> &str {
        &self.sha256
    }

    fn sha1_hex(&self) -> &str {
        &self.sha1
    }
}

pub(crate) fn hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

pub(crate) fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.chars().all(|ch| ch.is_ascii_hexdigit())
}
