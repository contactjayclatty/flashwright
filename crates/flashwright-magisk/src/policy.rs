// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! magiskboot is a user-supplied tool. This file records that it is not bundled.

use serde::Deserialize;

use crate::MagiskError;

const POLICY: &str = include_str!("../../../data/magiskboot.toml");

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct MagiskbootPolicy {
    pub bundled: bool,
}

impl MagiskbootPolicy {
    pub fn embedded() -> Result<Self, MagiskError> {
        toml::from_str(POLICY).map_err(|err| MagiskError::Message(err.to_string()))
    }

    pub fn text() -> &'static str {
        POLICY
    }
}
