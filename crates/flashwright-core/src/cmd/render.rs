// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Render a catalogue command to one argv vector.
//!
//! The phone-side command is a single argument after `shell` or `exec-out`.
//! Each token is wrapped with [`sh_quote`]. `su -c` inner commands are one
//! double-quoted token. The only unquoted shell syntax is `| sha256sum`.

use super::{
    AdbHostRead, AdbHostWrite, AdbShellRead, AdbShellWrite, ByNameRoot, CleanupCmd, CmdError,
    DeviceSerial, ExecOutSuRead, FastbootRead, FastbootWrite, ReadCmd, SuRead, SuWrite, WorkFile,
    WriteCmd,
};
use crate::device::{Partition, RebootTarget, Slot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Adb,
    Fastboot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub tool: Tool,
    pub args: Vec<String>,
}

pub fn sh_quote(token: &str) -> String {
    let mut out = String::from("'");
    for ch in token.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

pub fn read_argv(cmd: &ReadCmd) -> Result<Rendered, CmdError> {
    let args = match cmd {
        ReadCmd::AdbHost(AdbHostRead::Version) => vec!["version".into()],
        ReadCmd::AdbHost(AdbHostRead::Devices) | ReadCmd::Fastboot(FastbootRead::Devices) => {
            vec!["devices".into(), "-l".into()]
        }
        ReadCmd::AdbHost(AdbHostRead::GetState { serial }) => serial_args(serial, &["get-state"]),
        ReadCmd::AdbHost(AdbHostRead::Pull {
            serial,
            remote,
            dst_name,
        }) => {
            let remote = match remote {
                super::PullRemote::Validated(path) => path.as_str(),
                super::PullRemote::Work(file) => file.device_path(),
            };
            let mut args = serial_args(serial, &["pull", remote]);
            args.push(dst_name.as_str().to_string());
            args
        }
        ReadCmd::AdbShell(shell) => shell_remote(shell.serial_ref(), &render_shell(shell)?),
        ReadCmd::Su(su) => shell_remote(su.serial_ref(), &render_su(su)?),
        ReadCmd::ExecOutSu(ExecOutSuRead::CatBlock {
            serial,
            root,
            partition,
            slot,
        }) => {
            let path = block_path(*root, *partition, *slot);
            let mut args = serial_args(serial, &["exec-out"]);
            args.push(su_multi(&["cat", &path]));
            args
        }
        ReadCmd::Fastboot(FastbootRead::GetvarAll { serial }) => {
            serial_args(serial, &["getvar", "all"])
        }
        ReadCmd::Fastboot(FastbootRead::Getvar { serial, var }) => {
            let name = var.as_str();
            serial_args(serial, &["getvar", &name])
        }
    };
    reject_read_verbs(&args)?;
    let tool = if cmd.uses_fastboot() {
        Tool::Fastboot
    } else {
        Tool::Adb
    };
    Ok(Rendered { tool, args })
}

pub fn cleanup_argv(cmd: &CleanupCmd) -> Result<Rendered, CmdError> {
    let CleanupCmd::RemoveWorkDir { serial } = cmd;
    Ok(Rendered {
        tool: Tool::Adb,
        args: shell_remote(
            serial,
            &join_quoted(&["rm", "-rf", "/data/local/tmp/flashwright"]),
        ),
    })
}

pub fn write_argv(cmd: &WriteCmd) -> Result<Rendered, CmdError> {
    let args = match cmd {
        WriteCmd::AdbHost(AdbHostWrite::Push { serial, src, dst }) => {
            let mut args = serial_args(serial, &["push", src.path()]);
            args.push(dst.device_path().into());
            args
        }
        WriteCmd::AdbHost(AdbHostWrite::Reboot { serial, mode }) => reboot_args(serial, *mode),
        WriteCmd::AdbHost(AdbHostWrite::Sideload { serial, package }) => {
            serial_args(serial, &["sideload", package.path()])
        }
        WriteCmd::AdbShell(AdbShellWrite::MakeWorkDir { serial }) => shell_remote(
            serial,
            &join_quoted(&["mkdir", "-p", "/data/local/tmp/flashwright/out"]),
        ),
        WriteCmd::AdbShell(AdbShellWrite::RunPatchScript { serial }) => shell_remote(
            serial,
            &join_quoted(&["sh", WorkFile::PatchScript.device_path()]),
        ),
        WriteCmd::Su(SuWrite::RunPatchScript { serial }) => shell_remote(
            serial,
            &su_multi(&["sh", WorkFile::PatchScript.device_path()]),
        ),
        WriteCmd::Fastboot(FastbootWrite::Flash {
            serial,
            slot,
            partition,
            image,
        }) => {
            if !partition.is_writable() {
                return Err(CmdError::ReadOnlyPartition {
                    partition: partition.to_string(),
                });
            }
            serial_args(
                serial,
                &[
                    "--slot",
                    slot.as_str(),
                    "flash",
                    partition.fastboot_name(),
                    image.path(),
                ],
            )
        }
        WriteCmd::Fastboot(FastbootWrite::SetActive { serial, slot }) => {
            vec![
                "-s".into(),
                serial.as_str().into(),
                format!("--set-active={}", slot.as_str()),
            ]
        }
        WriteCmd::Fastboot(FastbootWrite::Update {
            serial,
            slot,
            package,
        }) => serial_args(
            serial,
            &[
                "--slot",
                slot.as_str(),
                "--skip-reboot",
                "update",
                package.path(),
            ],
        ),
        WriteCmd::Fastboot(FastbootWrite::Reboot { serial, mode }) => reboot_args(serial, *mode),
    };
    let tool = if cmd.uses_fastboot() {
        Tool::Fastboot
    } else {
        Tool::Adb
    };
    Ok(Rendered { tool, args })
}

fn render_shell(cmd: &AdbShellRead) -> Result<String, CmdError> {
    let rendered = match cmd {
        AdbShellRead::GetpropAll { .. } => join_quoted(&["getprop"]),
        AdbShellRead::Getprop { name, .. } => join_quoted(&["getprop", name.as_str()]),
        AdbShellRead::LsBlockByName {
            root,
            partition,
            slot,
            ..
        } => join_quoted(&["ls", &block_path(*root, *partition, *slot)]),
        AdbShellRead::LsWorkDir { .. } => join_quoted(&["ls", "/data/local/tmp/flashwright"]),
        AdbShellRead::DumpsysBattery { .. } => join_quoted(&["dumpsys", "battery"]),
        AdbShellRead::DumpsysDiskstats { .. } => join_quoted(&["dumpsys", "diskstats"]),
        AdbShellRead::DumpsysPackage { package, .. } => {
            join_quoted(&["dumpsys", "package", package.as_str()])
        }
    };
    Ok(rendered)
}

fn render_su(cmd: &SuRead) -> Result<String, CmdError> {
    Ok(match cmd {
        SuRead::Id { .. } => su_single("id"),
        SuRead::MagiskVersion { .. } => su_multi(&["magisk", "-v"]),
        SuRead::MagiskVersionCode { .. } => su_multi(&["magisk", "-V"]),
        SuRead::LsMagiskDir { .. } => su_multi(&["ls", "/data/adb/magisk"]),
        SuRead::Sha256Block {
            root,
            partition,
            slot,
            ..
        } => {
            let path = block_path(*root, *partition, *slot);
            su_multi(&["sha256sum", &path])
        }
        SuRead::Sha256BlockPrefix {
            root,
            partition,
            slot,
            len,
            ..
        } => {
            let path = block_path(*root, *partition, *slot);
            let len_text = len.get().to_string();
            su_pipe(&["head", "-c", &len_text, &path], " | sha256sum")
        }
    })
}

fn su_single(token: &str) -> String {
    format!("{} {} {}", sh_quote("su"), sh_quote("-c"), sh_quote(token))
}

fn su_multi(tokens: &[&str]) -> String {
    let inner = join_quoted(tokens);
    format!("{} {} \"{inner}\"", sh_quote("su"), sh_quote("-c"))
}

fn su_pipe(tokens: &[&str], pipe: &str) -> String {
    let inner = join_quoted(tokens);
    format!("{} {} \"{inner}{pipe}\"", sh_quote("su"), sh_quote("-c"))
}

fn join_quoted(tokens: &[&str]) -> String {
    tokens
        .iter()
        .map(|token| sh_quote(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn block_path(root: ByNameRoot, partition: Partition, slot: Slot) -> String {
    format!(
        "{}/{}{}",
        root.as_str(),
        partition.fastboot_name(),
        slot.suffix()
    )
}

fn serial_args(serial: &DeviceSerial, tail: &[&str]) -> Vec<String> {
    let mut args = Vec::with_capacity(tail.len() + 2);
    args.push("-s".into());
    args.push(serial.as_str().into());
    args.extend(tail.iter().map(|part| (*part).to_string()));
    args
}

fn shell_remote(serial: &DeviceSerial, remote: &str) -> Vec<String> {
    vec![
        "-s".into(),
        serial.as_str().into(),
        "shell".into(),
        remote.into(),
    ]
}

fn reboot_args(serial: &DeviceSerial, mode: RebootTarget) -> Vec<String> {
    match mode {
        RebootTarget::System => serial_args(serial, &["reboot"]),
        RebootTarget::Bootloader => serial_args(serial, &["reboot", "bootloader"]),
        RebootTarget::Sideload => serial_args(serial, &["reboot", "sideload"]),
    }
}

fn reject_read_verbs(args: &[String]) -> Result<(), CmdError> {
    if super::read_contains_write_verb(args).is_some() {
        return Err(CmdError::Rejected {
            field: "read",
            issue: "rejected",
        });
    }
    Ok(())
}

impl AdbShellRead {
    fn serial_ref(&self) -> &DeviceSerial {
        match self {
            Self::GetpropAll { serial }
            | Self::Getprop { serial, .. }
            | Self::LsBlockByName { serial, .. }
            | Self::LsWorkDir { serial }
            | Self::DumpsysBattery { serial }
            | Self::DumpsysDiskstats { serial }
            | Self::DumpsysPackage { serial, .. } => serial,
        }
    }
}

impl SuRead {
    fn serial_ref(&self) -> &DeviceSerial {
        match self {
            Self::Id { serial }
            | Self::MagiskVersion { serial }
            | Self::MagiskVersionCode { serial }
            | Self::LsMagiskDir { serial }
            | Self::Sha256Block { serial, .. }
            | Self::Sha256BlockPrefix { serial, .. } => serial,
        }
    }
}

#[cfg(test)]
mod golden {
    use super::*;
    use crate::cmd::{
        AdbHostRead, AdbHostWrite, AdbShellRead, AdbShellWrite, ByNameRoot, ByteLen, DeviceSerial,
        ExecOutSuRead, FastbootRead, FastbootVar, FastbootWrite, HostRef, ImageRef, PackageName,
        PropName, PullName, PullRemote, ReadCmd, SuRead, SuWrite, WorkFile, WriteCmd,
    };
    use crate::device::{Partition, RebootTarget, Slot};

    fn serial() -> DeviceSerial {
        DeviceSerial::try_from("pixel1").unwrap()
    }

    fn line(name: &str, rendered: &Rendered) -> String {
        let tool = match rendered.tool {
            Tool::Adb => "adb",
            Tool::Fastboot => "fastboot",
        };
        format!("{name} {tool} {}", rendered.args.join(" | "))
    }

    fn catalogue() -> String {
        let serial = serial();
        let prop = PropName::try_from("ro.build.fingerprint").unwrap();
        let package = PackageName::magisk_app();
        let path = crate::cmd::ValidatedDevicePath::from_code_path(
            "/data/app/~~abc==/com.topjohnwu.magisk-xyz",
        )
        .unwrap();
        let len = ByteLen::try_from(4096u64).unwrap();
        let image = ImageRef::new(1, "/var/flashwright/init_boot.img", 8);
        let rows = [
            (
                "version",
                read_argv(&ReadCmd::AdbHost(AdbHostRead::Version)).unwrap(),
            ),
            (
                "adb-devices",
                read_argv(&ReadCmd::AdbHost(AdbHostRead::Devices)).unwrap(),
            ),
            (
                "fastboot-devices",
                read_argv(&ReadCmd::Fastboot(FastbootRead::Devices)).unwrap(),
            ),
            (
                "get-state",
                read_argv(&ReadCmd::AdbHost(AdbHostRead::GetState {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "pull-apk",
                read_argv(&ReadCmd::AdbHost(AdbHostRead::Pull {
                    serial: serial.clone(),
                    remote: PullRemote::Validated(path),
                    dst_name: PullName::new("base.apk").unwrap(),
                }))
                .unwrap(),
            ),
            (
                "pull-patched",
                read_argv(&ReadCmd::AdbHost(AdbHostRead::Pull {
                    serial: serial.clone(),
                    remote: PullRemote::Work(WorkFile::Patched),
                    dst_name: PullName::new("patched.img").unwrap(),
                }))
                .unwrap(),
            ),
            (
                "getprop-all",
                read_argv(&ReadCmd::AdbShell(AdbShellRead::GetpropAll {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "getprop-one",
                read_argv(&ReadCmd::AdbShell(AdbShellRead::Getprop {
                    serial: serial.clone(),
                    name: prop,
                }))
                .unwrap(),
            ),
            (
                "ls-init-boot",
                read_argv(&ReadCmd::AdbShell(AdbShellRead::LsBlockByName {
                    serial: serial.clone(),
                    root: ByNameRoot::ByName,
                    partition: Partition::InitBoot,
                    slot: Slot::B,
                }))
                .unwrap(),
            ),
            (
                "ls-work",
                read_argv(&ReadCmd::AdbShell(AdbShellRead::LsWorkDir {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "battery",
                read_argv(&ReadCmd::AdbShell(AdbShellRead::DumpsysBattery {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "diskstats",
                read_argv(&ReadCmd::AdbShell(AdbShellRead::DumpsysDiskstats {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "package",
                read_argv(&ReadCmd::AdbShell(AdbShellRead::DumpsysPackage {
                    serial: serial.clone(),
                    package,
                }))
                .unwrap(),
            ),
            (
                "su-id",
                read_argv(&ReadCmd::Su(SuRead::Id {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "magisk-v",
                read_argv(&ReadCmd::Su(SuRead::MagiskVersion {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "magisk-code",
                read_argv(&ReadCmd::Su(SuRead::MagiskVersionCode {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "ls-magisk",
                read_argv(&ReadCmd::Su(SuRead::LsMagiskDir {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "sha256-block",
                read_argv(&ReadCmd::Su(SuRead::Sha256Block {
                    serial: serial.clone(),
                    root: ByNameRoot::BootdeviceByName,
                    partition: Partition::Boot,
                    slot: Slot::A,
                }))
                .unwrap(),
            ),
            (
                "sha256-prefix",
                read_argv(&ReadCmd::Su(SuRead::Sha256BlockPrefix {
                    serial: serial.clone(),
                    root: ByNameRoot::ByName,
                    partition: Partition::InitBoot,
                    slot: Slot::B,
                    len,
                }))
                .unwrap(),
            ),
            (
                "cat-block",
                read_argv(&ReadCmd::ExecOutSu(ExecOutSuRead::CatBlock {
                    serial: serial.clone(),
                    root: ByNameRoot::ByName,
                    partition: Partition::Boot,
                    slot: Slot::A,
                }))
                .unwrap(),
            ),
            (
                "getvar-all",
                read_argv(&ReadCmd::Fastboot(FastbootRead::GetvarAll {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "getvar-slot",
                read_argv(&ReadCmd::Fastboot(FastbootRead::Getvar {
                    serial: serial.clone(),
                    var: FastbootVar::PartitionSize {
                        partition: Partition::InitBoot,
                        slot: Slot::B,
                    },
                }))
                .unwrap(),
            ),
            (
                "push",
                write_argv(&WriteCmd::AdbHost(AdbHostWrite::Push {
                    serial: serial.clone(),
                    src: HostRef::Image(image.clone()),
                    dst: WorkFile::Stock,
                }))
                .unwrap(),
            ),
            (
                "reboot-system",
                write_argv(&WriteCmd::AdbHost(AdbHostWrite::Reboot {
                    serial: serial.clone(),
                    mode: RebootTarget::System,
                }))
                .unwrap(),
            ),
            (
                "reboot-bootloader",
                write_argv(&WriteCmd::Fastboot(FastbootWrite::Reboot {
                    serial: serial.clone(),
                    mode: RebootTarget::Bootloader,
                }))
                .unwrap(),
            ),
            (
                "sideload",
                write_argv(&WriteCmd::AdbHost(AdbHostWrite::Sideload {
                    serial: serial.clone(),
                    package: image.clone(),
                }))
                .unwrap(),
            ),
            (
                "mkdir",
                write_argv(&WriteCmd::AdbShell(AdbShellWrite::MakeWorkDir {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "rm",
                cleanup_argv(&CleanupCmd::RemoveWorkDir {
                    serial: serial.clone(),
                })
                .unwrap(),
            ),
            (
                "patch-su",
                write_argv(&WriteCmd::Su(SuWrite::RunPatchScript {
                    serial: serial.clone(),
                }))
                .unwrap(),
            ),
            (
                "flash",
                write_argv(&WriteCmd::Fastboot(FastbootWrite::Flash {
                    serial: serial.clone(),
                    slot: Slot::B,
                    partition: Partition::InitBoot,
                    image: image.clone(),
                }))
                .unwrap(),
            ),
            (
                "set-active",
                write_argv(&WriteCmd::Fastboot(FastbootWrite::SetActive {
                    serial: serial.clone(),
                    slot: Slot::B,
                }))
                .unwrap(),
            ),
            (
                "update",
                write_argv(&WriteCmd::Fastboot(FastbootWrite::Update {
                    serial: serial.clone(),
                    slot: Slot::B,
                    package: image,
                }))
                .unwrap(),
            ),
        ];
        let mut text = String::new();
        for (name, rendered) in rows {
            text.push_str(&line(name, &rendered));
            text.push('\n');
        }
        text
    }

    #[test]
    fn golden_argv_file_matches_the_renderer() {
        let actual = catalogue();
        let expected = include_str!("golden_argv.txt");
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/cmd/golden_argv.txt");
            std::fs::write(path, &actual).unwrap();
        }
        assert_eq!(actual, expected.replace("\r\n", "\n"));
    }
}
