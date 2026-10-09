// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! T1.1 — allow, block, scan-only, and zip SHA-1.

use std::io::{Cursor, Write};
use std::path::Path;

use flashwright_tools::{
    classify, import_platform_tools_zip, safe_relative, sha1_bytes, AllowEntry, HostKind,
    PlatformToolsPolicy, SdkVersion, ToolFiles, ToolsVerdict, CANDIDATE_37_0_1_ZIP_SHA1,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn version(text: &str) -> SdkVersion {
    SdkVersion::parse(text).unwrap()
}

fn files(adb: &str, fastboot: &str) -> ToolFiles {
    ToolFiles {
        adb_sha256: adb.into(),
        fastboot_sha256: fastboot.into(),
        dll_sha256s: Default::default(),
    }
}

fn fixture_policy() -> PlatformToolsPolicy {
    PlatformToolsPolicy::from_toml(
        r#"
[block]
below = "33.0.3"

[[block.ranges]]
from = "34.0.0"
to = "34.0.4"

[[block.exact]]
version = "36.0.2"
any_build = true

[[allow]]
version = "37.0.1"
zip_sha1 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
device_tested = true
adb_sha256 = "aa"
fastboot_sha256 = "bb"

[[allow]]
version = "34.0.4"
zip_sha1 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
device_tested = true
adb_sha256 = "aa"
fastboot_sha256 = "bb"
"#,
    )
    .unwrap()
}

#[test]
fn t1_1_six_fixture_cases() {
    let policy = fixture_policy();
    let host = HostKind::Other;

    let allowed = classify(&version("37.0.1"), &files("aa", "bb"), &policy, host);
    assert!(matches!(allowed, ToolsVerdict::Allowed { .. }));
    assert!(allowed.allows_writes());

    let blocked_range = classify(&version("34.0.4"), &files("aa", "bb"), &policy, host);
    assert!(matches!(blocked_range, ToolsVerdict::Blocked { .. }));
    assert!(!blocked_range.allows_scan());
    assert!(blocked_range.message().contains("34.0.4"));

    let blocked_old = classify(&version("33.0.2"), &files("aa", "bb"), &policy, host);
    assert!(matches!(blocked_old, ToolsVerdict::Blocked { .. }));
    assert!(blocked_old.message().contains("older than"));

    let blocked_exact = classify(
        &version("36.0.2-14143358"),
        &files("aa", "bb"),
        &policy,
        host,
    );
    assert!(matches!(blocked_exact, ToolsVerdict::Blocked { .. }));
    assert!(blocked_exact.message().contains("all build ids"));

    let untested = classify(&version("35.0.1"), &files("aa", "bb"), &policy, host);
    assert!(matches!(untested, ToolsVerdict::ScanOnly { .. }));
    assert!(untested.allows_scan());
    assert!(!untested.allows_writes());
    assert!(untested.message().contains("Untested platform-tools"));

    let bytes = stored_zip(&[
        ("platform-tools/adb", b"adb-bytes"),
        ("platform-tools/fastboot", b"fb"),
    ]);
    let digest = sha1_bytes(&bytes);
    let zip_policy = PlatformToolsPolicy::from_toml(&format!(
        r#"
[block]
below = "33.0.3"

[[candidate]]
version = "37.0.1"
zip_sha1 = "{digest}"
device_tested = false
adb_sha256 = ""
fastboot_sha256 = ""
"#
    ))
    .unwrap();
    let dir = std::env::temp_dir().join(format!("flashwright-zip-{}", std::process::id()));
    let imported = import_platform_tools_zip(&bytes, &dir, &zip_policy).unwrap();
    assert_eq!(imported.sha1, digest);
    assert!(dir
        .join("37.0.1")
        .join("platform-tools")
        .join("adb")
        .is_file());
    let mut flipped = bytes.clone();
    flipped[10] ^= 0xff;
    let refused = import_platform_tools_zip(&flipped, &dir, &zip_policy).unwrap_err();
    let message = refused.to_string();
    assert!(message.contains("SHA-1"), "{message}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn embedded_policy_does_not_write_enable_the_candidate() {
    let policy = PlatformToolsPolicy::embedded().unwrap();
    let text = include_str!("../../../data/platform_tools.toml");
    assert!(text.contains(CANDIDATE_37_0_1_ZIP_SHA1));
    assert_eq!(
        CANDIDATE_37_0_1_ZIP_SHA1,
        "e03e78b1d80b396f1c3358e31251cb31740e1110"
    );
    let verdict = classify(
        &version("37.0.1"),
        &files("aa", "bb"),
        &policy,
        HostKind::Other,
    );
    assert!(!verdict.allows_writes());
    assert!(verdict.message().contains("has not passed a device test"));
}

#[test]
fn boundary_versions_and_hash_mismatch() {
    let policy = fixture_policy();
    let host = HostKind::Other;
    assert!(
        classify(&version("33.0.3"), &files("aa", "bb"), &policy, host)
            .message()
            .contains("Untested platform-tools")
    );
    assert!(matches!(
        classify(&version("34.0.0"), &files("aa", "bb"), &policy, host),
        ToolsVerdict::Blocked { .. }
    ));
    assert!(matches!(
        classify(&version("34.0.5"), &files("aa", "bb"), &policy, host),
        ToolsVerdict::ScanOnly { .. }
    ));
    assert!(matches!(
        classify(&version("36.0.0"), &files("aa", "bb"), &policy, host),
        ToolsVerdict::ScanOnly { .. }
    ));
    assert!(matches!(
        classify(&version("36.0.1"), &files("aa", "bb"), &policy, host),
        ToolsVerdict::ScanOnly { .. }
    ));
    let mismatch = classify(&version("37.0.1"), &files("nope", "bb"), &policy, host);
    assert!(mismatch.message().contains("file hashes differ"));
    assert!(!mismatch.allows_writes());

    let mut windows_files = files("aa", "bb");
    let windows = classify(
        &version("37.0.1"),
        &windows_files,
        &policy,
        HostKind::Windows,
    );
    assert!(
        !windows.allows_writes(),
        "empty dll map must not unlock writes"
    );
    windows_files
        .dll_sha256s
        .insert("AdbWinApi.dll".into(), "cc".into());
    windows_files
        .dll_sha256s
        .insert("AdbWinUsbApi.dll".into(), "dd".into());
    let with_entry = allow_with_dlls();
    let unlocked = classify(
        &version("37.0.1"),
        &windows_files,
        &with_entry,
        HostKind::Windows,
    );
    assert!(unlocked.allows_writes());
}

fn allow_with_dlls() -> PlatformToolsPolicy {
    let mut policy = fixture_policy();
    let entry = policy
        .allow
        .iter_mut()
        .find(|entry| entry.version.triple() == (37, 0, 1))
        .unwrap();
    entry
        .dll_sha256s
        .insert("AdbWinApi.dll".into(), "cc".into());
    entry
        .dll_sha256s
        .insert("AdbWinUsbApi.dll".into(), "dd".into());
    let _ = entry as &mut AllowEntry;
    policy
}

#[test]
fn zip_paths_reject_traversal() {
    let err = safe_relative(Path::new("../etc/passwd")).unwrap_err();
    assert!(err.to_string().contains("safe relative"));
    assert!(safe_relative(Path::new("platform-tools/adb")).is_ok());
}

fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        for (name, bytes) in files {
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
    cursor.into_inner()
}
