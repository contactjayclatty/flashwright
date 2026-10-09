// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Step list for preparing a Magisk patch. This crate does not run commands.

use thiserror::Error;

pub const HIDDEN_APP: &str = "Hidden or renamed Magisk app isn't supported yet";
pub const OFFICIAL_PACKAGE: &str = flashwright_core::cmd::MAGISK_PACKAGE;
const SCRIPT: &str = include_str!("fl_patch.sh");

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MagiskError {
    #[error("{HIDDEN_APP}")]
    HiddenOrRenamed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchStep {
    MakeWorkDir,
    PushStock,
    PushScript,
    RunPatchScript,
    PullPatched,
    RemoveWorkDir,
}

pub fn patch_script() -> &'static str {
    SCRIPT
}

/// Official package only. A missing `codePath` for that package stops the plan.
pub fn require_official_app(dumpsys_package: &str) -> Result<(), MagiskError> {
    let mentions = dumpsys_package.contains(OFFICIAL_PACKAGE);
    let code_path = dumpsys_package.lines().any(|line| {
        let line = line.trim();
        line.starts_with("codePath=") && line.contains(OFFICIAL_PACKAGE)
    });
    if mentions && code_path {
        Ok(())
    } else {
        Err(MagiskError::HiddenOrRenamed)
    }
}

pub fn prepare_steps(dumpsys_package: &str) -> Result<Vec<PatchStep>, MagiskError> {
    require_official_app(dumpsys_package)?;
    Ok(vec![
        PatchStep::MakeWorkDir,
        PatchStep::PushStock,
        PatchStep::PushScript,
        PatchStep::RunPatchScript,
        PatchStep::PullPatched,
        PatchStep::RemoveWorkDir,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_app_writes_nothing() {
        let err = prepare_steps("package:com.example.hidden\n").unwrap_err();
        assert_eq!(err.to_string(), HIDDEN_APP);
        let ok = prepare_steps(
            "Package [com.topjohnwu.magisk]\n    codePath=/data/app/~~abc==/com.topjohnwu.magisk-xyz\n",
        )
        .unwrap();
        assert!(ok.contains(&PatchStep::RunPatchScript));
        assert!(ok.contains(&PatchStep::RemoveWorkDir));
    }

    #[test]
    fn script_uses_the_fixed_paths() {
        let script = patch_script();
        assert!(script.contains("/data/local/tmp/flashwright/out/patched.img"));
        assert!(script.contains("/data/local/tmp/flashwright/stock.img"));
        assert!(script.contains("FL_STOCK_SHA256="));
        assert!(script.contains("FL_OUT="));
        assert!(script.contains("FL_SHA1="));
    }
}
