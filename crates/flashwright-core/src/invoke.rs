// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Adapt the process runner to the platform-tools locator.

use std::path::Path;
use std::time::Duration;

use flashwright_tools::{ToolInvoker, ToolOutput, ToolsError};

use crate::cmd::{read_argv, AdbHostRead, CatalogueCommand, FastbootRead, ReadCmd};
use crate::exe::measure_platform_tool;
use crate::proc::{CommandRunner, RunLimits, ScriptedRunner, SystemRunner, SystemTools};

impl ToolInvoker for ScriptedRunner {
    async fn invoke(
        &self,
        program: &Path,
        args: &[String],
        detached: bool,
    ) -> Result<ToolOutput, ToolsError> {
        run_tool(self, program, args, detached).await
    }
}

impl ToolInvoker for SystemTools {
    async fn invoke(
        &self,
        program: &Path,
        args: &[String],
        detached: bool,
    ) -> Result<ToolOutput, ToolsError> {
        SystemRunner.invoke(program, args, detached).await
    }
}

impl ToolInvoker for SystemRunner {
    async fn invoke(
        &self,
        program: &Path,
        args: &[String],
        detached: bool,
    ) -> Result<ToolOutput, ToolsError> {
        run_tool(self, program, args, detached).await
    }
}

async fn run_tool<R: CommandRunner>(
    runner: &R,
    program: &Path,
    args: &[String],
    detached: bool,
) -> Result<ToolOutput, ToolsError> {
    let mut command = catalogue_probe(program, args)?;
    if detached {
        command = command.detached();
    }
    let exe = measure_platform_tool(program).map_err(|err| ToolsError::Version {
        detail: err.to_string(),
    })?;
    let result = runner
        .run(
            &exe,
            &command,
            RunLimits {
                timeout: Duration::from_secs(15),
                watchdog: None,
                finalising: None,
            },
        )
        .await
        .map_err(|err| ToolsError::Version {
            detail: err.to_string(),
        })?;
    Ok(ToolOutput {
        exit_code: result.exit_code,
        stdout: result.stdout_text(),
        ok: result.success_exit(),
    })
}

/// Version and adb-server probes are catalogue commands. Anything else is refused.
fn catalogue_probe(program: &Path, args: &[String]) -> Result<CatalogueCommand, ToolsError> {
    let name = program
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let adb = matches!(name.as_str(), "adb" | "adb.exe");
    let fastboot = matches!(name.as_str(), "fastboot" | "fastboot.exe");
    let read = if adb && args == ["version".to_string()] {
        ReadCmd::AdbHost(AdbHostRead::Version)
    } else if adb && args == ["kill-server".to_string()] {
        ReadCmd::AdbHost(AdbHostRead::KillServer)
    } else if adb && args == ["start-server".to_string()] {
        ReadCmd::AdbHost(AdbHostRead::StartServer)
    } else if fastboot && args == ["--version".to_string()] {
        ReadCmd::Fastboot(FastbootRead::Version)
    } else {
        return Err(ToolsError::Version {
            detail: "only a catalogue platform-tools probe may run".into(),
        });
    };
    let rendered = read_argv(&read).map_err(|err| ToolsError::Version {
        detail: err.to_string(),
    })?;
    Ok(CatalogueCommand::from_rendered(rendered))
}
