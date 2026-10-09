// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! OTA `META-INF/com/android/metadata` and factory `android-info.txt`.

use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageMeta {
    pub ota_type: Option<String>,
    pub pre_device: Option<String>,
    pub pre_build: Option<String>,
    pub post_build: Option<String>,
    pub post_security_patch: Option<String>,
    pub post_timestamp: Option<u64>,
    pub board: Option<String>,
    pub raw: String,
}

pub fn parse_metadata(text: &str) -> PackageMeta {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    let board = map
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("require board"))
        .map(|(_, value)| value.clone())
        .or_else(|| map.get("board").cloned());
    PackageMeta {
        ota_type: map.get("ota-type").cloned(),
        pre_device: map.get("pre-device").cloned(),
        pre_build: map.get("pre-build").cloned(),
        post_build: map.get("post-build").cloned(),
        post_security_patch: map
            .get("post-security-patch-level")
            .or_else(|| map.get("security-patch-level"))
            .cloned(),
        post_timestamp: map.get("post-timestamp").and_then(|value| value.parse().ok()),
        board,
        raw: text.to_string(),
    }
}

pub fn board_names(board: &str) -> Vec<String> {
    board
        .split(['|', ','])
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}
