// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::lines::{push_capped, tail_of, LineAssembler};
use crate::{CommandRunner, Invocation, ProcError, ProcessGroup, RunResult, StdStream};

/// Spawns real processes with `tokio::process::Command` and an argument vector.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    async fn run(&self, invocation: Invocation) -> Result<RunResult, ProcError> {
        invocation.validate()?;
        let started = Instant::now();
        let mut command = Command::new(&invocation.program);
        command
            .args(&invocation.args)
            .env_remove("ANDROID_SERIAL")
            .env("ANDROID_PRODUCT_OUT", "")
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW. Avoids a console flash for each adb/fastboot call.
            command.creation_flags(0x0800_0000);
        }

        if invocation.group == ProcessGroup::TiedToParent {
            #[cfg(unix)]
            {
                // SAFETY: setpgid in the child, before exec, is the documented
                // pre_exec use. The closure does not allocate or touch Rust state.
                unsafe {
                    command.pre_exec(|| {
                        if libc::setpgid(0, 0) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
        }

        let mut child = command
            .spawn()
            .map_err(|source| ProcError::spawn(&invocation.program, source))?;

        #[cfg(windows)]
        let _job = if invocation.group == ProcessGroup::TiedToParent {
            Some(crate::windows_job::assign(&child)?)
        } else {
            None
        };

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProcError::Io(std::io::Error::other("child stdout was not piped")))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProcError::Io(std::io::Error::other("child stderr was not piped")))?;

        let mut out_buf = Vec::new();
        let mut err_buf = Vec::new();
        let mut out_trunc = false;
        let mut err_trunc = false;
        let mut lines = LineAssembler::default();
        let mut stdout_open = true;
        let mut stderr_open = true;
        let mut last_output = Instant::now();
        let mut timed_out = false;
        let mut killed_by_watchdog = false;
        let mut exit_code = None;
        let mut scratch_out = [0u8; 8192];
        let mut scratch_err = [0u8; 8192];

        loop {
            if !stdout_open && !stderr_open {
                let status = child.wait().await?;
                exit_code = status.code();
                break;
            }

            let overall_left = invocation.timeout.saturating_sub(started.elapsed());
            if overall_left.is_zero() {
                timed_out = true;
                kill_child(&mut child).await;
                break;
            }
            let watchdog_left = invocation
                .watchdog
                .map(|window| window.saturating_sub(last_output.elapsed()));
            if matches!(watchdog_left, Some(left) if left.is_zero()) {
                killed_by_watchdog = true;
                kill_child(&mut child).await;
                break;
            }
            let sleep_for = match watchdog_left {
                Some(left) => overall_left.min(left),
                None => overall_left,
            };

            tokio::select! {
                biased;
                read = read_chunk(&mut stdout, &mut scratch_out), if stdout_open => {
                    match read {
                        Ok(0) => stdout_open = false,
                        Ok(n) => {
                            let chunk = &scratch_out[..n];
                            push_capped(&mut out_buf, chunk, &mut out_trunc);
                            lines.push(StdStream::Stdout, chunk, started.elapsed());
                            last_output = Instant::now();
                        }
                        Err(_) => stdout_open = false,
                    }
                }
                read = read_chunk(&mut stderr, &mut scratch_err), if stderr_open => {
                    match read {
                        Ok(0) => stderr_open = false,
                        Ok(n) => {
                            let chunk = &scratch_err[..n];
                            push_capped(&mut err_buf, chunk, &mut err_trunc);
                            lines.push(StdStream::Stderr, chunk, started.elapsed());
                            last_output = Instant::now();
                        }
                        Err(_) => stderr_open = false,
                    }
                }
                _ = tokio::time::sleep(sleep_for) => {
                    if invocation.timeout.saturating_sub(started.elapsed()).is_zero() {
                        timed_out = true;
                    } else {
                        killed_by_watchdog = true;
                    }
                    kill_child(&mut child).await;
                    break;
                }
            }
        }

        let duration = started.elapsed();
        let result = RunResult {
            exit_code,
            duration,
            stdout_tail: tail_of(&out_buf),
            stderr_tail: tail_of(&err_buf),
            stdout: out_buf,
            stderr: err_buf,
            stdout_truncated: out_trunc,
            stderr_truncated: err_trunc,
            lines: lines.into_lines(duration),
            timed_out,
            killed_by_watchdog,
        };
        tracing::info!(
            program = %invocation.program.display(),
            args = ?invocation.args,
            exit = ?result.exit_code,
            duration_ms = result.duration.as_millis() as u64,
            timed_out = result.timed_out,
            killed_by_watchdog = result.killed_by_watchdog,
            "process finished"
        );
        Ok(result)
    }
}

async fn read_chunk<R: AsyncRead + Unpin>(pipe: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    pipe.read(buf).await
}

async fn kill_child(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    crate::unix_kill::kill_group(child.id());
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
}
