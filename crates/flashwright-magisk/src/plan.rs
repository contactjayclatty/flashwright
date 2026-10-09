// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! App-method PreparePatch plan.
//!
//! Confirming it is what pushes files and runs the script. Building it does not.

use flashwright_core::cmd::{
    AdbHostRead, AdbHostWrite, AdbShellWrite, AssetRef, CleanupCmd, DeviceSerial, HostRef,
    ImageRef, PullName, PullRemote, ReadCmd, ValidatedDevicePath, VerifiedHostFile, WorkFile,
    WriteCmd,
};
use flashwright_core::wizard::{FirmwareClaim, ImageSeal, PlanRequest, PlanStep};
use flashwright_plan::{PREPARE_PATCH_BUTTON, PREPARE_PATCH_TITLE};

use crate::extract::ExtractedComponent;
use crate::gates::{check_device_space, check_magisk_version, check_region, patch_partition};
use crate::image::ExtractedBootImage;
use crate::MagiskError;

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
    /// Kept for the caller. The session clock decides when the plan expires.
    #[allow(dead_code)]
    pub expires_unix_ms: i64,
}

#[derive(Debug)]
pub struct AppPatchPlan {
    pub draft: PlanRequest,
    pub title: &'static str,
    pub button: &'static str,
    pub provenance: &'static str,
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
    let phone_code = phone_version_code(request.dumpsys_package)?;
    if phone_code != request.magisk_code {
        return Err(MagiskError::Message("versionCode rejected".into()));
    }
    check_device_space(request.diskstats, request.stock.size_bytes())?;
    let serial = DeviceSerial::try_from(request.serial)
        .map_err(|err| MagiskError::Message(err.to_string()))?;
    let stock_file = open_host(&request.stock_host_path)?;
    if stock_file.sha256() != request.stock.sha256_hex()
        || !stock_file
            .sha1()
            .eq_ignore_ascii_case(request.stock.sha1_hex())
    {
        return Err(MagiskError::Message(
            "stock file does not match the plan image".into(),
        ));
    }
    let script_file = open_host(&request.script_host_path)?;
    let stock = ImageRef::for_plan(1, &stock_file).map_err(host_err)?;
    let script = AssetRef::for_plan("fl_patch.sh", &script_file).map_err(host_err)?;
    let (components, mut component_seals) = host_components(request.components)?;
    let mut images = vec![
        seal("stock-init-boot", &stock_file),
        seal("fl_patch.sh", &script_file),
    ];
    images.append(&mut component_seals);

    let mut steps = vec![
        PlanStep::Read(ReadCmd::AdbHost(AdbHostRead::Pull {
            serial: serial.clone(),
            remote: PullRemote::Validated(apk),
            dst_name: pull_name("base.apk")?,
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
        dst_name: pull_name("patched.img")?,
    })));
    steps.push(PlanStep::Cleanup(CleanupCmd::RemoveWorkDir { serial }));
    Ok(AppPatchPlan {
        draft: PlanRequest {
            serial: request.serial.to_string(),
            dry_run: false,
            steps,
            firmware: FirmwareClaim {
                images,
                ..FirmwareClaim::default()
            },
        },
        title: PREPARE_PATCH_TITLE,
        button: PREPARE_PATCH_BUTTON,
        provenance: crate::MAGISK_PROVENANCE,
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

type HostAssets = (Vec<(WorkFile, AssetRef)>, Vec<ImageSeal>);

fn host_components(components: &[HostComponent]) -> Result<HostAssets, MagiskError> {
    let mut ordered = Vec::with_capacity(REQUIRED.len());
    let mut seals = Vec::with_capacity(REQUIRED.len());
    for work in REQUIRED {
        let Some(found) = components.iter().find(|item| item.work == *work) else {
            return Err(MagiskError::Message(
                "the Magisk app components are incomplete".into(),
            ));
        };
        let file = open_host(&found.host_path)?;
        let asset = AssetRef::for_plan(found.work.device_path(), &file).map_err(host_err)?;
        seals.push(seal(seal_role(*work), &file));
        ordered.push((*work, asset));
    }
    Ok((ordered, seals))
}

fn open_host(path: &str) -> Result<VerifiedHostFile, MagiskError> {
    VerifiedHostFile::open(path).map_err(host_err)
}

fn host_err(err: impl ToString) -> MagiskError {
    MagiskError::Message(err.to_string())
}

fn pull_name(name: &str) -> Result<PullName, MagiskError> {
    PullName::new(name).map_err(host_err)
}

fn seal(role: &str, file: &VerifiedHostFile) -> ImageSeal {
    ImageSeal {
        role: role.to_string(),
        sha1: file.sha1().to_string(),
        sha256: file.sha256().to_string(),
    }
}

fn seal_role(work: WorkFile) -> &'static str {
    match work {
        WorkFile::Stock => "stock.img",
        WorkFile::Patched => "patched.img",
        WorkFile::PatchScript => "fl_patch.sh",
        WorkFile::BootPatch => "boot_patch.sh",
        WorkFile::UtilFunctions => "util_functions.sh",
        WorkFile::AppFunctions => "app_functions.sh",
        WorkFile::StubApk => "stub.apk",
        WorkFile::Busybox => "libbusybox.so",
        WorkFile::Magiskboot => "libmagiskboot.so",
        WorkFile::Magiskinit => "libmagiskinit.so",
        WorkFile::Magisk => "libmagisk.so",
        WorkFile::InitLd => "libinit-ld.so",
    }
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
    match classify_app(dumpsys_package) {
        AppHit::Official { code_path } => ValidatedDevicePath::from_code_path(&code_path)
            .map_err(|_| MagiskError::CodePathRejected),
        AppHit::Hidden => Err(MagiskError::HiddenOrRenamed),
        AppHit::Absent => Err(MagiskError::NotInstalled),
        AppHit::BadCodePath => Err(MagiskError::CodePathRejected),
    }
}

/// `versionCode` from the official package block on the phone.
pub fn phone_version_code(dumpsys_package: &str) -> Result<u32, MagiskError> {
    let mut in_official = false;
    let mut found = None;
    for line in dumpsys_package.lines() {
        let trimmed = line.trim();
        if let Some(name) = package_name_at(trimmed) {
            in_official = name == crate::OFFICIAL_PACKAGE;
            continue;
        }
        if !in_official {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("versionCode=") {
            let token = rest.split_whitespace().next().unwrap_or("");
            found = token.parse().ok();
        }
    }
    found.ok_or_else(|| MagiskError::Message("versionCode rejected".into()))
}

/// One line for the section 17 detection log.
pub fn detection_log(dumpsys_package: &str) -> String {
    let kind = match classify_app(dumpsys_package) {
        AppHit::Official { .. } => "official",
        AppHit::Hidden => "hidden",
        AppHit::Absent => "absent",
        AppHit::BadCodePath => "codePath rejected",
    };
    format!("section 17 detect: {kind}")
}

enum AppHit {
    Official { code_path: String },
    Hidden,
    Absent,
    BadCodePath,
}

fn classify_app(dumpsys: &str) -> AppHit {
    let mut saw_hidden = false;
    let mut in_official = false;
    let mut saw_official = false;
    let mut code_path = None;
    for line in dumpsys.lines() {
        let trimmed = line.trim();
        if let Some(name) = package_name_at(trimmed) {
            in_official = name == crate::OFFICIAL_PACKAGE;
            if in_official {
                saw_official = true;
            } else if name.to_ascii_lowercase().contains("magisk") {
                saw_hidden = true;
            }
            continue;
        }
        if in_official {
            if let Some(rest) = trimmed.strip_prefix("codePath=") {
                code_path = Some(rest.trim());
            }
        }
    }
    if !saw_official {
        return if saw_hidden {
            AppHit::Hidden
        } else {
            AppHit::Absent
        };
    }
    let Some(path) = code_path else {
        return AppHit::Hidden;
    };
    if ValidatedDevicePath::from_code_path(path).is_err() {
        return AppHit::BadCodePath;
    }
    AppHit::Official {
        code_path: path.to_string(),
    }
}

fn package_name_at(trimmed: &str) -> Option<&str> {
    if let Some(rest) = trimmed.strip_prefix("Package [") {
        let name = rest.strip_suffix(']')?.trim();
        if name.is_empty() {
            return None;
        }
        return Some(name);
    }
    let rest = trimmed.strip_prefix("package:")?;
    let name = rest.split_whitespace().next()?;
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}
