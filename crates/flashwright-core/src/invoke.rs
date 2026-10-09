// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Adapt the process runner to the platform-tools locator.

use std::path::Path;
use std::time::Duration;

use flashwright_tools::{ToolInvoker, ToolOutput, ToolsError};

use crate::proc::{CommandRunner, Invocation, ScriptedRunner, SystemRunner};

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
    let mut invocation = Invocation::tied(program, args.to_vec(), Duration::from_secs(15));
    if detached {
        invocation = invocation.detached();
    }
    let result = runner
        .run(invocation)
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
