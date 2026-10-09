// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Argument newtypes. `TryFrom` rejects; it never rewrites the input.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::device::{Partition, Slot};

pub const MAGISK_PACKAGE: &str = "com.topjohnwu.magisk";
const MAX_BLOCK_LEN: u64 = 512 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CmdError {
    #[error("{field}: {issue}")]
    Rejected {
        field: &'static str,
        issue: &'static str,
    },
    #[error("{partition} is read-only in this phase")]
    ReadOnlyPartition { partition: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSerial(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropName(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageName(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteLen(u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedDevicePath(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ByNameRoot {
    ByName,
    BootdeviceByName,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DumpsysService {
    Battery,
    Diskstats,
    Package(PackageName),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FastbootVar {
    CurrentSlot,
    Unlocked,
    Product,
    VersionBootloader,
    SlotUnbootable(Slot),
    SlotSuccessful(Slot),
    SlotRetryCount(Slot),
    MaxDownloadSize,
    PartitionSize { partition: Partition, slot: Slot },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkFile {
    Stock,
    Patched,
    PatchScript,
    BootPatch,
    UtilFunctions,
    AppFunctions,
    StubApk,
    Busybox,
    Magiskboot,
    Magiskinit,
    Magisk,
    InitLd,
}

/// Identity of a core-held host file. Callers pass this id, not a path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    id: u64,
    path: String,
    size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRef {
    name: String,
    path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostRef {
    Image(ImageRef),
    Asset(AssetRef),
}

impl DeviceSerial {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn require_in_scan<'a>(
        &'a self,
        serials: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), CmdError> {
        if serials.into_iter().any(|serial| serial == self.0) {
            Ok(())
        } else {
            Err(CmdError::Rejected {
                field: "serial",
                issue: "not in the current scan",
            })
        }
    }
}

impl PropName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PackageName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn magisk_app() -> Self {
        Self(MAGISK_PACKAGE.to_string())
    }
}

impl ByteLen {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl ValidatedDevicePath {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `codePath` from dumpsys, validated, then `/base.apk` appended.
    pub fn from_code_path(code_path: &str) -> Result<Self, CmdError> {
        if !code_path_ok(code_path) {
            return Err(CmdError::Rejected {
                field: "codePath",
                issue: "does not match the Magisk APK path pattern",
            });
        }
        Ok(Self(format!("{code_path}/base.apk")))
    }
}

impl ByNameRoot {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ByName => "/dev/block/by-name",
            Self::BootdeviceByName => "/dev/block/bootdevice/by-name",
        }
    }
}

impl FastbootVar {
    pub fn as_str(&self) -> String {
        match self {
            Self::CurrentSlot => "current-slot".into(),
            Self::Unlocked => "unlocked".into(),
            Self::Product => "product".into(),
            Self::VersionBootloader => "version-bootloader".into(),
            Self::SlotUnbootable(slot) => format!("slot-unbootable:{}", slot.as_str()),
            Self::SlotSuccessful(slot) => format!("slot-successful:{}", slot.as_str()),
            Self::SlotRetryCount(slot) => format!("slot-retry-count:{}", slot.as_str()),
            Self::MaxDownloadSize => "max-download-size".into(),
            Self::PartitionSize { partition, slot } => {
                format!(
                    "partition-size:{}{}",
                    partition.fastboot_name(),
                    slot.suffix()
                )
            }
        }
    }
}

impl WorkFile {
    pub fn device_path(self) -> &'static str {
        match self {
            Self::Stock => "/data/local/tmp/flashwright/stock.img",
            Self::Patched => "/data/local/tmp/flashwright/out/patched.img",
            Self::PatchScript => "/data/local/tmp/flashwright/fl_patch.sh",
            Self::BootPatch => "/data/local/tmp/flashwright/boot_patch.sh",
            Self::UtilFunctions => "/data/local/tmp/flashwright/util_functions.sh",
            Self::AppFunctions => "/data/local/tmp/flashwright/app_functions.sh",
            Self::StubApk => "/data/local/tmp/flashwright/stub.apk",
            Self::Busybox => "/data/local/tmp/flashwright/libbusybox.so",
            Self::Magiskboot => "/data/local/tmp/flashwright/libmagiskboot.so",
            Self::Magiskinit => "/data/local/tmp/flashwright/libmagiskinit.so",
            Self::Magisk => "/data/local/tmp/flashwright/libmagisk.so",
            Self::InitLd => "/data/local/tmp/flashwright/libinit-ld.so",
        }
    }
}

impl ImageRef {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(id: u64, path: impl Into<String>, size_bytes: u64) -> Self {
        Self {
            id,
            path: path.into(),
            size_bytes,
        }
    }

    /// A host image the plan may name. The path is one argv element.
    pub fn for_plan(id: u64, path: impl Into<String>, size_bytes: u64) -> Result<Self, CmdError> {
        let path = path.into();
        if !host_path_ok(&path) {
            return Err(CmdError::Rejected {
                field: "image",
                issue: "rejected",
            });
        }
        Ok(Self {
            id,
            path,
            size_bytes,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

impl AssetRef {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }

    /// A host file the plan may push. The path is one argv element.
    pub fn for_plan(name: impl Into<String>, path: impl Into<String>) -> Result<Self, CmdError> {
        let path = path.into();
        if !host_path_ok(&path) {
            return Err(CmdError::Rejected {
                field: "asset",
                issue: "rejected",
            });
        }
        Ok(Self {
            name: name.into(),
            path,
        })
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }
}

impl HostRef {
    pub(crate) fn path(&self) -> &str {
        match self {
            Self::Image(image) => image.path(),
            Self::Asset(asset) => asset.path(),
        }
    }
}

impl TryFrom<&str> for DeviceSerial {
    type Error = CmdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if !matches_serial(value) {
            return Err(reject("serial"));
        }
        Ok(Self(value.to_string()))
    }
}

impl TryFrom<&str> for PropName {
    type Error = CmdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if !matches_prop(value) {
            return Err(reject("prop"));
        }
        Ok(Self(value.to_string()))
    }
}

impl TryFrom<&str> for PackageName {
    type Error = CmdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if !matches_package(value) {
            return Err(reject("package"));
        }
        Ok(Self(value.to_string()))
    }
}

impl TryFrom<&str> for ByteLen {
    type Error = CmdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(reject("length"));
        }
        let parsed: u64 = value.parse().map_err(|_| reject("length"))?;
        Self::try_from(parsed)
    }
}

impl TryFrom<u64> for ByteLen {
    type Error = CmdError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if (1..=MAX_BLOCK_LEN).contains(&value) {
            Ok(Self(value))
        } else {
            Err(reject("length"))
        }
    }
}

impl fmt::Display for DeviceSerial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn reject(field: &'static str) -> CmdError {
    CmdError::Rejected {
        field,
        issue: "rejected",
    }
}

fn forbidden_text(value: &str) -> bool {
    if value.is_empty() || value.starts_with('-') || value.contains("..") {
        return true;
    }
    value.chars().any(|ch| {
        matches!(
            ch,
            '\'' | '"'
                | ';'
                | '&'
                | '|'
                | '$'
                | '`'
                | '('
                | ')'
                | '<'
                | '>'
                | '*'
                | '?'
                | '\n'
                | '\r'
                | '\t'
                | ' '
                | '\0'
        ) || !ch.is_ascii()
    })
}

fn matches_serial(value: &str) -> bool {
    let len = value.chars().count();
    (1..=64).contains(&len)
        && !forbidden_text(value)
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-'))
}

fn matches_prop(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() || forbidden_text(value) {
        return false;
    }
    let rest = chars.count();
    rest <= 95
        && value
            .chars()
            .skip(1)
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
}

fn matches_package(value: &str) -> bool {
    if forbidden_text(value) {
        return false;
    }
    let labels: Vec<&str> = value.split('.').collect();
    if labels.len() < 2 || labels.len() > 8 {
        return false;
    }
    labels.iter().all(|label| package_label(label))
}

fn package_label(label: &str) -> bool {
    let mut chars = label.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && label.chars().count() <= 32
        && label
            .chars()
            .skip(1)
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn host_path_ok(path: &str) -> bool {
    if path.is_empty() || path.contains('\0') || path.contains('\n') || path.contains('\r') {
        return false;
    }
    if path.starts_with(r"\\") || path.starts_with("//") {
        return false;
    }
    Path::new(path).is_absolute()
}

fn code_path_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("/data/app/") else {
        return false;
    };
    if forbidden_text(rest) || rest.contains("//") {
        return false;
    }
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() > 2 || parts.is_empty() {
        return false;
    }
    parts.iter().all(|part| code_segment(part))
}

fn code_segment(part: &str) -> bool {
    let len = part.chars().count();
    (1..=128).contains(&len)
        && part
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '~' | '=' | '+' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magisk_package_is_the_only_phase1_constant() {
        let package = PackageName::try_from(MAGISK_PACKAGE).unwrap();
        assert_eq!(package.as_str(), PackageName::magisk_app().as_str());
        assert!(PackageName::try_from("com.topjohnwu.magisk;reboot").is_err());
    }

    #[test]
    fn image_and_asset_refs_keep_the_caller_path() {
        let image = ImageRef::new(7, "/var/flashwright/boot.img", 4096);
        assert_eq!(image.id(), 7);
        assert_eq!(image.size_bytes(), 4096);
        let asset = AssetRef::new("stub.apk", "/var/flashwright/stub.apk");
        assert_eq!(asset.path(), "/var/flashwright/stub.apk");
    }

    #[test]
    fn code_path_appends_base_apk_or_rejects() {
        let path =
            ValidatedDevicePath::from_code_path("/data/app/~~abc==/com.topjohnwu.magisk-xyz")
                .unwrap();
        assert!(path.as_str().ends_with("/base.apk"));
        assert!(ValidatedDevicePath::from_code_path("/data/app/foo; reboot").is_err());
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10_000))]

        #[test]
        fn serial_and_prop_never_rewrite_input(raw in "\\PC{0,80}") {
            if let Ok(serial) = DeviceSerial::try_from(raw.as_str()) {
                assert_eq!(serial.as_str(), raw);
            }
            if let Ok(prop) = PropName::try_from(raw.as_str()) {
                assert_eq!(prop.as_str(), raw);
            }
        }
    }
}
