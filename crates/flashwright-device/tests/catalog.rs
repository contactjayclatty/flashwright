// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use flashwright_device::{AliasTable, DeviceTable, KnownBadMagisk, MinBootloaderTable};

#[test]
fn embedded_tables_parse_and_stay_unseeded() {
    let aliases = AliasTable::embedded().unwrap();
    assert_eq!(aliases.canonical("eos"), "aurora");
    assert_eq!(aliases.canonical("shiba"), "shiba");
    let devices = DeviceTable::embedded().unwrap();
    assert!(devices.get("shiba").unwrap().has_init_boot);
    assert!(!devices.get("oriole").unwrap().has_init_boot);
    assert!(KnownBadMagisk::embedded().unwrap().version_codes.is_empty());
    assert!(MinBootloaderTable::embedded().unwrap().entries.is_empty());
}
