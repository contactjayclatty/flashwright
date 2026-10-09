// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Working-volume free space. Need is the inner zip, the extracted image, and 1 GiB.

use std::path::Path;

use crate::error::FirmwareError;

const GIB: u64 = 1024 * 1024 * 1024;
#[cfg(test)]
const MIB: u64 = 1024 * 1024;

pub fn working_need(inner_zip_bytes: u64, image_bytes: u64) -> u64 {
    inner_zip_bytes
        .saturating_add(image_bytes)
        .saturating_add(GIB)
}

pub fn space_is_sufficient(free_bytes: u64, need: u64) -> bool {
    free_bytes >= need
}

pub fn ensure_free_space(path: &Path, need: u64) -> Result<(), FirmwareError> {
    let free = volume_free(path)?;
    if space_is_sufficient(free, need) {
        Ok(())
    } else {
        Err(FirmwareError::NoSpace)
    }
}

#[cfg(unix)]
fn volume_free(path: &Path) -> Result<u64, FirmwareError> {
    let probe = if path.exists() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };
    let text = probe.to_string_lossy();
    let c_path = std::ffi::CString::new(text.as_bytes()).map_err(|_| FirmwareError::NoSpace)?;
    let mut stat = unsafe { std::mem::zeroed::<libc::statvfs>() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return Err(FirmwareError::io(std::io::Error::last_os_error()));
    }
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(windows)]
fn volume_free(path: &Path) -> Result<u64, FirmwareError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let probe = if path.exists() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };
    let wide: Vec<u16> = probe.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free as *mut u64),
            None,
            None,
        )
        .map_err(|_| FirmwareError::NoSpace)?;
    }
    Ok(free)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_mib_short_of_need_is_refused() {
        let need = working_need(50, 8);
        assert!(!space_is_sufficient(need - MIB, need));
        assert!(space_is_sufficient(need, need));
    }

    #[test]
    fn a_huge_need_is_refused_on_this_volume() {
        let err = ensure_free_space(Path::new("."), u64::MAX).unwrap_err();
        assert!(err.to_string().contains("G12"));
    }
}
