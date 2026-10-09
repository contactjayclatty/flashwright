# PixelFlasher reuse

Flashwright is AGPL-3.0-or-later. PixelFlasher is Copyright badabing2005, AGPL-3.0-or-later, commit `081286d` (`081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54`).

No PixelFlasher `.py` file is copied into this tree. No file from PixelFlasher's `bin/` directory is copied or executed. The rows below are behaviour rewritten in Rust. The AGPL-3.0-or-later notice for each item is kept here and in `docs/disclaimer.md`.

| Item | Upstream | Notice | Where it lives |
| --- | --- | --- | --- |
| App-method patch script | `pf_modules.py` `patch_magisk_script` lines 2698–2939 | AGPL-3.0-or-later | `crates/flashwright-magisk/src/fl_patch.sh` |
| App versus rooted method | `pf_modules.py` `patch_boot_img` lines 4783–4906 | AGPL-3.0-or-later | `crates/flashwright-magisk` plan |
| Hidden Magisk app | `pf_modules.py` `magisk_not_found` lines 3687–3749 | AGPL-3.0-or-later | `crates/flashwright-magisk` detection |
| Component extract | `runtime.py` `extract_magiskboot` lines 8702–8732 | AGPL-3.0-or-later | `crates/flashwright-magisk/src/extract.rs` |
| Stock SHA-1 | `runtime.py` `sha1` lines 3198–3211 | AGPL-3.0-or-later | host file hash in the magisk plan |
| Patched image check | `pf_modules.py` lines 5020–5119 | AGPL-3.0-or-later | `crates/flashwright-bootimg` |
| Device and slot tables | `android_devices.json` and the safety tables at the same commit | AGPL-3.0-or-later | `data/` and `flashwright-core` |

`extract_sha1`, `compare_sha1`, and `drive_magisk` are not ported. Magisk is GPL-3.0 and is not shipped.
