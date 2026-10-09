// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Facts the phone already reported. The image's own patch and build date are
//! read from the extracted boot image, never supplied here.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceFacts {
    pub codename: String,
    pub build_date_utc: Option<u64>,
    pub security_patch: Option<String>,
}
