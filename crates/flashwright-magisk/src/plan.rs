// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! App-method PreparePatch plan.
//!
//! Confirming it is what pushes files and runs the script. Building it does not.

use flashwright_core::cmd::{
    AdbHostRead, AdbHostWrite, AdbShellWrite, AssetRef, DeviceSerial, HostRef, ImageRef,
    PullRemote, ReadCmd, ValidatedDevicePath, WorkFile, WriteCmd,
};
use flashwright_core::wizard::{PlanDraft, PlanStep};
use flashwright_plan::{PREPARE_PATCH_BUTTON, PREPARE_PATCH_TITLE};

use crate::extract::ExtractedComponent;
use crate::gates::{check_device_space, check_magisk_version, check_region, patch_partition};
use crate::image::ExtractedBootImage;
use crate::MagiskError;

pub const FINALLY_STEPS: usize = 1;

#[derive(Clone, Debug)]
pub struct HostComponent {
    pub work: WorkFile,
    pub host_path: String,
}

pub struct AppPatchRequest<'a> {
    pub serial: &'a str,
    pub codename: &'a str,
    pub region: &'a str,
    pub dumpsys_package: &'a str,
    pub diskstats: &'a str,
    pub stock: &'a dyn ExtractedBootImage,
    pub stock_host_path: String,
    pub script_host_path: String,
    pub components: &'a [HostComponent],
    pub magisk_code: u32,
    pub security_patch: &'a str,
    pub known_bad: &'a [u32],
    pub expires_unix_ms: i64,
}

#[derive(Debug)]
pub struct AppPatchPlan {
    pub draft: PlanDraft,
    pub finally_steps: usize,
    pub title: &'static str,
    pub button: &'static str,
}

/// Catalogue plan for "Patch on your phone?".
///
/// A hidden app, a komodo boot image, a blocked Magisk, or low `/data` space
/// returns before any step exists.
pub fn plan_app_patch(request: &AppPatchRequest<'_>) -> Result<AppPatchPlan, MagiskError> {
    check_region(request.region)?;
    patch_partition(request.codename, request.stock)?;
    let apk = official_base_apk(request.dumpsys_package)?;
    check_magisk_version(
        request.magisk_code,
        request.security_patch,
        request.known_bad,
    )?;
    check_device_space(request.diskstats, request.stock.size_bytes())?;
    let serial = DeviceSerial::try_from(request.serial)
        .map_err(|err| MagiskError::Message(err.to_string()))?;
    let stock = ImageRef::for_plan(1, &request.stock_host_path, request.stock.size_bytes())
        .map_err(|err| MagiskError::Message(err.to_string()))?;
    let script = AssetRef::for_plan("fl_patch.sh", &request.script_host_path)
        .map_err(|err| MagiskError::Message(err.to_string()))?;
    let components = host_components(request.components)?;

    let mut steps = vec![
        PlanStep::Read(ReadCmd::AdbHost(AdbHostRead::Pull {
            serial: serial.clone(),
            remote: PullRemote::Validated(apk),
            dst_name: "base.apk".into(),
        })),
        PlanStep::Write(WriteCmd::AdbShell(AdbShellWrite::MakeWorkDir {
            serial: serial.clone(),
        })),
        PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Push {
            serial: serial.clone(),
            src: HostRef::Image(stock),
            dst: WorkFile::Stock,
        })),
        PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Push {
            serial: serial.clone(),
            src: HostRef::Asset(script),
            dst: WorkFile::PatchScript,
        })),
    ];
    for (work, asset) in components {
        steps.push(PlanStep::Write(WriteCmd::AdbHost(AdbHostWrite::Push {
            serial: serial.clone(),
            src: HostRef::Asset(asset),
            dst: work,
        })));
    }
    steps.push(PlanStep::Write(WriteCmd::AdbShell(
        AdbShellWrite::RunPatchScript {
            serial: serial.clone(),
        },
    )));
    steps.push(PlanStep::Read(ReadCmd::AdbHost(AdbHostRead::Pull {
        serial: serial.clone(),
        remote: PullRemote::Work(WorkFile::Patched),
        dst_name: "patched.img".into(),
    })));
    steps.push(PlanStep::Write(WriteCmd::AdbShell(
        AdbShellWrite::RemoveWorkDir { serial },
    )));
    Ok(AppPatchPlan {
        draft: PlanDraft {
            serial: request.serial.to_string(),
            dry_run: false,
            expires_unix_ms: request.expires_unix_ms,
            steps,
        },
        finally_steps: FINALLY_STEPS,
        title: PREPARE_PATCH_TITLE,
        button: PREPARE_PATCH_BUTTON,
    })
}

pub fn components_from_extract(
    extracted: &[ExtractedComponent],
    host_paths: &[String],
) -> Result<Vec<HostComponent>, MagiskError> {
    if extracted.len() != host_paths.len() {
        return Err(MagiskError::Message(
            "each extracted Magisk file needs a host path".into(),
        ));
    }
    Ok(extracted
        .iter()
        .zip(host_paths)
        .map(|(item, host_path)| HostComponent {
            work: item.work,
            host_path: host_path.clone(),
        })
        .collect())
}

fn host_components(components: &[HostComponent]) -> Result<Vec<(WorkFile, AssetRef)>, MagiskError> {
    let mut ordered = Vec::with_capacity(REQUIRED.len());
    for work in REQUIRED {
        let Some(found) = components.iter().find(|item| item.work == *work) else {
            return Err(MagiskError::Message(
                "the Magisk app components are incomplete".into(),
            ));
        };
        let asset = AssetRef::for_plan(found.work.device_path(), &found.host_path)
            .map_err(|err| MagiskError::Message(err.to_string()))?;
        ordered.push((*work, asset));
    }
    Ok(ordered)
}

const REQUIRED: &[WorkFile] = &[
    WorkFile::BootPatch,
    WorkFile::UtilFunctions,
    WorkFile::AppFunctions,
    WorkFile::StubApk,
    WorkFile::Busybox,
    WorkFile::Magiskboot,
    WorkFile::Magiskinit,
    WorkFile::Magisk,
    WorkFile::InitLd,
];

/// `codePath` for the official package, with `/base.apk` appended.
pub fn official_base_apk(dumpsys_package: &str) -> Result<ValidatedDevicePath, MagiskError> {
    let value = code_path_value(dumpsys_package).ok_or(MagiskError::HiddenOrRenamed)?;
    ValidatedDevicePath::from_code_path(value).map_err(|_| MagiskError::HiddenOrRenamed)
}

fn code_path_value(dumpsys: &str) -> Option<&str> {
    let mut in_official = false;
    for line in dumpsys.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Package [") || trimmed.starts_with("package:") {
            in_official = trimmed.contains(crate::OFFICIAL_PACKAGE);
        }
        if let Some(rest) = trimmed.strip_prefix("codePath=") {
            let rest = rest.trim();
            if in_official || rest.contains(crate::OFFICIAL_PACKAGE) {
                return Some(rest);
            }
        }
    }
    None
}
