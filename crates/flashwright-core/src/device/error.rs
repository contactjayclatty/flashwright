// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use crate::proc::ProcError;
use thiserror::Error;

/// Device-layer failures. Messages name the condition the caller can act on.
#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("active slot is unknown; refusing to assume slot a")]
    UnknownSlot,

    #[error("{partition} is read-only in this phase")]
    ReadOnlyPartition { partition: String },

    #[error("device {serial} is not connected")]
    NotConnected { serial: String },

    #[error("transition from {from} to {to} is not in the Phase 1 table")]
    UnsupportedTransition { from: String, to: String },

    #[error("{command} failed (exit {exit:?}): {detail}")]
    CommandFailed {
        command: String,
        exit: Option<i32>,
        detail: String,
    },

    #[error("{message}")]
    NeedsUser { message: String },

    #[error("device catalogue is invalid: {0}")]
    Catalogue(String),

    #[error(transparent)]
    Process(#[from] ProcError),

    #[error("{0}")]
    Message(String),
}
