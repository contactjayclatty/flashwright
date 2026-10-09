// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use serde::Serialize;
use thiserror::Error;

/// Failures the wizard can show. Device I/O is not included in this build.
#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreError {
    #[error("{message}")]
    Message { message: String },
    #[error("{reason}")]
    Rejected { reason: String },
    #[error("device writes are not available in this build")]
    DeviceLayerStub,
}

impl CoreError {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message {
            message: text.into(),
        }
    }
}
