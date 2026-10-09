// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! M3 patch tests. adb is a scripted stand-in. No phone is contacted.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flashwright_core::cmd::WorkFile;
use flashwright_core::device::{Partition, PlatformToolsTransport, TransportConfig};
use flashwright_core::proc::{Invocation, ScriptedResponse, ScriptedRunner};
use flashwright_core::wizard::{FixedClock, WizardSession};
use flashwright_magisk::{
    check_device_space, check_magisk_version, check_patched_sha1, check_region, embedded_known_bad,
    extract_apk_components, komodo_has_init_boot, offer, patch_partition, plan_app_patch, store,
    validate_patched_init_boot, AppPatchRequest, DeviceAbi, ExtractedBootImage, HostComponent,
    MagiskError, MagiskbootPolicy, PatchCacheMeta, PatchedCheck, SyntheticInitBoot, FINALLY_STEPS,
    HIDDEN_APP,
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
    assert_eq!(err.to_string(), HIDDEN_APP);
}

#[tokio::test]
async fn unconfirmed_plan_spawns_nothing() {
    let runner = Arc::new(ScriptedRunner::new());
    script(&runner, true);
    let mut session = session(Arc::clone(&runner));
    let stock = SyntheticInitBoot::komodo();
    let components = host_components();
    let plan = plan_app_patch(&request(
        &stock,
        &components,
        DUMPS,
        PLENTY,
        30_700,
        "2026-01-01",
        &[],
    ))
    .unwrap();
    assert_eq!(plan.title, "Patch on your phone?");
    assert_eq!(plan.button, "Patch now");
    assert_eq!(plan.finally_steps, FINALLY_STEPS);
    let preview = session.build_plan(plan.draft).unwrap();
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
        .confirm_and_run_finally("flp1-not-issued", FINALLY_STEPS)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not issued"));
    assert!(runner.calls().is_empty());
    let listed = session.dry_run(&preview.plan_hash).unwrap();
    assert!(listed.iter().any(|line| line.starts_with("WOULD RUN:")));
    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn confirmed_app_patch_runs_three_times() {
    let runner = Arc::new(ScriptedRunner::new());
    script(&runner, true);
    let stock = SyntheticInitBoot::komodo();
    for _ in 0..3 {
        let mut session = session(Arc::clone(&runner));
        let components = host_components();
        let plan = plan_app_patch(&request(
            &stock,
            &components,
            DUMPS,
            PLENTY,
            30_700,
            "2026-01-01",
            &[],
        ))
        .unwrap();
        let preview = session.build_plan(plan.draft).unwrap();
        let report = session
            .confirm_and_run_finally(&preview.plan_hash, plan.finally_steps)
            .await
            .unwrap();
        let text = report.lines.join("\n");
        assert!(text.contains(&format!("FL_STOCK_SHA256={}", stock.sha256_hex())));
        assert!(text.contains("FL_OUT=/data/local/tmp/flashwright/out/patched.img"));
        assert!(text.contains(&format!("FL_SHA1={PATCHED_SHA1}")));
    }
    let calls = runner.calls();
    assert_eq!(count_shell(&calls, "fl_patch.sh"), 3);
    assert_eq!(count_shell(&calls, "'rm'"), 3);
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
    let components = host_components();
    let plan = plan_app_patch(&request(
        &stock,
        &components,
        DUMPS,
        PLENTY,
        30_700,
        "2026-01-01",
        &[],
    ))
    .unwrap();
    let preview = session.build_plan(plan.draft).unwrap();
    let err = session
        .confirm_and_run_finally(&preview.plan_hash, plan.finally_steps)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("write step"));
    assert!(count_remote(&runner.calls(), "'rm'") >= 1);
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
    let components = host_components();
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
    assert!(embedded_known_bad().unwrap().is_empty());
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

#[tokio::test]
async fn pc_magiskboot_checks_the_ramdisk() {
    let dir = std::env::temp_dir().join("flashwright-m3-magiskboot");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tool_path = dir.join(if cfg!(windows) {
        "magiskboot.exe"
    } else {
        "magiskboot"
    });
    std::fs::write(&tool_path, b"magiskboot-stand-in").unwrap();
    let digest = hex(Sha256::digest(b"magiskboot-stand-in"));
    let stock = SyntheticInitBoot::komodo();
    let image = dir.join("init_boot.img");
    std::fs::write(&image, stock.bytes()).unwrap();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let patched = hex(Sha256::digest(b"patched-init-boot-stand-in"));
    let runner = ScriptedRunner::new();
    let name = tool_path.file_name().unwrap().to_str().unwrap().to_string();
    runner.on(
        &name,
        &["unpack"],
        ScriptedResponse::ok("RAMDISK_FMT     [gzip]\n"),
    );
    runner.on(
        &name,
        &["cpio", "ramdisk.cpio", "test"],
        ScriptedResponse::ok("Magisk detected\n"),
    );
    runner.on(
        &name,
        &["cpio", "ramdisk.cpio", "exists", "init"],
        ScriptedResponse::ok(""),
    );
    runner.on(
        &name,
        &["cpio", "ramdisk.cpio", "extract"],
        ScriptedResponse::ok(format!("SHA1={}\n", stock.sha1_hex())),
    );
    let report = validate_patched_init_boot(
        &runner,
        pc_check(&tool_path, &digest, &image, &work, &stock, &patched),
    )
    .await
    .unwrap();
    assert_eq!(report.ramdisk_format, "gzip");
    assert!(report.magisk_init);
    assert_eq!(report.config_sha1, stock.sha1_hex());

    let mismatch = ScriptedRunner::new();
    mismatch.on(
        &name,
        &["unpack"],
        ScriptedResponse::ok("RAMDISK_FMT     [gzip]\n"),
    );
    mismatch.on(
        &name,
        &["cpio", "ramdisk.cpio", "test"],
        ScriptedResponse::ok("Magisk detected\n"),
    );
    mismatch.on(
        &name,
        &["cpio", "ramdisk.cpio", "exists", "init"],
        ScriptedResponse::ok(""),
    );
    mismatch.on(
        &name,
        &["cpio", "ramdisk.cpio", "extract"],
        ScriptedResponse::ok("SHA1=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n"),
    );
    assert!(validate_patched_init_boot(
        &mismatch,
        pc_check(&tool_path, &digest, &image, &work, &stock, &patched),
    )
    .await
    .is_err());

    let unknown = ScriptedRunner::new();
    unknown.on(
        &name,
        &["unpack"],
        ScriptedResponse::ok("RAMDISK_FMT     [xz]\n"),
    );
    let err = validate_patched_init_boot(
        &unknown,
        pc_check(&tool_path, &digest, &image, &work, &stock, &patched),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("unsupported ramdisk format"));
    let wrong = "0".repeat(64);
    assert!(validate_patched_init_boot(
        &unknown,
        pc_check(&tool_path, &wrong, &image, &work, &stock, &patched),
    )
    .await
    .is_err());
    let _ = std::fs::remove_dir_all(&dir);
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
    let policy = MagiskbootPolicy::embedded().unwrap();
    assert!(!policy.bundled);
    assert!(!MagiskbootPolicy::text()
        .to_ascii_lowercase()
        .contains("sha256"));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    walk_names(&root);
}

#[tokio::test]
async fn real_magiskboot_is_opt_in() {
    let Ok(tool) = std::env::var("FLASHWRIGHT_MAGISKBOOT") else {
        return;
    };
    let Ok(image) = std::env::var("FLASHWRIGHT_INIT_BOOT") else {
        return;
    };
    let Ok(digest) = std::env::var("FLASHWRIGHT_MAGISKBOOT_SHA256") else {
        return;
    };
    let Ok(stock_sha1) = std::env::var("FLASHWRIGHT_STOCK_SHA1") else {
        return;
    };
    let Ok(stock_sha256) = std::env::var("FLASHWRIGHT_STOCK_SHA256") else {
        return;
    };
    let Ok(patched_sha256) = std::env::var("FLASHWRIGHT_PATCHED_SHA256") else {
        return;
    };
    let work = std::env::temp_dir().join("flashwright-m3-real-magiskboot");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let runner = flashwright_core::proc::SystemRunner;
    validate_patched_init_boot(
        &runner,
        PatchedCheck {
            tool_path: Path::new(&tool),
            expected_sha256: &digest,
            image: Path::new(&image),
            work: &work,
            stock_sha1: &stock_sha1,
            stock_sha256: &stock_sha256,
            patched_sha256: &patched_sha256,
        },
    )
    .await
    .expect("opt-in magiskboot check");
    let _ = std::fs::remove_dir_all(&work);
}

fn pc_check<'a>(
    tool_path: &'a Path,
    digest: &'a str,
    image: &'a Path,
    work: &'a Path,
    stock: &'a SyntheticInitBoot,
    patched: &'a str,
) -> PatchedCheck<'a> {
    PatchedCheck {
        tool_path,
        expected_sha256: digest,
        image,
        work,
        stock_sha1: stock.sha1_hex(),
        stock_sha256: stock.sha256_hex(),
        patched_sha256: patched,
    }
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

fn count_remote(calls: &[Invocation], needle: &str) -> usize {
    calls
        .iter()
        .filter(|call| call.args.iter().any(|arg| arg.contains(needle)))
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
