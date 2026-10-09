// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use flashwright_proc::ProcError;
use thiserror::Error;

/// Platform-tools discovery and import failures.
#[derive(Debug, Error)]
pub enum ToolsError {
    #[error("platform-tools not found in {directory}")]
    NotFound { directory: String },

    #[error("could not parse platform-tools version: {detail}")]
    Version { detail: String },

    #[error("zip SHA-1 {got} does not match a published platform-tools checksum")]
    ZipChecksum { got: String },

    #[error("zip entry {name} is not a safe relative path")]
    ZipPath { name: String },

    #[error("zip entry {name} exceeds the size limit")]
    ZipTooLarge { name: String },

    #[error("restart adb was not confirmed")]
    RestartNotConfirmed,

    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("platform-tools policy is invalid: {0}")]
    Policy(String),

    #[error(transparent)]
    Process(#[from] ProcError),

    #[error("zip error: {0}")]
    Zip(String),
}

impl ToolsError {
    pub(crate) fn io(path: &std::path::Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}
