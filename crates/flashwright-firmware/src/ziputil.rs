// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Zip helpers. A stored entry is read as a sub-range of the already-open file.
//! A deflated entry is streamed to the work directory. Output files are created
//! without following a symlink.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::error::FirmwareError;

pub const COPY_CHUNK: usize = 64 * 1024;

pub fn copy_chunk_len() -> usize {
    COPY_CHUNK.saturating_mul(crate::open::extraction_workers())
}

pub fn entry_name<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> Result<String, FirmwareError> {
    let name = entry.name().map_err(map_zip)?;
    Ok(name.replace('\\', "/"))
}

pub fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

pub fn map_zip(err: zip::result::ZipError) -> FirmwareError {
    let text = err.to_string();
    let lowered = text.to_ascii_lowercase();
    if lowered.contains("crc") || lowered.contains("checksum") {
        FirmwareError::Crc
    } else {
        FirmwareError::Truncated
    }
}

pub fn map_read(err: io::Error) -> FirmwareError {
    let text = err.to_string().to_ascii_lowercase();
    if text.contains("crc") || text.contains("checksum") {
        FirmwareError::Crc
    } else {
        FirmwareError::io(err)
    }
}

/// Create `path` for writing. An existing name, including a symlink, is refused.
pub fn create_output(path: &Path) -> Result<File, FirmwareError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(FirmwareError::io)?;
        }
    }
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT
        options.custom_flags(0x0020_0000);
    }
    options.open(path).map_err(FirmwareError::io)
}

/// Copy until `reader` ends. A chunk that would pass `cap` is refused before it is written.
pub fn copy_capped<R: Read>(
    reader: &mut R,
    output: &mut File,
    cap: u64,
    too_big: &'static str,
) -> Result<u64, FirmwareError> {
    let mut buf = vec![0u8; copy_chunk_len()];
    let mut written = 0u64;
    loop {
        let read = reader.read(&mut buf).map_err(map_read)?;
        if read == 0 {
            break;
        }
        let next = written.saturating_add(read as u64);
        if next > cap {
            return Err(FirmwareError::Archive(too_big.into()));
        }
        output.write_all(&buf[..read]).map_err(FirmwareError::io)?;
        written = next;
    }
    output.flush().map_err(FirmwareError::io)?;
    Ok(written)
}

pub fn read_limited<R: Read>(
    reader: &mut R,
    declared: u64,
    cap: u64,
    too_big: &'static str,
) -> Result<Vec<u8>, FirmwareError> {
    if declared > cap {
        return Err(FirmwareError::Archive(too_big.into()));
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = reader.read(&mut buf).map_err(map_read)?;
        if read == 0 {
            break;
        }
        if (out.len() as u64).saturating_add(read as u64) > cap {
            return Err(FirmwareError::Archive(too_big.into()));
        }
        out.extend_from_slice(&buf[..read]);
    }
    Ok(out)
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

#[derive(Clone, Debug)]
pub struct ListedEntry {
    pub index: usize,
    pub name: String,
    pub size: u64,
    pub compressed_size: u64,
    pub compression: zip::CompressionMethod,
    pub data_start: Option<u64>,
    pub header_start: u64,
    pub is_dir: bool,
}

pub fn list_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<ListedEntry>, FirmwareError> {
    let mut out = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(map_zip)?;
        out.push(ListedEntry {
            index,
            name: entry_name(&entry)?,
            size: entry.size(),
            compressed_size: entry.compressed_size(),
            compression: entry.compression(),
            data_start: entry.data_start(),
            header_start: entry.header_start(),
            is_dir: entry.is_dir(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Chunks {
        data: Vec<u8>,
        pos: usize,
        chunk: usize,
    }

    impl Read for Chunks {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() {
                return Ok(0);
            }
            let count = (self.data.len() - self.pos).min(self.chunk).min(buf.len());
            buf[..count].copy_from_slice(&self.data[self.pos..self.pos + count]);
            self.pos += count;
            Ok(count)
        }
    }

    #[test]
    fn copy_stops_before_the_overflowing_chunk() {
        let dir = std::env::temp_dir().join(format!("fw-cap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("out.bin");
        let _ = std::fs::remove_file(&path);
        let mut output = create_output(&path).unwrap();
        let mut reader = Chunks {
            data: vec![7u8; 200],
            pos: 0,
            chunk: 40,
        };
        let err = copy_capped(&mut reader, &mut output, 100, "too big").unwrap_err();
        assert!(matches!(err, FirmwareError::Archive(_)));
        output.flush().unwrap();
        let len = output.metadata().unwrap().len();
        assert!(len <= 100);
        assert_eq!(len, 80);
        drop(output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn declared_metadata_over_the_cap_is_refused_before_the_body() {
        let mut reader = Cursor::new(vec![1u8; 32]);
        let err = read_limited(&mut reader, 2_000_000, 1024, "too big").unwrap_err();
        assert!(matches!(err, FirmwareError::Archive(_)));
        assert_eq!(reader.position(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_destination_is_refused() {
        let dir = std::env::temp_dir().join(format!("fw-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("safe.txt");
        std::fs::write(&target, b"keep").unwrap();
        let link = dir.join("init_boot.img");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(create_output(&link).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
