// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::path::PathBuf;

use sha2::Digest;
use std::sync::Arc;

use crate::cmd::{
    AdbHostWrite, DeviceSerial, FastbootRead, FastbootVar, FastbootWrite, HostRef, ImageRef,
    ReadCmd, WorkFile, WriteCmd,
};
use crate::device::{Mode, Partition, RebootTarget, Slot, TransportConfig};
use crate::exe::{platform_tool, ListenerImage};
use crate::proc::{ScriptedResponse, ScriptedRunner};
use crate::token::mint_confirmed;
#[cfg(test)]
use crate::token::test_gate;

use super::PlatformToolsTransport;

fn adb_name() -> &'static str {
    if cfg!(windows) {
        "adb.exe"
    } else {
        "adb"
    }
}

fn fastboot_name() -> &'static str {
    if cfg!(windows) {
        "fastboot.exe"
    } else {
        "fastboot"
    }
}

fn tool_path(name: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"C:\flashwright-test\{name}"))
    } else {
        PathBuf::from(format!("/opt/flashwright-test/{name}"))
    }
}

fn serial() -> DeviceSerial {
    DeviceSerial::try_from("pixel1").unwrap()
}

fn missing_image(path: &str) -> ImageRef {
    ImageRef::new(0, path, 8 * 1024 * 1024)
}

fn transport(runner: Arc<ScriptedRunner>) -> PlatformToolsTransport<ScriptedRunner> {
    let dir = std::env::temp_dir().join(format!(
        "flashwright-write-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let adb_path = dir.join(adb_name());
    let fastboot_path = dir.join(fastboot_name());
    std::fs::write(&adb_path, b"adb-bytes").unwrap();
    std::fs::write(&fastboot_path, b"fastboot-bytes").unwrap();
    let adb_hash = sha256_file(&adb_path);
    let fastboot_hash = sha256_file(&fastboot_path);
    let adb = platform_tool(&adb_path, &adb_hash).unwrap();
    let fastboot = platform_tool(&fastboot_path, &fastboot_hash).unwrap();
    let listener = ListenerImage {
        path: adb_path.clone(),
        sha256: adb_hash,
    };
    let transport = PlatformToolsTransport::new(
        runner,
        adb_path,
        fastboot_path,
        TransportConfig::for_tests(),
    );
    transport.install_verified(adb, fastboot, Some(listener));
    transport.note_tools_verdict(true);
    transport
}

fn bare_transport(runner: Arc<ScriptedRunner>) -> PlatformToolsTransport<ScriptedRunner> {
    PlatformToolsTransport::new(
        runner,
        tool_path(adb_name()),
        tool_path(fastboot_name()),
        TransportConfig::for_tests(),
    )
}

fn arm(
    transport: &PlatformToolsTransport<ScriptedRunner>,
    steps: &[WriteCmd],
) -> crate::token::WriteToken {
    let (plan, token) = mint_confirmed("flp1-test", "pixel1", steps);
    transport.arm(&plan);
    token
}

#[tokio::test]
async fn a_scan_refuses_a_hashed_adb_that_is_not_allow_listed() {
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        adb_name(),
        &["devices", "-l"],
        ScriptedResponse::ok("pixel1 device\n"),
    );
    let dir = std::env::temp_dir().join(format!("flashwright-scan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let adb_path = dir.join(adb_name());
    std::fs::write(&adb_path, b"not-on-the-allow-list").unwrap();
    let transport = PlatformToolsTransport::new(
        Arc::clone(&runner),
        adb_path,
        tool_path(fastboot_name()),
        TransportConfig::for_tests(),
    );
    let err = transport.list().await.unwrap_err();
    assert!(err.to_string().contains("allow-listed"), "{err}");
    assert!(runner.calls().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_mismatched_write_spawns_nothing() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    let transport = transport(Arc::clone(&runner));
    let reboot = WriteCmd::AdbHost(AdbHostWrite::Reboot {
        serial: serial(),
        mode: RebootTarget::Bootloader,
    });
    let token = arm(&transport, &[reboot]);
    let flash = WriteCmd::Fastboot(FastbootWrite::Flash {
        serial: serial(),
        slot: Slot::B,
        partition: Partition::InitBoot,
        image: missing_image("/var/flashwright/init_boot.img"),
    });
    let err = transport.run_write(&token, flash).await.unwrap_err();
    assert!(err.to_string().contains("next plan step"));
    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn adb_reboot_waits_for_fastboot() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(adb_name(), &["devices", "-l"], ScriptedResponse::ok(""));
    runner.on(
        fastboot_name(),
        &["devices", "-l"],
        ScriptedResponse::ok("pixel1 fastboot\n"),
    );
    runner.on(
        adb_name(),
        &["-s", "pixel1", "reboot", "bootloader"],
        ScriptedResponse::ok(""),
    );
    let transport = transport(Arc::clone(&runner));
    let step = WriteCmd::AdbHost(AdbHostWrite::Reboot {
        serial: serial(),
        mode: RebootTarget::Bootloader,
    });
    let token = arm(&transport, &[step]);
    let outcome = transport
        .reboot(&token, "pixel1", Mode::Adb, RebootTarget::Bootloader)
        .await
        .unwrap();
    assert_eq!(outcome, crate::device::WaitOutcome::Reached);
}

#[tokio::test]
async fn lying_fastboot_exit_zero_is_not_a_flash() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    let image = "/var/flashwright/init_boot.img";
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "--slot", "b", "flash", "init_boot", image],
        ScriptedResponse::ok("OKAY\nFinished. Total time: 0.1s\nFAILED (remote: 'x')\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::Fastboot(FastbootWrite::Flash {
        serial: serial(),
        slot: Slot::B,
        partition: Partition::InitBoot,
        image: missing_image(image),
    });
    let token = arm(&transport, &[step]);
    let err = transport
        .fastboot_flash(
            &token,
            "pixel1",
            Slot::B,
            Partition::InitBoot,
            std::path::Path::new(image),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("fastboot flash"));
}

#[tokio::test]
async fn sideload_without_total_xfer_fails() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    let package = "/var/flashwright/ota.zip";
    runner.on(
        adb_name(),
        &["-s", "pixel1", "sideload", package],
        ScriptedResponse::ok("serving: 'ota'\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::AdbHost(AdbHostWrite::Sideload {
        serial: serial(),
        package: missing_image(package),
    });
    let token = arm(&transport, &[step]);
    let err = transport
        .sideload(
            &token,
            "pixel1",
            std::path::Path::new(package),
            Slot::B,
            Slot::A,
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("sideload"));
}

#[tokio::test]
async fn sideload_total_xfer_checks_the_slot() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    let package = "/var/flashwright/ota.zip";
    runner.on(
        adb_name(),
        &["-s", "pixel1", "sideload", package],
        ScriptedResponse::ok("Total xfer: 1.00x\n"),
    );
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "getvar", "current-slot"],
        ScriptedResponse::ok("current-slot: b\n"),
    );
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "getvar", "slot-unbootable:b"],
        ScriptedResponse::ok("slot-unbootable:b: no\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::AdbHost(AdbHostWrite::Sideload {
        serial: serial(),
        package: missing_image(package),
    });
    let token = arm(&transport, &[step]);
    transport
        .sideload(
            &token,
            "pixel1",
            std::path::Path::new(package),
            Slot::B,
            Slot::A,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn push_needs_the_completion_line() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    let local = "/var/flashwright/stock.img";
    runner.on(
        adb_name(),
        &["-s", "pixel1", "push", local],
        ScriptedResponse::ok("stock.img: 1 file pushed, 0 skipped.\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::AdbHost(AdbHostWrite::Push {
        serial: serial(),
        src: HostRef::Image(missing_image(local)),
        dst: WorkFile::Stock,
    });
    let token = arm(&transport, &[step]);
    transport
        .push(
            &token,
            "pixel1",
            std::path::Path::new(local),
            WorkFile::Stock,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn set_active_requires_the_new_slot() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "--set-active=b"],
        ScriptedResponse::ok("Setting current slot to 'b'\nOKAY\nFinished. Total time: 0.1s\n"),
    );
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "getvar", "current-slot"],
        ScriptedResponse::ok("current-slot: b\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::Fastboot(FastbootWrite::SetActive {
        serial: serial(),
        slot: Slot::B,
    });
    let token = arm(&transport, &[step]);
    transport
        .fastboot_set_active(&token, "pixel1", Slot::B)
        .await
        .unwrap();
    let _ = ReadCmd::Fastboot(FastbootRead::Getvar {
        serial: serial(),
        var: FastbootVar::CurrentSlot,
    });
}

#[tokio::test]
async fn update_needs_finished_and_not_the_source_slot() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    let package = "/var/flashwright/image.zip";
    runner.on(
        fastboot_name(),
        &[
            "-s",
            "pixel1",
            "--slot",
            "b",
            "--skip-reboot",
            "update",
            package,
        ],
        ScriptedResponse::ok("Finished. Total time: 1.0s\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::Fastboot(FastbootWrite::Update {
        serial: serial(),
        slot: Slot::B,
        package: missing_image(package),
    });
    let token = arm(&transport, &[step]);
    transport
        .fastboot_update(
            &token,
            "pixel1",
            Slot::B,
            Slot::A,
            std::path::Path::new(package),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn changed_tools_or_a_foreign_adb_server_block_the_write() {
    let _gate = test_gate().await;
    let dir = std::env::temp_dir().join(format!("flashwright-locks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let adb_path = dir.join(adb_name());
    let fastboot_path = dir.join(fastboot_name());
    std::fs::write(&adb_path, b"adb-bytes").unwrap();
    std::fs::write(&fastboot_path, b"fastboot-bytes").unwrap();
    let adb_hash = sha256_file(&adb_path);
    let fastboot_hash = sha256_file(&fastboot_path);
    let adb = platform_tool(&adb_path, &adb_hash).unwrap();
    let fastboot = platform_tool(&fastboot_path, &fastboot_hash).unwrap();

    let runner = Arc::new(ScriptedRunner::new());
    let transport = transport(Arc::clone(&runner));
    transport.install_verified(adb, fastboot, None);
    let step = WriteCmd::AdbHost(AdbHostWrite::Reboot {
        serial: serial(),
        mode: RebootTarget::System,
    });
    let token = arm(&transport, &[step]);
    let err = transport
        .run_write(
            &token,
            WriteCmd::AdbHost(AdbHostWrite::Reboot {
                serial: serial(),
                mode: RebootTarget::System,
            }),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("5037"));
    assert!(runner.calls().is_empty());

    transport.invalidate_plan();
    let _ = ListenerImage {
        path: adb_path,
        sha256: adb_hash,
    };
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_write_without_verified_tools_is_blocked() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    let transport = bare_transport(Arc::clone(&runner));
    let step = WriteCmd::AdbHost(AdbHostWrite::Reboot {
        serial: serial(),
        mode: RebootTarget::System,
    });
    let token = arm(&transport, &[step]);
    let err = transport
        .run_write(
            &token,
            WriteCmd::AdbHost(AdbHostWrite::Reboot {
                serial: serial(),
                mode: RebootTarget::System,
            }),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("G21"));
    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn set_active_reads_the_slot_from_stderr() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "--set-active=b"],
        ScriptedResponse::ok("Setting current slot to 'b'\nOKAY\nFinished. Total time: 0.1s\n"),
    );
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "getvar", "current-slot"],
        ScriptedResponse::ok("Finished. Total time: 0.1s\n")
            .with_stderr("(bootloader) current-slot: b\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::Fastboot(FastbootWrite::SetActive {
        serial: serial(),
        slot: Slot::B,
    });
    let token = arm(&transport, &[step]);
    transport
        .fastboot_set_active(&token, "pixel1", Slot::B)
        .await
        .unwrap();
}

#[tokio::test]
async fn set_active_does_not_treat_total_as_slot_a() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "--set-active=a"],
        ScriptedResponse::ok("Setting current slot to 'a'\nOKAY\nFinished. Total time: 0.1s\n"),
    );
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "getvar", "current-slot"],
        ScriptedResponse::ok("Finished. Total time: 0.1s\n")
            .with_stderr("(bootloader) current-slot: b\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::Fastboot(FastbootWrite::SetActive {
        serial: serial(),
        slot: Slot::A,
    });
    let token = arm(&transport, &[step]);
    let err = transport
        .fastboot_set_active(&token, "pixel1", Slot::A)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("current-slot"));
}

#[tokio::test]
async fn sideload_post_state_reads_stderr() {
    let _gate = test_gate().await;
    let runner = Arc::new(ScriptedRunner::new());
    let package = "/var/flashwright/ota.zip";
    runner.on(
        adb_name(),
        &["-s", "pixel1", "sideload", package],
        ScriptedResponse::ok("Total xfer: 1.00x\n"),
    );
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "getvar", "current-slot"],
        ScriptedResponse::ok("Finished. Total time: 0.001s\n")
            .with_stderr("(bootloader) current-slot: b\n"),
    );
    runner.on(
        fastboot_name(),
        &["-s", "pixel1", "getvar", "slot-unbootable:b"],
        ScriptedResponse::ok("Finished. Total time: 0.001s\n")
            .with_stderr("(bootloader) slot-unbootable:b: no\n"),
    );
    let transport = transport(runner);
    let step = WriteCmd::AdbHost(AdbHostWrite::Sideload {
        serial: serial(),
        package: missing_image(package),
    });
    let token = arm(&transport, &[step]);
    transport
        .sideload(
            &token,
            "pixel1",
            std::path::Path::new(package),
            Slot::B,
            Slot::A,
        )
        .await
        .unwrap();
}

#[test]
fn a_changed_library_fails_the_tool_gate() {
    let dir = std::env::temp_dir().join(format!(
        "flashwright-dll-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let adb_path = dir.join(adb_name());
    let fastboot_path = dir.join(fastboot_name());
    let dll = dir.join("AdbWinApi.dll");
    std::fs::write(&adb_path, b"adb-bytes").unwrap();
    std::fs::write(&fastboot_path, b"fastboot-bytes").unwrap();
    std::fs::write(&dll, b"dll-v1").unwrap();
    let adb = platform_tool(&adb_path, &sha256_file(&adb_path)).unwrap();
    let fastboot = platform_tool(&fastboot_path, &sha256_file(&fastboot_path)).unwrap();
    let listener = ListenerImage {
        path: adb_path.clone(),
        sha256: sha256_file(&adb_path),
    };
    let transport = PlatformToolsTransport::new(
        Arc::new(ScriptedRunner::new()),
        adb_path,
        fastboot_path,
        TransportConfig::for_tests(),
    );
    transport.install_verified(adb, fastboot, Some(listener));
    let (_installed, matched, _server) = transport.tool_gate();
    assert!(matched);
    std::fs::write(&dll, b"dll-v2").unwrap();
    let (_installed, matched, _server) = transport.tool_gate();
    assert!(!matched);
    let _ = std::fs::remove_dir_all(&dir);
}

fn sha256_file(path: &std::path::Path) -> String {
    use std::io::Read;
    let mut file = std::fs::File::open(path).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let digest = sha2::Sha256::digest(&bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
