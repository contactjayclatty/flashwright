// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use flashwright_device::{AliasTable, DeviceTable, KnownBadMagisk, MinBootloaderTable};

#[test]
fn embedded_tables_parse() {
    let aliases = AliasTable::embedded().unwrap();
    assert_eq!(aliases.canonical("eos"), "aurora");
    assert_eq!(aliases.canonical("shiba"), "shiba");
    let devices = DeviceTable::embedded().unwrap();
    assert!(devices.get("shiba").unwrap().has_init_boot);
    assert!(!devices.get("oriole").unwrap().has_init_boot);
    let komodo = devices.get("komodo").unwrap();
    assert_eq!(komodo.model.as_deref(), Some("Pixel 9 Pro XL"));
    assert!(komodo.has_init_boot);
    let magisk = KnownBadMagisk::embedded().unwrap();
    assert!(magisk.version_codes.contains(&25207));
    let bootloaders = MinBootloaderTable::embedded().unwrap();
    assert!(bootloaders
        .entries
        .iter()
        .any(|entry| entry.codename == "oriole" && entry.min == "15.3-13239612"));
}
