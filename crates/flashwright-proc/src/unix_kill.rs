// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Kill the child's process group. `pre_exec` made the child the leader.

/// Send SIGKILL to the process group whose id is `pid`.
pub(crate) fn kill_group(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    let pid = pid as i32;
    if pid <= 0 {
        return;
    }
    // SAFETY: kill(2) with a negative pid signals that process group.
    // The pid came from the child we spawned.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}
