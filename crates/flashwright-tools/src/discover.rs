// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::hashutil::sha256_file;
use crate::policy::{classify, HostKind, PlatformToolsPolicy, ToolFiles, ToolsVerdict};
use crate::version::{parse_adb_version_output, parse_fastboot_version_output, SdkVersion};
use crate::ToolsError;

/// File names inside a platform-tools directory.
#[derive(Clone, Copy, Debug)]
pub struct ToolBinaryNames {
    pub adb: &'static str,
    pub fastboot: &'static str,
    pub dlls: &'static [&'static str],
}

impl ToolBinaryNames {
    pub fn for_host() -> Self {
        if cfg!(windows) {
            Self::windows()
        } else {
            Self::unix()
        }
    }

    pub fn windows() -> Self {
        Self {
            adb: "adb.exe",
            fastboot: "fastboot.exe",
            dlls: &["AdbWinApi.dll", "AdbWinUsbApi.dll"],
        }
    }

    pub fn unix() -> Self {
        Self {
            adb: "adb",
            fastboot: "fastboot",
            dlls: &[],
        }
    }
}

/// Where to look for an existing platform-tools install.
#[derive(Clone, Debug, Default)]
pub struct DiscoverRequest {
    pub local_app_data: PathBuf,
    pub android_home: Option<PathBuf>,
    pub android_sdk_root: Option<PathBuf>,
    pub user_picked: Option<PathBuf>,
}

pub fn candidate_directories(request: &DiscoverRequest) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(picked) = &request.user_picked {
        out.push(picked.clone());
    }
    let managed = request
        .local_app_data
        .join("Flashwright")
        .join("platform-tools");
    if let Ok(entries) = std::fs::read_dir(&managed) {
        let mut kids: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        kids.sort();
        out.extend(kids);
    }
    out.push(
        request
            .local_app_data
            .join("Android")
            .join("Sdk")
            .join("platform-tools"),
    );
    if let Some(home) = &request.android_home {
        out.push(home.join("platform-tools"));
    }
    if let Some(root) = &request.android_sdk_root {
        out.push(root.join("platform-tools"));
    }
    out
}

pub fn directories_with_tools(request: &DiscoverRequest, names: &ToolBinaryNames) -> Vec<PathBuf> {
    candidate_directories(request)
        .into_iter()
        .filter(|dir| dir.join(names.adb).is_file() && dir.join(names.fastboot).is_file())
        .collect()
}

/// Output of one tool invocation used while locating platform-tools.
pub struct ToolOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub ok: bool,
}

/// Runs a verified argv. The process runner lives in the session crate.
pub trait ToolInvoker: Send + Sync {
    fn invoke<'a>(
        &'a self,
        program: &'a Path,
        args: &'a [String],
        detached: bool,
    ) -> impl std::future::Future<Output = Result<ToolOutput, ToolsError>> + Send + 'a;
}

#[derive(Clone, Debug)]
pub struct ToolsReport {
    pub directory: PathBuf,
    pub version: SdkVersion,
    pub verdict: ToolsVerdict,
    pub adb: PathBuf,
    pub fastboot: PathBuf,
    pub files: ToolFiles,
    pub parser_profile: String,
}

pub async fn evaluate_installation<R: ToolInvoker>(
    dir: &Path,
    names: &ToolBinaryNames,
    policy: &PlatformToolsPolicy,
    runner: &R,
    host: HostKind,
) -> Result<ToolsReport, ToolsError> {
    let adb = dir.join(names.adb);
    let fastboot = dir.join(names.fastboot);
    if !adb.is_file() || !fastboot.is_file() {
        return Err(ToolsError::NotFound {
            directory: dir.display().to_string(),
        });
    }
    let mut dll_sha256s = BTreeMap::new();
    if host == HostKind::Windows {
        for dll in names.dlls {
            let path = dir.join(dll);
            if path.is_file() {
                dll_sha256s.insert((*dll).to_string(), sha256_file(&path)?);
            }
        }
    }
    let files = ToolFiles {
        adb_sha256: sha256_file(&adb)?,
        fastboot_sha256: sha256_file(&fastboot)?,
        dll_sha256s,
    };
    let version = adb_version(runner, &adb).await?;
    let fastboot_version = fastboot_version(runner, &fastboot).await?;
    let adb_verdict = classify(&version, &files, policy, host);
    let fastboot_verdict = classify(&fastboot_version, &files, policy, host);
    let verdict = worse(adb_verdict, fastboot_verdict);
    let parser_profile = policy
        .allow
        .iter()
        .chain(policy.candidates.iter())
        .find(|entry| entry.version.triple() == verdict.version().triple())
        .map(|entry| entry.parser_profile.clone())
        .unwrap_or_default();
    Ok(ToolsReport {
        directory: dir.to_path_buf(),
        version: verdict.version().clone(),
        verdict,
        adb,
        fastboot,
        files,
        parser_profile,
    })
}

pub async fn adb_version<R: ToolInvoker>(runner: &R, adb: &Path) -> Result<SdkVersion, ToolsError> {
    let output = runner.invoke(adb, &["version".into()], false).await?;
    if !output.ok {
        return Err(ToolsError::Version {
            detail: format!("adb version exited {:?}", output.exit_code),
        });
    }
    parse_adb_version_output(&output.stdout)
}

pub async fn fastboot_version<R: ToolInvoker>(
    runner: &R,
    fastboot: &Path,
) -> Result<SdkVersion, ToolsError> {
    let output = runner
        .invoke(fastboot, &["--version".into()], false)
        .await?;
    if !output.ok {
        return Err(ToolsError::Version {
            detail: format!("fastboot version exited {:?}", output.exit_code),
        });
    }
    parse_fastboot_version_output(&output.stdout)
}

fn worse(left: ToolsVerdict, right: ToolsVerdict) -> ToolsVerdict {
    fn rank(verdict: &ToolsVerdict) -> u8 {
        match verdict {
            ToolsVerdict::Blocked { .. } => 0,
            ToolsVerdict::ScanOnly { .. } => 1,
            ToolsVerdict::Allowed { .. } => 2,
        }
    }
    if rank(&right) < rank(&left) {
        right
    } else {
        left
    }
}
