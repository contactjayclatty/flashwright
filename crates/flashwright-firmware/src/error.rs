// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use thiserror::Error;

/// A package was refused or could not be opened. Messages name the gate and
/// do not include checksums, build ids, or fingerprints.
#[derive(Debug, Error)]
pub enum FirmwareError {
    #[error("only a .zip package is accepted")]
    NotZip,

    #[error("tar and tgz packages are not accepted")]
    UnsupportedArchive,

    #[error("G04: the firmware codename does not match the device")]
    CodenameMismatch,

    #[error("G05: the package SHA-256 does not match the published value")]
    PublishedHash,

    #[error("G05: the filename checksum fragment does not match the package")]
    FilenameFragment,

    #[error("G06: an incremental OTA is not accepted")]
    IncrementalOta,

    #[error("G06: the package is not a full A/B OTA")]
    NotFullOta,

    #[error("G06: a factory package needs exactly one image zip")]
    FactoryLayout,

    #[error("G07: the firmware is older than the device")]
    Downgrade,

    #[error("G08: the image security patch does not match the package")]
    SecurityPatchMismatch,

    #[error("G08: the image fingerprint does not match the package")]
    FingerprintMismatch,

    #[error("G12: not enough free space on the working volume")]
    NoSpace,

    #[error("LU0 / FIPS region is not supported")]
    RestrictedRegion,

    #[error("the package has no init_boot or boot image")]
    NoBootImage,

    #[error("the device and the package do not agree on init_boot")]
    PartitionMismatch,

    #[error("the extracted image does not match the payload metadata hash")]
    PartitionHash,

    #[error("the extracted image is not a boot image with an AVB footer")]
    BootImage,

    #[error("payload.bin must be stored uncompressed inside the package")]
    CompressedPayload,

    #[error("the package is truncated or is not a valid zip")]
    Truncated,

    #[error("the package could not be read")]
    Io(#[source] std::io::Error),

    #[error("CRC-32 check failed")]
    Crc,

    #[error("the package could not be opened")]
    Archive(String),
}

impl FirmwareError {
    pub(crate) fn io(err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::UnexpectedEof {
            Self::Truncated
        } else {
            Self::Io(err)
        }
    }
}
