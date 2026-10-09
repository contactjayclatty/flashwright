// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Ramdisk decompression. Supported inputs are gzip, lz4 legacy, lz4 frames,
//! and an uncompressed newc cpio. Anything else fails closed.

use std::io::Read;

use flate2::read::GzDecoder;

use crate::error::BootError;

const MAX_OUT: usize = 32 * 1024 * 1024;
const LZ4_LEGACY_BLOCK: usize = 0x800000;
const MAX_LZ4_BLOCKS: usize = 8;
const LZ4_LEGACY_MAGIC: u32 = 0x184C_2102;
const LZ4_FRAME_MAGIC: u32 = 0x184D_2204;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RamdiskFormat {
    Gzip,
    Lz4Legacy,
    Lz4,
    Cpio,
}

impl RamdiskFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Lz4Legacy => "lz4_legacy",
            Self::Lz4 => "lz4",
            Self::Cpio => "cpio",
        }
    }
}

pub(crate) fn decompress(input: &[u8]) -> Result<(RamdiskFormat, Vec<u8>), BootError> {
    let format = detect(input)?;
    let bytes = match format {
        RamdiskFormat::Gzip => gzip(input)?,
        RamdiskFormat::Lz4Legacy => lz4_legacy(input)?,
        RamdiskFormat::Lz4 => lz4_frame(input)?,
        RamdiskFormat::Cpio => {
            if input.len() > MAX_OUT {
                return Err(BootError::TooLarge);
            }
            input.to_vec()
        }
    };
    if bytes.is_empty() || bytes.len() > MAX_OUT {
        return Err(BootError::TooLarge);
    }
    Ok((format, bytes))
}

fn detect(input: &[u8]) -> Result<RamdiskFormat, BootError> {
    if input.len() >= 2 && input[0] == 0x1f && input[1] == 0x8b {
        return Ok(RamdiskFormat::Gzip);
    }
    if input.len() >= 4 {
        let magic = u32::from_le_bytes([input[0], input[1], input[2], input[3]]);
        if magic == LZ4_LEGACY_MAGIC {
            return Ok(RamdiskFormat::Lz4Legacy);
        }
        if magic == LZ4_FRAME_MAGIC {
            return Ok(RamdiskFormat::Lz4);
        }
    }
    if input.len() >= 6 && &input[..6] == b"070701" {
        return Ok(RamdiskFormat::Cpio);
    }
    Err(BootError::UnsupportedRamdisk)
}

fn gzip(input: &[u8]) -> Result<Vec<u8>, BootError> {
    let mut decoder = GzDecoder::new(input);
    read_capped(&mut decoder)
}

fn lz4_frame(input: &[u8]) -> Result<Vec<u8>, BootError> {
    let mut decoder = lz4_flex::frame::FrameDecoder::new(input);
    read_capped(&mut decoder)
}

fn read_capped(reader: &mut impl Read) -> Result<Vec<u8>, BootError> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = reader
            .read(&mut buf)
            .map_err(|_| BootError::UnsupportedRamdisk)?;
        if read == 0 {
            break;
        }
        if out.len().saturating_add(read) > MAX_OUT {
            return Err(BootError::TooLarge);
        }
        out.extend_from_slice(&buf[..read]);
    }
    Ok(out)
}

fn lz4_legacy(input: &[u8]) -> Result<Vec<u8>, BootError> {
    if input.len() < 4 {
        return Err(BootError::UnsupportedRamdisk);
    }
    let mut pos = 4usize;
    let mut out = Vec::new();
    let mut block_out = vec![0u8; LZ4_LEGACY_BLOCK];
    let mut blocks = 0usize;
    while pos < input.len() {
        if blocks >= MAX_LZ4_BLOCKS {
            return Err(BootError::TooLarge);
        }
        let end = pos.checked_add(4).ok_or(BootError::UnsupportedRamdisk)?;
        let size_bytes: [u8; 4] = input
            .get(pos..end)
            .ok_or(BootError::UnsupportedRamdisk)?
            .try_into()
            .map_err(|_| BootError::UnsupportedRamdisk)?;
        let block_size = u32::from_le_bytes(size_bytes) as usize;
        pos = end;
        if block_size == 0 {
            break;
        }
        if block_size > LZ4_LEGACY_BLOCK {
            return Err(BootError::TooLarge);
        }
        let data_end = pos
            .checked_add(block_size)
            .ok_or(BootError::UnsupportedRamdisk)?;
        let compressed = input
            .get(pos..data_end)
            .ok_or(BootError::UnsupportedRamdisk)?;
        let written = lz4_flex::block::decompress_into(compressed, &mut block_out)
            .map_err(|_| BootError::UnsupportedRamdisk)?;
        if written == 0 || out.len().saturating_add(written) > MAX_OUT {
            return Err(BootError::TooLarge);
        }
        out.extend_from_slice(&block_out[..written]);
        pos = data_end;
        blocks += 1;
    }
    if blocks == 0 {
        return Err(BootError::UnsupportedRamdisk);
    }
    Ok(out)
}
