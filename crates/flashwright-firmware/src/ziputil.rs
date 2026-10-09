// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Zip helpers. A stored entry is read as a sub-range of the outer file.
//! A deflated entry is streamed to the work directory in 64 KiB buffers.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::error::FirmwareError;

pub const COPY_CHUNK: usize = 64 * 1024;

pub fn open_zip(path: &Path) -> Result<ZipArchive<File>, FirmwareError> {
    let file = File::open(path).map_err(FirmwareError::io)?;
    ZipArchive::new(file).map_err(|err| map_zip(err))
}

pub fn entry_name<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> String {
    entry
        .name()
        .map(|value| value.to_string())
        .unwrap_or_default()
        .replace('\\', "/")
}

pub fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

pub fn map_zip(err: zip::result::ZipError) -> FirmwareError {
    let text = err.to_string();
    if text.to_ascii_lowercase().contains("crc") {
        FirmwareError::Archive("CRC-32 check failed".into())
    } else {
        FirmwareError::Truncated
    }
}

pub fn stream_entry<R: Read>(
    entry: &mut zip::read::ZipFile<'_, R>,
    dest: &Path,
) -> Result<(), FirmwareError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(FirmwareError::io)?;
    }
    let mut output = File::create(dest).map_err(FirmwareError::io)?;
    let mut buf = vec![0u8; COPY_CHUNK];
    loop {
        let read = entry.read(&mut buf).map_err(FirmwareError::io)?;
        if read == 0 {
            break;
        }
        output.write_all(&buf[..read]).map_err(FirmwareError::io)?;
    }
    output.flush().map_err(FirmwareError::io)?;
    Ok(())
}

/// Byte range of a stored local file, after the local header.
pub fn stored_data_range(file: &mut File, header_start: u64) -> Result<(u64, u64), FirmwareError> {
    file.seek(SeekFrom::Start(header_start))
        .map_err(FirmwareError::io)?;
    let mut local = [0u8; 30];
    file.read_exact(&mut local).map_err(FirmwareError::io)?;
    if &local[0..4] != b"PK\x03\x04" {
        return Err(FirmwareError::Truncated);
    }
    let name_len = u16::from_le_bytes([local[26], local[27]]) as u64;
    let extra_len = u16::from_le_bytes([local[28], local[29]]) as u64;
    let start = header_start + 30 + name_len + extra_len;
    Ok((start, name_len))
}

pub struct FileWindow {
    file: File,
    start: u64,
    len: u64,
    pos: u64,
}

impl FileWindow {
    pub fn new(file: File, start: u64, len: u64) -> Self {
        Self {
            file,
            start,
            len,
            pos: 0,
        }
    }
}

impl Read for FileWindow {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let max = ((self.len - self.pos) as usize).min(buf.len());
        self.file.seek(SeekFrom::Start(self.start + self.pos))?;
        let read = self.file.read(&mut buf[..max])?;
        self.pos += read as u64;
        Ok(read)
    }
}

impl Seek for FileWindow {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(value) => value as i64,
            SeekFrom::End(value) => self.len as i64 + value,
            SeekFrom::Current(value) => self.pos as i64 + value,
        };
        if next < 0 || next as u64 > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek outside the stored entry",
            ));
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

pub fn windows_flash_name() -> &'static str {
    concat!("flash-all.", "bat")
}

pub fn is_image_zip(name: &str) -> bool {
    let base = base_name(name);
    base.starts_with("image-") && base.ends_with(".zip")
}

pub fn temp_inner(dir: &Path) -> PathBuf {
    dir.join("image.zip")
}
