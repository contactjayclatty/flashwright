// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Read the Magisk app archive the user already has.
//!
//! Busybox and magiskboot stay inside that archive. This crate does not
//! ship either file.

use std::io::{Cursor, Read};

use flashwright_core::cmd::WorkFile;
use zip::ZipArchive;

use crate::MagiskError;

/// ABI directory inside the user's Magisk app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceAbi {
    Arm64V8a,
    ArmeabiV7a,
    X86_64,
    X86,
}

impl DeviceAbi {
    fn lib_dir(self) -> &'static str {
        match self {
            Self::Arm64V8a => "lib/arm64-v8a",
            Self::ArmeabiV7a => "lib/armeabi-v7a",
            Self::X86_64 => "lib/x86_64",
            Self::X86 => "lib/x86",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedComponent {
    pub work: WorkFile,
    pub archive_path: String,
    pub bytes: Vec<u8>,
}

/// Pull the scripts and native libraries the app-method patch pushes.
pub fn extract_apk_components(
    apk: &[u8],
    abi: DeviceAbi,
) -> Result<Vec<ExtractedComponent>, MagiskError> {
    let mut archive = ZipArchive::new(Cursor::new(apk)).map_err(|err| {
        MagiskError::Message(format!("the Magisk app archive could not be read: {err}"))
    })?;
    let mut out = Vec::with_capacity(9);
    for (archive_path, work) in component_entries(abi) {
        let mut file = archive.by_name(archive_path).map_err(|_| {
            MagiskError::Message(format!("the Magisk app is missing {archive_path}"))
        })?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|err| {
            MagiskError::Message(format!("the Magisk app entry {archive_path} failed: {err}"))
        })?;
        if bytes.is_empty() {
            return Err(MagiskError::Message(format!(
                "the Magisk app entry {archive_path} is empty"
            )));
        }
        out.push(ExtractedComponent {
            work,
            archive_path: archive_path.to_string(),
            bytes,
        });
    }
    Ok(out)
}

pub fn component_entries(abi: DeviceAbi) -> Vec<(&'static str, WorkFile)> {
    let lib = abi.lib_dir();
    vec![
        ("assets/boot_patch.sh", WorkFile::BootPatch),
        ("assets/util_functions.sh", WorkFile::UtilFunctions),
        ("assets/app_functions.sh", WorkFile::AppFunctions),
        ("assets/stub.apk", WorkFile::StubApk),
        (lib_file(lib, "libbusybox.so"), WorkFile::Busybox),
        (lib_file(lib, "libmagiskboot.so"), WorkFile::Magiskboot),
        (lib_file(lib, "libmagiskinit.so"), WorkFile::Magiskinit),
        (lib_file(lib, "libmagisk.so"), WorkFile::Magisk),
        (lib_file(lib, "libinit-ld.so"), WorkFile::InitLd),
    ]
}

fn lib_file(dir: &'static str, name: &'static str) -> &'static str {
    match (dir, name) {
        ("lib/arm64-v8a", "libbusybox.so") => "lib/arm64-v8a/libbusybox.so",
        ("lib/arm64-v8a", "libmagiskboot.so") => "lib/arm64-v8a/libmagiskboot.so",
        ("lib/arm64-v8a", "libmagiskinit.so") => "lib/arm64-v8a/libmagiskinit.so",
        ("lib/arm64-v8a", "libmagisk.so") => "lib/arm64-v8a/libmagisk.so",
        ("lib/arm64-v8a", "libinit-ld.so") => "lib/arm64-v8a/libinit-ld.so",
        ("lib/armeabi-v7a", "libbusybox.so") => "lib/armeabi-v7a/libbusybox.so",
        ("lib/armeabi-v7a", "libmagiskboot.so") => "lib/armeabi-v7a/libmagiskboot.so",
        ("lib/armeabi-v7a", "libmagiskinit.so") => "lib/armeabi-v7a/libmagiskinit.so",
        ("lib/armeabi-v7a", "libmagisk.so") => "lib/armeabi-v7a/libmagisk.so",
        ("lib/armeabi-v7a", "libinit-ld.so") => "lib/armeabi-v7a/libinit-ld.so",
        ("lib/x86_64", "libbusybox.so") => "lib/x86_64/libbusybox.so",
        ("lib/x86_64", "libmagiskboot.so") => "lib/x86_64/libmagiskboot.so",
        ("lib/x86_64", "libmagiskinit.so") => "lib/x86_64/libmagiskinit.so",
        ("lib/x86_64", "libmagisk.so") => "lib/x86_64/libmagisk.so",
        ("lib/x86_64", "libinit-ld.so") => "lib/x86_64/libinit-ld.so",
        ("lib/x86", "libbusybox.so") => "lib/x86/libbusybox.so",
        ("lib/x86", "libmagiskboot.so") => "lib/x86/libmagiskboot.so",
        ("lib/x86", "libmagiskinit.so") => "lib/x86/libmagiskinit.so",
        ("lib/x86", "libmagisk.so") => "lib/x86/libmagisk.so",
        ("lib/x86", "libinit-ld.so") => "lib/x86/libinit-ld.so",
        _ => "unused",
    }
}
