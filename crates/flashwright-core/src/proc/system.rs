// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::proc::lines::{push_capped, tail_of, LineAssembler};
use crate::proc::spawn;
use crate::proc::{CommandRunner, Invocation, ProcError, ProcessGroup, RunResult, StdStream};

/// Spawns real processes with `tokio::process::Command` and an argument vector.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    async fn run(&self, invocation: Invocation) -> Result<RunResult, ProcError> {
        invocation.validate()?;
        let verified = crate::exe::host_utility(&invocation.program)?;
        spawn::note_spawn(verified.path(), &invocation.args);
        let started = Instant::now();
        let mut command = child_command(&verified, &invocation.args)?;
        let tools_dir = verified
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        command
            .env_remove("ANDROID_SERIAL")
            .env_remove("ADB_VENDOR_KEYS")
            .env_remove("ANDROID_ADB_SERVER_ADDRESS")
            .env_remove("ANDROID_ADB_SERVER_PORT")
            .env("ANDROID_PRODUCT_OUT", "")
            .env("PATH", path_with_tools(&tools_dir))
            .current_dir(&tools_dir)
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
            Some(crate::proc::windows_job::assign(&child)?)
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

fn child_command(
    verified: &crate::exe::VerifiedExe,
    args: &[String],
) -> Result<tokio::process::Command, crate::proc::ProcError> {
    #[cfg(all(test, unix))]
    if let Some(log) = spawn::exec_trace() {
        let strace = crate::exe::host_utility(Path::new("/usr/bin/strace"))
            .or_else(|_| crate::exe::host_utility(Path::new("/bin/strace")))?;
        let mut command = spawn::command_for(&strace);
        command
            .arg("-e")
            .arg("trace=execve")
            .arg("-o")
            .arg(log)
            .arg("--")
            .arg(verified.path())
            .args(args);
        return Ok(command);
    }
    let mut command = spawn::command_for(verified);
    command.args(args);
    Ok(command)
}

fn path_with_tools(tools_dir: &Path) -> OsString {
    let system = if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
        PathBuf::from(root).join("System32")
    } else {
        PathBuf::from("/usr/bin")
    };
    let mut path = OsString::from(tools_dir);
    path.push(if cfg!(windows) { ";" } else { ":" });
    path.push(system.as_os_str());
    path
}

async fn read_chunk<R: AsyncRead + Unpin>(pipe: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    pipe.read(buf).await
}

async fn kill_child(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    crate::proc::unix_kill::kill_group(child.id());
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::proc::{CommandRunner, Invocation};

    #[cfg(unix)]
    struct ClearTrace;

    #[cfg(unix)]
    impl Drop for ClearTrace {
        fn drop(&mut self) {
            spawn::set_exec_trace(None);
        }
    }

    /// T4.9. Linux records the real `execve` with strace and checks it against the spawn log.
    #[tokio::test]
    #[cfg(unix)]
    async fn spawn_log_matches_strace_execve() {
        let strace = if Path::new("/usr/bin/strace").is_file() {
            Path::new("/usr/bin/strace")
        } else {
            Path::new("/bin/strace")
        };
        assert!(
            strace.is_file(),
            "strace is required for the spawn-log trace"
        );
        let log = std::env::temp_dir().join(format!(
            "flashwright-strace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.subsec_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_file(&log);
        spawn::clear_spawn_log();
        spawn::set_exec_trace(Some(log.clone()));
        let _clear = ClearTrace;
        let result = SystemRunner
            .run(Invocation::tied(
                "/bin/echo",
                vec!["hello-trace".into()],
                Duration::from_secs(10),
            ))
            .await
            .expect("echo");
        assert!(result.success_exit());
        assert!(result.stdout_text().contains("hello-trace"));
        let recorded = spawn::spawn_log();
        assert!(
            recorded.iter().any(|(program, args)| {
                program == "/bin/echo" && args.as_slice() == ["hello-trace".to_string()]
            }),
            "{recorded:?}"
        );
        let text = std::fs::read_to_string(&log).expect("strace log");
        assert!(
            text.contains("execve") && text.contains("/bin/echo") && text.contains("hello-trace"),
            "{text}"
        );
        let _ = std::fs::remove_file(&log);
    }

    /// T4.9. Windows does not attach ETW here. The spawn log still names the real child.
    #[tokio::test]
    #[cfg(windows)]
    async fn spawn_log_records_the_child_without_etw() {
        eprintln!(
            "T4.9: ETW process tracing is not wired on Windows. The spawn log records the program and arguments of the real child."
        );
        spawn::clear_spawn_log();
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let program = PathBuf::from(root).join("System32").join("where.exe");
        let result = SystemRunner
            .run(Invocation::tied(
                &program,
                vec!["where".into()],
                Duration::from_secs(10),
            ))
            .await
            .expect("where");
        assert!(result.success_exit());
        let recorded = spawn::spawn_log();
        let shown = program.display().to_string();
        assert!(
            recorded.iter().any(|(path, args)| {
                path.eq_ignore_ascii_case(&shown) && args.as_slice() == ["where".to_string()]
            }),
            "{recorded:?}"
        );
    }
}
