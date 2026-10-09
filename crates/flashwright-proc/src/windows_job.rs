// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Assign a child to a job that dies when this handle is closed.
//! `adb start-server` does not use this, so the server can keep running.

use std::mem::size_of;

use tokio::process::Child;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::IO_COUNTERS;

use crate::ProcError;

pub(crate) struct JobGuard {
    handle: HANDLE,
}

// SAFETY: the job handle is owned here and closed once in Drop. Moving the
// guard to another thread does not alias it.
unsafe impl Send for JobGuard {}

impl Drop for JobGuard {
    fn drop(&mut self) {
        // SAFETY: this guard is the only closer of the job handle.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

pub(crate) fn assign(child: &Child) -> Result<JobGuard, ProcError> {
    // SAFETY: CreateJobObjectW / SetInformationJobObject / AssignProcessToJobObject
    // are called with a live process handle and a zeroed limit struct whose
    // only flag is KILL_ON_JOB_CLOSE.
    unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null())
            .map_err(|err| ProcError::Job(err.to_string()))?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            IoInfo: IO_COUNTERS::default(),
            ..Default::default()
        };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(|err| ProcError::Job(err.to_string()))?;

        let raw = child
            .raw_handle()
            .ok_or_else(|| ProcError::Job("child has no process handle".into()))?;
        let process = HANDLE(raw as *mut _);
        AssignProcessToJobObject(job, process).map_err(|err| ProcError::Job(err.to_string()))?;
        Ok(JobGuard { handle: job })
    }
}
