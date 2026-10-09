// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BootError {
    #[error("the boot image is not a v3 or v4 init_boot")]
    Header,

    #[error("unsupported ramdisk format")]
    UnsupportedRamdisk,

    #[error("patched init_boot has no Magisk init")]
    NoMagiskInit,

    #[error("patched init_boot is missing the stock SHA-1")]
    MissingSha1,

    #[error("the patched image does not match the stock image")]
    StockMismatch,

    #[error("the image is larger than the parser allows")]
    TooLarge,

    #[error("the plan binding is not valid")]
    Binding,
}
