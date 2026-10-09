// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Identify the process listening on a loopback TCP port.
//!
//! The adb server check compares this image with the verified adb. If the
//! listener cannot be named, the result is `None` and the write stays blocked.

use std::path::PathBuf;

use crate::exe::{hash_regular_file, ListenerImage};

/// Path and SHA-256 of the process listening on `127.0.0.1:port`.
///
/// A missing table, an unreadable executable, or no loopback listener is
/// `None`. Callers treat that as "the adb server was not identified".
pub(crate) fn listener_on_port(port: u16) -> Option<ListenerImage> {
    let path = listener_path(port)?;
    let sha256 = hash_regular_file(&path).ok()?;
    Some(ListenerImage { path, sha256 })
}

fn listener_path(port: u16) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        let inode = unix_listen_inode(port)?;
        unix_exe_for_inode(inode)
    }
    #[cfg(windows)]
    {
        windows_listener_path(port)
    }
}

#[cfg(unix)]
fn unix_listen_inode(port: u16) -> Option<u64> {
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        if let Some(inode) = inode_in_tcp_table(&text, port) {
            return Some(inode);
        }
    }
    None
}

/// `local_address` is `ADDR:PORT` in hex. State `0A` is listen.
#[cfg(any(unix, test))]
pub(crate) fn inode_in_tcp_table(text: &str, port: u16) -> Option<u64> {
    for line in text.lines().skip(1) {
        let mut cols = line.split_whitespace();
        let _slot = cols.next()?;
        let local = cols.next()?;
        let _remote = cols.next()?;
        let state = cols.next()?;
        if !state.eq_ignore_ascii_case("0A") {
            continue;
        }
        let (addr, local_port) = local.split_once(':')?;
        let local_port = u16::from_str_radix(local_port, 16).ok()?;
        if local_port != port || !loopback_address(addr) {
            continue;
        }
        // tx, tr, retrnsmt, uid, timeout, then inode.
        let inode = cols.nth(5)?;
        let inode = inode.parse().ok()?;
        if inode > 0 {
            return Some(inode);
        }
    }
    None
}

#[cfg(any(unix, test))]
fn loopback_address(addr: &str) -> bool {
    let lower = addr.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        // 127.0.0.1 and 0.0.0.0, little-endian, from /proc/net/tcp.
        "0100007f" | "00000000"
        // ::1 and :: from /proc/net/tcp6.
        | "00000000000000000000000001000000"
        | "00000000000000000000000000000000"
    )
}

#[cfg(unix)]
fn unix_exe_for_inode(inode: u64) -> Option<PathBuf> {
    let needle = format!("socket:[{inode}]");
    let proc = std::fs::read_dir("/proc").ok()?;
    for entry in proc.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let fd_dir = entry.path().join("fd");
        let Ok(fds) = std::fs::read_dir(fd_dir) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };
            if target.as_os_str() != std::ffi::OsStr::new(&needle) {
                continue;
            }
            let exe = std::fs::read_link(entry.path().join("exe")).ok()?;
            if exe.is_absolute() {
                return Some(exe);
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows_listener_path(port: u16) -> Option<PathBuf> {
    let pid = windows_listener_pid(port)?;
    process_image(pid)
}

#[cfg(windows)]
fn windows_listener_pid(port: u16) -> Option<u32> {
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(
            table: *mut u8,
            size: *mut u32,
            order: i32,
            family: u32,
            class: i32,
            reserved: u32,
        ) -> u32;
    }
    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_LISTENER: i32 = 3;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const NO_ERROR: u32 = 0;
    let mut size = 0u32;
    let probe = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if probe != ERROR_INSUFFICIENT_BUFFER && probe != NO_ERROR {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    let rc = unsafe {
        GetExtendedTcpTable(
            buffer.as_mut_ptr(),
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if rc != NO_ERROR || buffer.len() < 4 {
        return None;
    }
    let count = u32::from_le_bytes(buffer[0..4].try_into().ok()?) as usize;
    let row = 24usize;
    for index in 0..count {
        let start = 4 + index * row;
        let end = start + row;
        if end > buffer.len() {
            return None;
        }
        let local_addr = u32::from_le_bytes(buffer[start + 4..start + 8].try_into().ok()?);
        let local_port = u32::from_le_bytes(buffer[start + 8..start + 12].try_into().ok()?);
        let pid = u32::from_le_bytes(buffer[start + 20..start + 24].try_into().ok()?);
        let port_bits = (local_port & 0xffff) as u16;
        let host_port = u16::from_be(port_bits);
        // 127.0.0.1 in the table is the native DWORD `inet_addr` returns.
        if host_port == port && (local_addr == 0x0100_007f || local_addr == 0) {
            return Some(pid);
        }
    }
    None
}

#[cfg(windows)]
fn process_image(pid: u32) -> Option<PathBuf> {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
        fn QueryFullProcessImageNameW(
            process: *mut core::ffi::c_void,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut words = vec![0u16; 32 * 1024];
    let mut len = words.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, words.as_mut_ptr(), &mut len) };
    unsafe {
        CloseHandle(handle);
    }
    if ok == 0 {
        return None;
    }
    let text = String::from_utf16_lossy(&words[..len as usize]);
    let path = PathBuf::from(text);
    if path.is_absolute() {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_table_finds_loopback_listen() {
        let text = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 4242 1 0000000000000000 100 0 0 10 0
   1: 00000000:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 99 1 0000000000000000 100 0 0 10 0
";
        assert_eq!(inode_in_tcp_table(text, 8080), Some(4242));
        assert_eq!(inode_in_tcp_table(text, 80), Some(99));
        assert_eq!(inode_in_tcp_table(text, 5037), None);
    }

    #[cfg(unix)]
    #[test]
    fn live_loopback_listener_is_this_process() {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let image = listener_on_port(port).expect("listener image");
        let exe = std::fs::read_link("/proc/self/exe").unwrap();
        assert_eq!(image.path, exe);
        let again = hash_regular_file(&exe).unwrap();
        assert_eq!(image.sha256, again);
        drop(socket);
    }
}
