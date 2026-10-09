// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use crate::CoreError;

pub fn external_url(id: &str) -> Result<&'static str, CoreError> {
    match id {
        "platform_tools" => Ok("https://developer.android.com/tools/releases/platform-tools"),
        "usb_driver" => Ok("https://developer.android.com/studio/run/win-usb"),
        "firmware_full" => Ok("https://developers.google.com/android/ota"),
        "firmware_factory" => Ok("https://developers.google.com/android/images"),
        _ => Err(CoreError::Rejected {
            reason: "That link is not on the allow list.".to_string(),
        }),
    }
}
