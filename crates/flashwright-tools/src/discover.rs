// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use flashwright_proc::{CommandRunner, Invocation, ProcessGroup};

use crate::hashutil::sha256_file;
use crate::policy::{classify, HostKind, PlatformToolsPolicy, ToolFiles, ToolsVerdict};
use crate::version::{parse_adb_version_output, SdkVersion};
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

#[derive(Clone, Debug)]
pub struct ToolsReport {
    pub directory: PathBuf,
    pub version: SdkVersion,
    pub verdict: ToolsVerdict,
    pub adb: PathBuf,
    pub fastboot: PathBuf,
    pub files: ToolFiles,
}

pub async fn evaluate_installation<R: CommandRunner>(
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
    let verdict = classify(&version, &files, policy, host);
    Ok(ToolsReport {
        directory: dir.to_path_buf(),
        version,
        verdict,
        adb,
        fastboot,
        files,
    })
}

pub async fn adb_version<R: CommandRunner>(
    runner: &R,
    adb: &Path,
) -> Result<SdkVersion, ToolsError> {
    let result = runner
        .run(Invocation {
            program: adb.to_path_buf(),
            args: vec!["version".into()],
            timeout: Duration::from_secs(10),
            watchdog: None,
            group: ProcessGroup::TiedToParent,
        })
        .await?;
    if !result.success_exit() {
        return Err(ToolsError::Version {
            detail: format!("adb version exited {:?}", result.exit_code),
        });
    }
    parse_adb_version_output(&result.stdout_text())
}
