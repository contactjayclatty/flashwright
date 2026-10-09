// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Incremental output parsers. Exit status 0 is never enough on its own.

use crate::device::{Partition, Slot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Failed { reason: FailReason },
    Uncertain { reason: &'static str },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailReason {
    Global,
    WrongTarget,
    NoCompletionMarker,
    DeviceMissing,
    OtaNotApplied,
    MissingMarker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootCheck {
    RootOk,
    RootNotGranted,
    RootMissing,
    FlashMismatch,
    WrongSlotOrBuild,
    NotBooted,
}

pub fn classify_root(
    slot_ok: bool,
    fingerprint_ok: bool,
    su_ok: bool,
    su_missing: bool,
    magisk_ok: bool,
    hash_matches: bool,
) -> RootCheck {
    if !slot_ok || !fingerprint_ok {
        return RootCheck::WrongSlotOrBuild;
    }
    if su_missing {
        return RootCheck::RootMissing;
    }
    if !su_ok {
        return RootCheck::RootNotGranted;
    }
    if !magisk_ok {
        return RootCheck::RootMissing;
    }
    if hash_matches {
        RootCheck::RootOk
    } else {
        RootCheck::FlashMismatch
    }
}

pub fn parse_flash(text: &str, partition: Partition, slot: Slot) -> Verdict {
    if let Some(verdict) = global_failure(text) {
        return verdict;
    }
    let expected = format!("{}_{}", partition.fastboot_name(), slot.as_str());
    let mut saw_sending = false;
    let mut saw_writing = false;
    let mut phase_open = false;
    let mut finished = false;
    for line in lines(text) {
        if let Some(name) = phase_name(&line, "Sending") {
            if phase_open {
                return Verdict::Failed {
                    reason: FailReason::MissingMarker,
                };
            }
            if name != expected {
                return Verdict::Failed {
                    reason: FailReason::WrongTarget,
                };
            }
            saw_sending = true;
            phase_open = !line.contains("OKAY");
            continue;
        }
        if let Some(name) = phase_name(&line, "Writing") {
            if phase_open {
                return Verdict::Failed {
                    reason: FailReason::MissingMarker,
                };
            }
            if name != expected {
                return Verdict::Failed {
                    reason: FailReason::WrongTarget,
                };
            }
            saw_writing = true;
            phase_open = !line.contains("OKAY");
            continue;
        }
        if line.contains("OKAY") && phase_open {
            phase_open = false;
            continue;
        }
        if line.starts_with("Finished.") {
            finished = true;
        }
    }
    if saw_sending && saw_writing && !phase_open && finished {
        Verdict::Ok
    } else {
        Verdict::Failed {
            reason: FailReason::MissingMarker,
        }
    }
}

pub fn parse_set_active(text: &str, slot: Slot) -> Verdict {
    if let Some(verdict) = global_failure(text) {
        return verdict;
    }
    let setting = format!("Setting current slot to '{}'", slot.as_str());
    let ok = text.contains(&setting) && text.contains("OKAY") && text.contains("Finished");
    if ok {
        Verdict::Ok
    } else {
        Verdict::Failed {
            reason: FailReason::MissingMarker,
        }
    }
}

pub fn parse_update(text: &str, target: Slot, source: Slot) -> Verdict {
    if let Some(verdict) = global_failure(text) {
        return verdict;
    }
    let target_suffix = format!("_{}", target.as_str());
    let source_suffix = format!("_{}", source.as_str());
    for line in lines(text) {
        if let Some(name) = quoted_name(&line) {
            if name.ends_with(source_suffix.as_str()) {
                return Verdict::Failed {
                    reason: FailReason::WrongTarget,
                };
            }
            if name.contains('_')
                && !name.ends_with(target_suffix.as_str())
                && looks_partition(&name)
            {
                return Verdict::Failed {
                    reason: FailReason::WrongTarget,
                };
            }
        }
    }
    if text.contains("Finished") {
        Verdict::Ok
    } else {
        Verdict::Failed {
            reason: FailReason::MissingMarker,
        }
    }
}

pub fn parse_fastboot_reboot(text: &str) -> Verdict {
    if let Some(verdict) = global_failure(text) {
        return verdict;
    }
    if text.contains("Rebooting") && text.contains("OKAY") {
        Verdict::Ok
    } else {
        Verdict::Failed {
            reason: FailReason::MissingMarker,
        }
    }
}

pub fn parse_adb_reboot(text: &str) -> Verdict {
    if let Some(verdict) = global_failure(text) {
        return verdict;
    }
    Verdict::Ok
}

pub fn parse_sideload(text: &str) -> Verdict {
    if waiting_line(text) {
        return Verdict::Failed {
            reason: FailReason::DeviceMissing,
        };
    }
    let total = text.lines().any(|line| total_xfer(line.trim()));
    let soft = sideload_soft_trailer(text);
    if soft && total {
        return Verdict::Uncertain {
            reason: "sideload trailer",
        };
    }
    if let Some(verdict) = global_failure(text) {
        return verdict;
    }
    if sideload_hard_error(text) {
        return Verdict::Failed {
            reason: FailReason::Global,
        };
    }
    if total {
        Verdict::Ok
    } else {
        Verdict::Failed {
            reason: FailReason::NoCompletionMarker,
        }
    }
}

pub fn sideload_post_state(
    current_slot: &str,
    unbootable: &str,
    target: Slot,
    source: Slot,
) -> Verdict {
    if current_slot == source.as_str() {
        return Verdict::Failed {
            reason: FailReason::OtaNotApplied,
        };
    }
    if current_slot == target.as_str() && unbootable == "no" {
        Verdict::Ok
    } else {
        Verdict::Failed {
            reason: FailReason::MissingMarker,
        }
    }
}

pub fn parse_push(text: &str) -> Verdict {
    if let Some(verdict) = global_failure(text) {
        return verdict;
    }
    if text.contains("1 file pushed") {
        Verdict::Ok
    } else {
        Verdict::Failed {
            reason: FailReason::MissingMarker,
        }
    }
}

/// mkdir and rm use fixed paths. Exit 0 is checked by the caller.
/// A global error line still fails the step.
pub fn parse_fixed_shell(text: &str) -> Verdict {
    if let Some(verdict) = global_failure(text) {
        return verdict;
    }
    Verdict::Ok
}

pub fn parse_patch_script(text: &str) -> Verdict {
    if text.lines().any(|line| line.starts_with("! ")) {
        return Verdict::Failed {
            reason: FailReason::Global,
        };
    }
    let outs: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("FL_OUT="))
        .collect();
    let sha1: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("FL_SHA1="))
        .collect();
    let stock: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("FL_STOCK_SHA256="))
        .collect();
    let out_ok = outs.len() == 1 && outs[0] == "FL_OUT=/data/local/tmp/flashwright/out/patched.img";
    let sha_ok = sha1.len() == 1 && is_hex(&sha1[0]["FL_SHA1=".len()..], 40);
    let stock_ok = stock.len() == 1 && is_hex(&stock[0]["FL_STOCK_SHA256=".len()..], 64);
    if out_ok && sha_ok && stock_ok {
        Verdict::Ok
    } else {
        Verdict::Failed {
            reason: FailReason::MissingMarker,
        }
    }
}

fn global_failure(text: &str) -> Option<Verdict> {
    if waiting_line(text) {
        return Some(Verdict::Failed {
            reason: FailReason::DeviceMissing,
        });
    }
    for line in lines(text) {
        if line.starts_with("FAILED")
            || line.starts_with("fastboot: error:")
            || line.starts_with("error:")
            || line.starts_with("adb: error:")
            || line.contains("no devices/emulators found")
            || line.contains("device not found")
            || line.contains("Invalid sparse file format")
            || line.contains("remote: '")
            || line.starts_with("! ")
        {
            return Some(Verdict::Failed {
                reason: FailReason::Global,
            });
        }
    }
    None
}

fn waiting_line(text: &str) -> bool {
    text.contains("< waiting for")
}

fn lines(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(['\n', '\r'])
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
}

fn phase_name(line: &str, kind: &str) -> Option<String> {
    let rest = line.strip_prefix(kind)?;
    let rest = rest.trim_start();
    if rest.starts_with("(sparse)") {
        return quoted_name(rest);
    }
    quoted_name(rest)
}

fn quoted_name(line: &str) -> Option<String> {
    let start = line.find('\'')?;
    let end = line[start + 1..].find('\'')?;
    Some(line[start + 1..start + 1 + end].to_string())
}

fn looks_partition(name: &str) -> bool {
    name.ends_with("_a") || name.ends_with("_b")
}

fn sideload_soft_trailer(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line.contains("failed to read command: No error")
            || line.contains("failed to read command: Success")
    })
}

fn sideload_hard_error(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        if line.contains("failed to read command: No error")
            || line.contains("failed to read command: Success")
        {
            return false;
        }
        line.contains("adb: sideload connection failed")
            || line.contains("adb: failed to")
            || line.contains("failed to read command")
    })
}

fn total_xfer(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("Total xfer: ") else {
        return false;
    };
    let rest = rest.trim_end_matches('x').trim();
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.chars().all(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_requires_the_target_name() {
        let good = "Sending 'init_boot_b' (1024 KB)\nOKAY\nWriting 'init_boot_b'\nOKAY\nFinished. Total time: 0.1s\n";
        assert_eq!(parse_flash(good, Partition::InitBoot, Slot::B), Verdict::Ok);
        let wrong = good.replace("init_boot_b", "init_boot_a");
        assert_eq!(
            parse_flash(&wrong, Partition::InitBoot, Slot::B),
            Verdict::Failed {
                reason: FailReason::WrongTarget
            }
        );
        let lied = "OKAY\nFinished. Total time: 0.1s\n";
        assert!(matches!(
            parse_flash(lied, Partition::Boot, Slot::A),
            Verdict::Failed { .. }
        ));
    }

    #[test]
    fn sideload_needs_total_xfer() {
        assert_eq!(
            parse_sideload("adb: failed to read command: Success\n"),
            Verdict::Failed {
                reason: FailReason::NoCompletionMarker
            }
        );
        assert_eq!(parse_sideload("Total xfer: 1.00x\n"), Verdict::Ok);
        assert_eq!(
            parse_sideload("Total xfer: 1.00x\nadb: failed to read command: No error\n"),
            Verdict::Uncertain {
                reason: "sideload trailer"
            }
        );
        assert_eq!(
            parse_sideload("< waiting for any device >\n"),
            Verdict::Failed {
                reason: FailReason::DeviceMissing
            }
        );
    }

    #[test]
    fn mismatch_when_the_prefix_hash_differs() {
        assert_eq!(
            classify_root(true, true, true, false, true, false),
            RootCheck::FlashMismatch
        );
    }
}
