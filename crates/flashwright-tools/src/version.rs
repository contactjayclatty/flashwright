// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::fmt;

use crate::ToolsError;

/// A platform-tools release, such as `37.0.1` or `36.0.2-14143358`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdkVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub build: Option<String>,
}

impl SdkVersion {
    pub fn parse(text: &str) -> Result<Self, ToolsError> {
        let text = text.trim();
        let (triple, build) = match text.split_once('-') {
            Some((triple, build)) => (triple, Some(build.to_string())),
            None => (text, None),
        };
        let mut parts = triple.split('.');
        let major = take_number(parts.next())?;
        let minor = take_number(parts.next())?;
        let patch = take_number(parts.next())?;
        if parts.next().is_some() {
            return Err(ToolsError::Version {
                detail: format!("too many components in {text}"),
            });
        }
        Ok(Self {
            major,
            minor,
            patch,
            build,
        })
    }

    pub fn triple(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

impl fmt::Display for SdkVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(build) = &self.build {
            write!(formatter, "-{build}")?;
        }
        Ok(())
    }
}

/// Read the SDK version from `adb version` output.
pub fn parse_adb_version_output(text: &str) -> Result<SdkVersion, ToolsError> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Version ") {
            let token = rest.split_whitespace().next().unwrap_or(rest);
            return SdkVersion::parse(token);
        }
    }
    SdkVersion::parse(text.trim()).map_err(|_| ToolsError::Version {
        detail: "adb version output has no Version line".into(),
    })
}

/// Read the SDK version from `fastboot --version` output.
pub fn parse_fastboot_version_output(text: &str) -> Result<SdkVersion, ToolsError> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("fastboot version ") {
            let token = rest.split_whitespace().next().unwrap_or(rest);
            return SdkVersion::parse(token);
        }
    }
    Err(ToolsError::Version {
        detail: "fastboot version output has no version line".into(),
    })
}

fn take_number(part: Option<&str>) -> Result<u64, ToolsError> {
    let Some(part) = part else {
        return Err(ToolsError::Version {
            detail: "expected major.minor.patch".into(),
        });
    };
    part.parse().map_err(|_| ToolsError::Version {
        detail: format!("not a number: {part}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adb_banner_and_build_id() {
        let text = "Android Debug Bridge version 1.0.41\nVersion 36.0.2-14143358\nInstalled as C:\\platform-tools\\adb.exe\n";
        let version = parse_adb_version_output(text).unwrap();
        assert_eq!(version.triple(), (36, 0, 2));
        assert_eq!(version.build.as_deref(), Some("14143358"));
        assert_eq!(version.to_string(), "36.0.2-14143358");
    }
}
