// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! M3 patch tests. adb is a scripted stand-in. No phone is contacted.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use flashwright_core::cmd::WorkFile;
use flashwright_core::device::{Partition, PlatformToolsTransport, TransportConfig};
use flashwright_core::proc::{Invocation, ScriptedResponse, ScriptedRunner};
use flashwright_core::wizard::{FixedClock, WizardSession};
use flashwright_magisk::{
    accept_patch_pull, check_device_space, check_magisk_version, check_patched_sha1, check_region,
    detection_log, embedded_known_bad, extract_apk_components, komodo_has_init_boot, offer,
    patch_partition, plan_app_patch, store, validate_patched_init_boot, AppPatchRequest, DeviceAbi,
    ExtractedBootImage, HostComponent, MagiskError, PatchCacheMeta, PatchPull, PatchedCheck,
    SyntheticInitBoot, HIDDEN_APP, MAGISK_PROVENANCE,
};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const SERIAL: &str = "komodo1";
const DUMPS: &str = "\
Package [com.topjohnwu.magisk]
    codePath=/data/app/~~abc==/com.topjohnwu.magisk-xyz
    versionCode=30700
    versionName=30.7
";
const PLENTY: &str = "Data-Free: 8000000K\n";
const PATCHED_SHA1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn hidden_app_does_not_build_a_plan() {
    let stock = SyntheticInitBoot::komodo();
    let components = host_components();
    let err = plan_app_patch(&request(
        &stock,
        &components,
        "package:com.example.hidden\n",
        PLENTY,
        30_700,
        "2026-01-01",
        &[],
    ))
    .unwrap_err();
    assert_eq!(err.to_string(), "Magisk is not installed.");
    let lookalike = plan_app_patch(&request(
        &stock,
        &components,
        "Package [com.topjohnwu.magisk.hidden]\n    codePath=/data/app/~~abc==/com.topjohnwu.magisk-xyz\n",
        PLENTY,
        30_700,
        "2026-01-01",
        &[],
    ))
    .unwrap_err();
    assert_eq!(lookalike.to_string(), HIDDEN_APP);
    assert!(detection_log("package:com.example.hidden\n").starts_with("section 17 detect:"));
}

#[tokio::test]
async fn unconfirmed_plan_spawns_nothing() {
    let runner = Arc::new(ScriptedRunner::new());
    script(&runner, true);
    let open = session;
    let mut session = open(Arc::clone(&runner));
    let stock = SyntheticInitBoot::komodo();
    let mut components = host_components();
    let plan = plan_app_patch(&staged_request(
        &stock,
        &mut components,
        DUMPS,
        PLENTY,
        30_700,
        "2026-01-01",
        &[],
    ))
    .unwrap();
    assert_eq!(plan.title, "Patch on your phone?");
    assert_eq!(plan.button, "Patch now");
    assert_eq!(plan.provenance, MAGISK_PROVENANCE);
    assert_eq!(plan.draft.images[0].sha1, stock.sha1_hex());
    assert_eq!(plan.draft.images[0].sha256, stock.sha256_hex());
    assert!(plan
        .draft
        .images
        .iter()
        .any(|seal| seal.role == "libbusybox.so" && seal.sha256.len() == 64));
    let mut dry = plan.draft.clone();
    dry.dry_run = true;
    let preview = session.build_plan(plan.draft).await.unwrap();
    let blob = preview
        .steps
        .iter()
        .flat_map(|step| step.argv.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(blob.contains("fl_patch.sh"));
    assert!(blob.contains("base.apk"));
    assert!(!blob.contains("'su'"));
    assert!(!blob.contains("'pm'"));
    assert!(!blob.contains(" pm "));
    let err = session
        .confirm_and_run_finally("flp1-not-issued", 0)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not issued"));
    assert!(runner.calls().is_empty());
    let mut dry_session = open(Arc::clone(&runner));
    let dry_preview = dry_session.build_plan(dry).await.unwrap();
    let listed = dry_session.dry_run(&dry_preview.plan_hash).unwrap();
    assert!(listed.iter().any(|line| line.starts_with("WOULD BLOCK:")));
    assert!(listed.iter().all(|line| !line.starts_with("WOULD RUN")));
    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn confirmed_app_patch_runs_three_times() {
    let runner = Arc::new(ScriptedRunner::new());
    script(&runner, true);
    let stock = SyntheticInitBoot::komodo();
    for _ in 0..3 {
        let mut session = session(Arc::clone(&runner));
        let mut components = host_components();
        let plan = plan_app_patch(&staged_request(
            &stock,
            &mut components,
            DUMPS,
            PLENTY,
            30_700,
            "2026-01-01",
            &[],
        ))
        .unwrap();
        let preview = session.build_plan(plan.draft).await.unwrap();
        let err = session
            .confirm_and_run_finally(&preview.plan_hash, 0)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Blocked"));
    }
    let calls = runner.calls();
    assert_eq!(count_shell(&calls, "fl_patch.sh"), 0);
    assert_eq!(count_shell(&calls, "'rm'"), 0);
    let blob = calls
        .iter()
        .flat_map(|call| call.args.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!blob.contains("'su'"));
    assert!(!blob.contains("'pm'"));
    assert!(!blob.contains(" pm "));
}

#[tokio::test]
async fn cleanup_runs_after_a_script_failure() {
    let runner = Arc::new(ScriptedRunner::new());
    script(&runner, false);
    let mut session = session(Arc::clone(&runner));
    let stock = SyntheticInitBoot::komodo();
    let mut components = host_components();
    let plan = plan_app_patch(&staged_request(
        &stock,
        &mut components,
        DUMPS,
        PLENTY,
        30_700,
        "2026-01-01",
        &[],
    ))
    .unwrap();
    let preview = session.build_plan(plan.draft).await.unwrap();
    let err = session
        .confirm_and_run_finally(&preview.plan_hash, 0)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Blocked"));
    assert_eq!(count_shell(&runner.calls(), "fl_patch.sh"), 0);
    let listed = session
        .read(flashwright_core::cmd::ReadCmd::AdbShell(
            flashwright_core::cmd::AdbShellRead::LsWorkDir {
                serial: flashwright_core::cmd::DeviceSerial::try_from(SERIAL).unwrap(),
            },
        ))
        .await
        .unwrap();
    assert!(listed.stdout_text().contains("No such file"));
}

#[test]
fn komodo_patches_init_boot_and_blocks_lu0() {
    assert!(komodo_has_init_boot().unwrap());
    let init_boot = SyntheticInitBoot::komodo();
    assert_eq!(
        patch_partition("komodo", &init_boot).unwrap(),
        Partition::InitBoot
    );
    let boot = SyntheticInitBoot::boot_fixture();
    assert_eq!(
        patch_partition("komodo", &boot).unwrap_err().to_string(),
        "Komodo patches init_boot, not boot."
    );
    assert_eq!(
        check_region("LU0").unwrap_err().to_string(),
        "The LU0 / FIPS region is off-limits."
    );
    assert!(check_region("FIPS").is_err());
    assert!(check_region("LU0 / FIPS").is_err());
    assert!(check_region("US").is_ok());
    assert!(check_region("NOTLU0").is_ok());
    assert!(check_region("FIPSCO").is_ok());
    assert!(check_region("my LU0 phone").is_ok());
    assert_eq!(patch_partition("oriole", &boot).unwrap(), Partition::Boot);
    assert!(patch_partition("oriole", &init_boot).is_err());
    assert_eq!(
        patch_partition("shiba", &init_boot).unwrap(),
        Partition::InitBoot
    );
    assert!(patch_partition("KOMODO", &init_boot).is_ok());
    assert!(patch_partition("not-a-phone", &init_boot).is_err());
    let components = host_components();
    let mismatch = plan_app_patch(&request(
        &init_boot,
        &components,
        DUMPS,
        PLENTY,
        30_701,
        "2026-01-01",
        &[],
    ))
    .unwrap_err();
    assert!(mismatch.to_string().contains("versionCode"));
    let err = plan_app_patch(&request(
        &boot,
        &components,
        DUMPS,
        PLENTY,
        30_700,
        "2026-01-01",
        &[],
    ))
    .unwrap_err();
    assert_eq!(err, MagiskError::KomodoBoot);
}

#[test]
fn magisk_gates_and_device_space() {
    assert!(embedded_known_bad().unwrap().contains(&25207));
    assert_eq!(
        check_magisk_version(27_000, "2025-12-01", &[]).unwrap_err(),
        MagiskError::MagiskTooOld
    );
    assert_eq!(
        check_magisk_version(30_600, "2025-12-01", &[30_600]).unwrap_err(),
        MagiskError::KnownBad
    );
    assert!(check_magisk_version(30_599, "2025-11-30", &[]).is_ok());
    assert!(check_magisk_version(30_600, "2025-12-01", &[]).is_ok());
    let stock = SyntheticInitBoot::komodo();
    assert_eq!(
        check_device_space("Data-Free: 1024K\n", stock.size_bytes()).unwrap_err(),
        MagiskError::DeviceSpace
    );
    assert!(check_device_space(PLENTY, stock.size_bytes()).is_ok());
    let components = host_components();
    let err = plan_app_patch(&request(
        &stock,
        &components,
        DUMPS,
        "Data-Free: 1024K\n",
        30_700,
        "2026-01-01",
        &[],
    ))
    .unwrap_err();
    assert_eq!(err, MagiskError::DeviceSpace);
}

#[test]
fn strict_sha1_and_cache_reuse() {
    let stock = SyntheticInitBoot::komodo();
    let patched = hex(Sha256::digest(b"patched-init-boot-stand-in"));
    assert!(check_patched_sha1(
        stock.sha1_hex(),
        stock.sha256_hex(),
        &patched,
        stock.sha1_hex()
    )
    .is_ok());
    assert!(check_patched_sha1(
        stock.sha1_hex(),
        stock.sha256_hex(),
        &patched,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .is_err());
    assert!(check_patched_sha1(
        stock.sha1_hex(),
        stock.sha256_hex(),
        stock.sha256_hex(),
        stock.sha1_hex(),
    )
    .is_err());

    let root = std::env::temp_dir().join("flashwright-m3-cache");
    let _ = std::fs::remove_dir_all(&root);
    let meta = PatchCacheMeta {
        stock_sha1: stock.sha1_hex().to_string(),
        stock_sha256: stock.sha256_hex().to_string(),
        magisk_version: "30.7".into(),
        magisk_code: 30_700,
        method: "app".into(),
        patched_sha1: PATCHED_SHA1.into(),
        patched_sha256: patched.clone(),
        config_sha1: stock.sha1_hex().to_string(),
        created_utc: "2026-10-09T00:00:00Z".into(),
    };
    let stored = store(&root, &meta, b"patched-init-boot-stand-in").unwrap();
    let again = offer(&root, stock.sha1_hex(), stock.sha256_hex(), 30_700)
        .unwrap()
        .unwrap();
    assert_eq!(stored, again);
    std::fs::write(&stored, b"tampered-image").unwrap();
    assert!(offer(&root, stock.sha1_hex(), stock.sha256_hex(), 30_700)
        .unwrap()
        .is_none());
    std::fs::write(&stored, b"patched-init-boot-stand-in").unwrap();
    assert!(store(&root, &meta, b"not-the-patched-bytes").is_err());
    let mut broken = meta;
    broken.config_sha1 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
    std::fs::write(
        root.join(stock.sha1_hex()).join("30700").join("meta.json"),
        serde_json::to_vec(&broken).unwrap(),
    )
    .unwrap();
    assert!(offer(&root, stock.sha1_hex(), stock.sha256_hex(), 30_700).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_pc_parser_checks_the_ramdisk() {
    let stock = SyntheticInitBoot::komodo();
    let image = patched_cpio_init_boot(stock.sha1_hex());
    let report = validate_patched_init_boot(PatchedCheck {
        patched: &image,
        stock_sha1: stock.sha1_hex(),
        stock_sha256: stock.sha256_hex(),
        plan_hash: "flp1-parser",
    })
    .unwrap();
    assert_eq!(report.ramdisk_format.as_str(), "cpio");
    assert_eq!(report.config_sha1, stock.sha1_hex());
    assert_eq!(report.init_sha256.len(), 64);
    assert_ne!(report.patched_sha256, stock.sha256_hex());
    let script = format!(
        "FL_COMPONENTS_OK\nFL_STOCK_SHA256={}\nFL_SHA1={}\nFL_OUT=/data/local/tmp/flashwright/out/patched.img\n",
        stock.sha256_hex(),
        stock.sha1_hex()
    );
    let accepted = accept_patch_pull(PatchPull {
        script_text: &script,
        pull_text: "1 file pulled\n",
        patched: &image,
        apk: b"base-apk",
        stock_sha1: stock.sha1_hex(),
        stock_sha256: stock.sha256_hex(),
        plan_hash: "flp1-parser",
    })
    .unwrap();
    assert_eq!(accepted.apk_sha256.len(), 64);
    assert!(accept_patch_pull(PatchPull {
        script_text:
            "FL_STOCK_SHA256=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\n",
        pull_text: "1 file pulled\n",
        patched: &image,
        apk: b"base-apk",
        stock_sha1: stock.sha1_hex(),
        stock_sha256: stock.sha256_hex(),
        plan_hash: "flp1-parser",
    })
    .is_err());
    assert!(accept_patch_pull(PatchPull {
        script_text: &script,
        pull_text: "",
        patched: &image,
        apk: b"base-apk",
        stock_sha1: stock.sha1_hex(),
        stock_sha256: stock.sha256_hex(),
        plan_hash: "flp1-parser",
    })
    .is_err());
    let other = validate_patched_init_boot(PatchedCheck {
        patched: &image,
        stock_sha1: stock.sha1_hex(),
        stock_sha256: stock.sha256_hex(),
        plan_hash: "flp1-other",
    })
    .unwrap();
    assert_ne!(report.plan_binding, other.plan_binding);

    let wrong = patched_cpio_init_boot("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    assert_eq!(
        validate_patched_init_boot(PatchedCheck {
            patched: &wrong,
            stock_sha1: stock.sha1_hex(),
            stock_sha256: stock.sha256_hex(),
            plan_hash: "flp1-parser",
        })
        .unwrap_err(),
        MagiskError::PatchedSha1
    );
    let xz = patched_boot_with_ramdisk(b"\xfd7zXZ\x00");
    let err = validate_patched_init_boot(PatchedCheck {
        patched: &xz,
        stock_sha1: stock.sha1_hex(),
        stock_sha256: stock.sha256_hex(),
        plan_hash: "flp1-parser",
    })
    .unwrap_err();
    assert!(err.to_string().contains("unsupported ramdisk format"));
}

#[test]
fn apk_components_come_from_the_archive() {
    let apk = sample_apk();
    let parts = extract_apk_components(&apk, DeviceAbi::Arm64V8a).unwrap();
    assert_eq!(parts.len(), 9);
    assert!(parts.iter().any(|part| part.work == WorkFile::Busybox));
    assert!(parts
        .iter()
        .any(|part| part.archive_path == "lib/arm64-v8a/libmagiskboot.so"));
    let mut broken = sample_entries();
    broken.retain(|(name, _)| *name != "assets/boot_patch.sh");
    let err = extract_apk_components(&zip_with(&broken), DeviceAbi::Arm64V8a).unwrap_err();
    assert!(err.to_string().contains("boot_patch.sh"));
}

#[test]
fn magiskboot_and_busybox_are_not_bundled() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    walk_names(&root);
}

fn patched_cpio_init_boot(sha1: &str) -> Vec<u8> {
    let config = format!("KEEPVERITY=true\nSHA1={sha1}\n");
    let archive = cpio_newc(&[
        (".backup/.magisk", config.as_bytes()),
        ("init", b"magisk-init"),
    ]);
    patched_boot_with_ramdisk(&archive)
}

fn patched_boot_with_ramdisk(ramdisk: &[u8]) -> Vec<u8> {
    let mut header = vec![0u8; 1584];
    header[..8].copy_from_slice(b"ANDROID!");
    header[12..16].copy_from_slice(&(ramdisk.len() as u32).to_le_bytes());
    header[20..24].copy_from_slice(&1584u32.to_le_bytes());
    header[40..44].copy_from_slice(&4u32.to_le_bytes());
    let mut image = header;
    image.resize(4096, 0);
    image.extend_from_slice(ramdisk);
    let pad = (4096 - (ramdisk.len() % 4096)) % 4096;
    image.resize(image.len() + pad, 0);
    image
}

fn cpio_newc(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, (name, data)) in files.iter().enumerate() {
        let mut name_bytes = name.as_bytes().to_vec();
        name_bytes.push(0);
        let mut header = String::from("070701");
        for field in [
            index as u32 + 1,
            0o100755,
            0,
            0,
            1,
            0,
            data.len() as u32,
            0,
            0,
            0,
            0,
            name_bytes.len() as u32,
            0,
        ] {
            header.push_str(&format!("{field:08x}"));
        }
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&name_bytes);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    let name_bytes = b"TRAILER!!!\0".to_vec();
    let mut header = String::from("070701");
    for field in [
        0u32,
        0,
        0,
        0,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        name_bytes.len() as u32,
        0,
    ] {
        header.push_str(&format!("{field:08x}"));
    }
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&name_bytes);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

fn staged_request<'a>(
    stock: &'a SyntheticInitBoot,
    components: &'a mut [HostComponent],
    dumpsys: &'a str,
    diskstats: &'a str,
    magisk_code: u32,
    security_patch: &'a str,
    known_bad: &'a [u32],
) -> AppPatchRequest<'a> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fw-m3-{}-{}", std::process::id(), id));
    std::fs::create_dir_all(&dir).unwrap();
    let stock_path = dir.join("stock.img");
    std::fs::write(&stock_path, stock.bytes()).unwrap();
    let script_path = dir.join("fl_patch.sh");
    std::fs::write(&script_path, b"#!/system/bin/sh\nset -eu\n").unwrap();
    for (index, component) in components.iter_mut().enumerate() {
        let path = dir.join(format!("part-{index}.bin"));
        std::fs::write(&path, [u8::try_from(index).unwrap_or(0), 7, 7, 7]).unwrap();
        component.host_path = path.to_string_lossy().into_owned();
    }
    let mut built = request(
        stock,
        components,
        dumpsys,
        diskstats,
        magisk_code,
        security_patch,
        known_bad,
    );
    built.stock_host_path = stock_path.to_string_lossy().into_owned();
    built.script_host_path = script_path.to_string_lossy().into_owned();
    built
}

fn request<'a>(
    stock: &'a SyntheticInitBoot,
    components: &'a [HostComponent],
    dumpsys: &'a str,
    diskstats: &'a str,
    magisk_code: u32,
    security_patch: &'a str,
    known_bad: &'a [u32],
) -> AppPatchRequest<'a> {
    AppPatchRequest {
        serial: SERIAL,
        codename: "komodo",
        region: "US",
        dumpsys_package: dumpsys,
        diskstats,
        stock,
        stock_host_path: host_path("stock.img"),
        script_host_path: host_path("fl_patch.sh"),
        components,
        magisk_code,
        security_patch,
        known_bad,
        expires_unix_ms: 60_000,
    }
}

fn host_components() -> Vec<HostComponent> {
    [
        (WorkFile::BootPatch, "boot_patch.sh"),
        (WorkFile::UtilFunctions, "util_functions.sh"),
        (WorkFile::AppFunctions, "app_functions.sh"),
        (WorkFile::StubApk, "stub.apk"),
        (WorkFile::Busybox, "libbusybox.so"),
        (WorkFile::Magiskboot, "libmagiskboot.so"),
        (WorkFile::Magiskinit, "libmagiskinit.so"),
        (WorkFile::Magisk, "libmagisk.so"),
        (WorkFile::InitLd, "libinit-ld.so"),
    ]
    .into_iter()
    .map(|(work, name)| HostComponent {
        work,
        host_path: host_path(name),
    })
    .collect()
}

fn host_path(name: &str) -> String {
    if cfg!(windows) {
        format!(r"C:\flashwright-test\{name}")
    } else {
        format!("/var/flashwright/{name}")
    }
}

fn session(runner: Arc<ScriptedRunner>) -> WizardSession<ScriptedRunner> {
    let transport = PlatformToolsTransport::new(
        runner,
        tool_path(if cfg!(windows) { "adb.exe" } else { "adb" }),
        tool_path(if cfg!(windows) {
            "fastboot.exe"
        } else {
            "fastboot"
        }),
        TransportConfig::for_tests(),
    );
    WizardSession::with_clock(transport, Box::new(FixedClock(0)))
}

fn tool_path(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"C:\flashwright-test\{name}"))
    } else {
        PathBuf::from(format!("/opt/flashwright-test/{name}"))
    }
}

fn script(runner: &ScriptedRunner, patch_ok: bool) {
    let stock = SyntheticInitBoot::komodo();
    let lines = format!(
        "FL_STOCK_SHA256={}\nFL_OUT=/data/local/tmp/flashwright/out/patched.img\nFL_SHA1={PATCHED_SHA1}\n",
        stock.sha256_hex()
    );
    let name = if cfg!(windows) { "adb.exe" } else { "adb" };
    runner.on_fn(name, &["-s", SERIAL], move |invocation, _| {
        route(invocation, patch_ok, &lines)
    });
}

fn route(invocation: &Invocation, patch_ok: bool, lines: &str) -> ScriptedResponse {
    match invocation.args.get(2).map(String::as_str) {
        Some("push") => ScriptedResponse::ok("1 file pushed\n"),
        Some("pull") => ScriptedResponse::ok(""),
        Some("shell") => {
            let remote = invocation.args.get(3).map(String::as_str).unwrap_or("");
            if remote.contains("fl_patch.sh") {
                if patch_ok {
                    ScriptedResponse::ok(lines.to_string())
                } else {
                    ScriptedResponse::fail(1, "! stock image is missing\n")
                }
            } else if remote.contains("mkdir") || remote.contains("'rm'") || remote.contains("'ls'")
            {
                if remote.contains("'ls'") {
                    ScriptedResponse::ok(
                        "ls: /data/local/tmp/flashwright: No such file or directory\n",
                    )
                } else {
                    ScriptedResponse::ok("")
                }
            } else {
                ScriptedResponse::fail(1, "unexpected shell\n")
            }
        }
        _ => ScriptedResponse::fail(1, "unexpected command\n"),
    }
}

fn count_shell(calls: &[Invocation], needle: &str) -> usize {
    calls
        .iter()
        .filter(|call| {
            call.args.get(2).map(String::as_str) == Some("shell")
                && call.args.iter().any(|arg| arg.contains(needle))
        })
        .count()
}

fn sample_apk() -> Vec<u8> {
    let mut entries = sample_entries();
    entries.push(("../etc/passwd", b"nope"));
    zip_with(&entries)
}

fn sample_entries() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("assets/boot_patch.sh", b"#!/system/bin/sh\necho patch\n"),
        ("assets/util_functions.sh", b"echo util\n"),
        ("assets/app_functions.sh", b"echo app\n"),
        ("assets/stub.apk", b"stub"),
        ("lib/arm64-v8a/libbusybox.so", b"busybox-bytes"),
        ("lib/arm64-v8a/libmagiskboot.so", b"magiskboot-bytes"),
        ("lib/arm64-v8a/libmagiskinit.so", b"init-bytes"),
        ("lib/arm64-v8a/libmagisk.so", b"magisk-bytes"),
        ("lib/arm64-v8a/libinit-ld.so", b"ld-bytes"),
    ]
}

fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
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

fn hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn walk_names(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if path.is_dir() {
            if matches!(
                name.as_str(),
                "target" | ".git" | "node_modules" | ".netlify"
            ) {
                continue;
            }
            walk_names(&path);
            continue;
        }
        assert!(
            !matches!(
                name.as_str(),
                "magiskboot"
                    | "magiskboot.exe"
                    | "busybox"
                    | "busybox.exe"
                    | "libmagiskboot.so"
                    | "libbusybox.so"
            ),
            "bundled tool {}",
            path.display()
        );
    }
}
