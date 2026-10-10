// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::cmd::CatalogueCommand;
use crate::exe::VerifiedExe;
use crate::proc::lines::{push_capped, tail_of, LineAssembler};
use crate::proc::spawn;
use crate::proc::{CommandRunner, ProcError, ProcessGroup, RunLimits, RunResult, StdStream};

/// Spawns real processes from an allow-listed adb or fastboot.
///
/// The argument vector is a catalogue command. The tools directory is on
/// `PATH` and is not the working directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

/// Runs the verified adb and fastboot on this computer.
///
/// The spawn implementation stays inside this crate.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTools;

impl CommandRunner for SystemTools {
    async fn run(
        &self,
        exe: &VerifiedExe,
        command: &CatalogueCommand,
        limits: RunLimits,
    ) -> Result<RunResult, ProcError> {
        SystemRunner.run(exe, command, limits).await
    }
}

impl CommandRunner for SystemRunner {
    async fn run(
        &self,
        exe: &VerifiedExe,
        command: &CatalogueCommand,
        limits: RunLimits,
    ) -> Result<RunResult, ProcError> {
        crate::exe::recheck_allow_list(exe)?;
        spawn::note_spawn(exe.path(), command.args());
        let started = Instant::now();
        let mut child_cmd = spawn::child_command(exe, command.args())?;
        let tools_dir = exe
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        child_cmd
            .env_remove("ANDROID_SERIAL")
            .env_remove("ADB_VENDOR_KEYS")
            .env_remove("ANDROID_ADB_SERVER_ADDRESS")
            .env_remove("ANDROID_ADB_SERVER_PORT")
            .env("ANDROID_PRODUCT_OUT", "")
            .env("PATH", path_with_tools(&tools_dir))
            .current_dir(std::env::temp_dir())
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW. Avoids a console flash for each adb/fastboot call.
            child_cmd.creation_flags(0x0800_0000);
        }

        if command.group() == ProcessGroup::TiedToParent {
            #[cfg(unix)]
            {
                // SAFETY: setpgid in the child, before exec, is the documented
                // pre_exec use. The closure does not allocate or touch Rust state.
                unsafe {
                    child_cmd.pre_exec(|| {
                        if libc::setpgid(0, 0) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
        }

        let mut child = child_cmd
            .spawn()
            .map_err(|source| ProcError::spawn(exe.path(), source))?;

        #[cfg(windows)]
        let _job = if command.group() == ProcessGroup::TiedToParent {
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
        let mut saw_percent = false;
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

            let overall_left = limits.timeout.saturating_sub(started.elapsed());
            if overall_left.is_zero() {
                timed_out = true;
                kill_child(&mut child).await;
                break;
            }
            let quiet = quiet_window(&limits, saw_percent);
            let watchdog_left = quiet.map(|window| window.saturating_sub(last_output.elapsed()));
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
                            if chunk_has_percent(chunk) {
                                saw_percent = true;
                            }
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
                            if chunk_has_percent(chunk) {
                                saw_percent = true;
                            }
                            last_output = Instant::now();
                        }
                        Err(_) => stderr_open = false,
                    }
                }
                _ = tokio::time::sleep(sleep_for) => {
                    if limits.timeout.saturating_sub(started.elapsed()).is_zero() {
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
            program = %exe.path().display(),
            args = ?command.args(),
            exit = ?result.exit_code,
            duration_ms = result.duration.as_millis() as u64,
            timed_out = result.timed_out,
            killed_by_watchdog = result.killed_by_watchdog,
            "process finished"
        );
        Ok(result)
    }
}

fn quiet_window(limits: &RunLimits, saw_percent: bool) -> Option<Duration> {
    if saw_percent {
        limits.finalising.or(limits.watchdog)
    } else {
        limits.watchdog
    }
}

fn chunk_has_percent(chunk: &[u8]) -> bool {
    let text = String::from_utf8_lossy(chunk);
    let Some(start) = text.find("(~") else {
        return false;
    };
    text[start..].contains("%)")
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

#[cfg(all(test, unix))]
mod runner_tests {
    use std::os::unix::fs::symlink;
    use std::time::{Duration, Instant};

    use std::path::Path;

    use sha2::{Digest, Sha256};

    use crate::cmd::{CatalogueCommand, Tool};
    use crate::exe::{measure_platform_tool, platform_tool, scripted_tool, VerifiedExe};
    use crate::proc::{CommandRunner, RunLimits, SystemRunner};

    fn allow_listed(path: &Path) -> VerifiedExe {
        let bytes = std::fs::read(path).unwrap();
        let hash = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        platform_tool(path, &hash).unwrap()
    }

    fn limits(
        timeout: Duration,
        watchdog: Option<Duration>,
        finalising: Option<Duration>,
    ) -> RunLimits {
        RunLimits {
            timeout,
            watchdog,
            finalising,
        }
    }

    #[tokio::test]
    async fn measured_copies_obey_the_timeout_and_the_watchdog() {
        let dir = std::env::temp_dir().join(format!("fw-runner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let echo = dir.join("adb");
        std::fs::copy("/bin/echo", &echo).unwrap();
        let exe = allow_listed(&echo);
        let runner = SystemRunner;
        let echoed = runner
            .run(
                &exe,
                &CatalogueCommand::for_test(Tool::Adb, vec!["hello".into()]),
                limits(Duration::from_secs(5), None, None),
            )
            .await
            .unwrap();
        assert_eq!(echoed.stdout_text().trim(), "hello");

        let sleep_path = dir.join("fastboot");
        std::fs::copy("/bin/sleep", &sleep_path).unwrap();
        let sleep_exe = allow_listed(&sleep_path);
        let started = Instant::now();
        let timed = runner
            .run(
                &sleep_exe,
                &CatalogueCommand::for_test(Tool::Fastboot, vec!["30".into()]),
                limits(Duration::from_millis(200), None, None),
            )
            .await
            .unwrap();
        assert!(timed.timed_out);
        assert!(started.elapsed() < Duration::from_secs(3));

        let started = Instant::now();
        let watched = runner
            .run(
                &sleep_exe,
                &CatalogueCommand::for_test(Tool::Fastboot, vec!["30".into()]),
                limits(
                    Duration::from_secs(5),
                    Some(Duration::from_millis(150)),
                    None,
                ),
            )
            .await
            .unwrap();
        assert!(watched.killed_by_watchdog);
        assert!(started.elapsed() < Duration::from_secs(3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_percent_line_opens_the_finalising_window() {
        let dir = std::env::temp_dir().join(format!("fw-final-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("adb");
        std::fs::copy("/bin/sh", &program).unwrap();
        let exe = allow_listed(&program);
        let started = Instant::now();
        let result = SystemRunner
            .run(
                &exe,
                &CatalogueCommand::for_test(
                    Tool::Adb,
                    vec!["-c".into(), "echo '(~47%)'; exec sleep 30".into()],
                ),
                limits(
                    Duration::from_secs(5),
                    Some(Duration::from_millis(200)),
                    Some(Duration::from_millis(1200)),
                ),
            )
            .await
            .unwrap();
        assert!(result.killed_by_watchdog);
        assert!(started.elapsed() >= Duration::from_millis(900));
        assert!(started.elapsed() < Duration::from_secs(4));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn interpreters_and_a_shell_symlink_are_refused() {
        let dir = std::env::temp_dir().join(format!("fw-deny-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["adb-helper", "python3", "perl", "node"] {
            let path = dir.join(name);
            std::fs::write(&path, b"not-a-tool").unwrap();
            assert!(measure_platform_tool(&path).is_err(), "{name}");
            let scripted = scripted_tool(&path).unwrap();
            let err = SystemRunner
                .run(
                    &scripted,
                    &CatalogueCommand::for_test(Tool::Adb, vec!["version".into()]),
                    limits(Duration::from_secs(1), None, None),
                )
                .await
                .unwrap_err();
            assert!(err.to_string().contains("adb or fastboot"), "{name}");
        }
        let link = dir.join("adb");
        symlink("/bin/sh", &link).unwrap();
        assert!(measure_platform_tool(&link).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_hashed_copy_is_refused_until_the_allow_list_matches() {
        let dir = std::env::temp_dir().join(format!("fw-copy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("adb");
        std::fs::copy("/bin/echo", &program).unwrap();
        let measured = measure_platform_tool(&program).unwrap();
        let err = SystemRunner
            .run(
                &measured,
                &CatalogueCommand::for_test(Tool::Adb, vec!["hello".into()]),
                limits(Duration::from_secs(2), None, None),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("allow-listed"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_hard_link_and_a_replaced_copy_are_refused() {
        let dir = std::env::temp_dir().join(format!("fw-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("echo-bin");
        std::fs::copy("/bin/echo", &source).unwrap();
        let link = dir.join("adb");
        std::fs::hard_link(&source, &link).unwrap();
        let err = measure_platform_tool(&link).unwrap_err();
        assert!(err.to_string().contains("hard link"), "{err}");

        std::fs::remove_file(&link).unwrap();
        std::fs::copy("/bin/echo", &link).unwrap();
        let exe = allow_listed(&link);
        // Keep the measured inode allocated so the replacement cannot reuse it.
        let held = std::fs::File::open(&link).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::fs::copy("/bin/echo", &link).unwrap();
        drop(held);
        let err = SystemRunner
            .run(
                &exe,
                &CatalogueCommand::for_test(Tool::Adb, vec!["hello".into()]),
                limits(Duration::from_secs(2), None, None),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("copy"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_tools_directory_is_not_the_working_directory() {
        let dir = std::env::temp_dir().join(format!("fw-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("adb");
        std::fs::copy("/bin/sh", &program).unwrap();
        let exe = allow_listed(&program);
        let result = SystemRunner
            .run(
                &exe,
                &CatalogueCommand::for_test(Tool::Adb, vec!["-c".into(), "pwd".into()]),
                limits(Duration::from_secs(2), None, None),
            )
            .await
            .unwrap();
        let cwd = result.stdout_text().trim().to_string();
        assert_ne!(cwd, dir.to_string_lossy());
        assert!(!cwd.contains("fw-cwd-"));
        let _ = std::fs::remove_dir_all(&dir);
    }
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
    use crate::cmd::{CatalogueCommand, Rendered, Tool};
    use crate::exe::{platform_tool, VerifiedExe};
    use crate::proc::CommandRunner;

    fn allow_listed_copy(path: &Path) -> VerifiedExe {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(std::fs::read(path).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        platform_tool(path, &hash).unwrap()
    }

    fn limits() -> RunLimits {
        RunLimits {
            timeout: Duration::from_secs(10),
            watchdog: None,
            finalising: None,
        }
    }

    fn command(args: Vec<String>) -> CatalogueCommand {
        CatalogueCommand::from_rendered(Rendered {
            tool: Tool::Adb,
            args,
        })
    }

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
        let dir = std::env::temp_dir().join(format!("fw-trace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("adb");
        std::fs::copy("/bin/echo", &program).unwrap();
        let exe = allow_listed_copy(&program);
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
            .run(&exe, &command(vec!["hello-trace".into()]), limits())
            .await
            .expect("echo");
        assert!(result.success_exit());
        assert!(result.stdout_text().contains("hello-trace"));
        let shown = program.display().to_string();
        let recorded = spawn::spawn_log();
        assert!(
            recorded.iter().any(|(path, args)| {
                path == &shown && args.as_slice() == ["hello-trace".to_string()]
            }),
            "{recorded:?}"
        );
        let text = std::fs::read_to_string(&log).expect("strace log");
        assert!(
            text.contains("execve") && text.contains(&shown) && text.contains("hello-trace"),
            "{text}"
        );
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_dir_all(&dir);
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
        let source = PathBuf::from(root).join("System32").join("where.exe");
        let dir = std::env::temp_dir().join(format!("fw-trace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("adb.exe");
        std::fs::copy(&source, &program).unwrap();
        let exe = allow_listed_copy(&program);
        let result = SystemRunner
            .run(&exe, &command(vec!["where".into()]), limits())
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
        let _ = std::fs::remove_dir_all(&dir);
    }
}
