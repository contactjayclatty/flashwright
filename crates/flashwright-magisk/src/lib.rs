// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Magisk app patch plan.
//!
//! This crate does not spawn processes and does not mint write tokens.
//! [`plan_app_patch`] builds a catalogue plan. The session confirms it.

mod cache;
mod extract;
mod gates;
mod image;
mod pc;
mod plan;

pub use cache::{offer, store, PatchCacheMeta};
pub use extract::{extract_apk_components, DeviceAbi, ExtractedComponent};
pub use gates::{
    check_device_space, check_magisk_version, check_patched_sha1, check_region, data_free_bytes,
    embedded_known_bad, komodo_has_init_boot, patch_partition, KOMODO, LATE_SPL,
    MIN_CODE_FOR_LATE_SPL,
};
pub use image::{ExtractedBootImage, SyntheticInitBoot};
pub use pc::{validate_patched_init_boot, PatchedCheck};
pub use plan::{
    components_from_extract, official_base_apk, plan_app_patch, AppPatchPlan, AppPatchRequest,
    HostComponent,
};

use thiserror::Error;

pub const HIDDEN_APP: &str = "Hidden or renamed Magisk app isn't supported yet";
pub const OFFICIAL_PACKAGE: &str = flashwright_core::cmd::MAGISK_PACKAGE;
const SCRIPT: &str = include_str!("fl_patch.sh");

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MagiskError {
    #[error("{HIDDEN_APP}")]
    HiddenOrRenamed,

    #[error("Komodo patches init_boot, not boot.")]
    KomodoBoot,

    #[error("The LU0 / FIPS region is off-limits.")]
    Lu0Fips,

    #[error("Not enough free space in /data for the patch.")]
    DeviceSpace,

    #[error("The patched image does not match the stock image.")]
    PatchedSha1,

    #[error("This Magisk version is blocked.")]
    KnownBad,

    #[error("This Magisk version is too old for this security patch.")]
    MagiskTooOld,

    #[error("{0}")]
    Message(String),
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
    official_base_apk(dumpsys_package).map(|_| ())
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
        assert!(script.contains("KEEPVERITY=true"));
        assert!(script.contains("KEEPFORCEENCRYPT=true"));
        assert!(script.contains("RECOVERYMODE=false"));
        assert!(script.contains("./boot_patch.sh"));
        assert!(script.contains("chmod 755"));
        assert!(!script.contains("[ -f\""));
    }
}
