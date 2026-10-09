# Disclaimer

Flashwright is licensed AGPL-3.0-or-later. Copyright (C) 2026 Jay Clatty (Clatty Works).

Portions derived from PixelFlasher, Copyright badabing2005, AGPL-3.0-or-later, https://github.com/badabing2005/PixelFlasher, commit 081286d.

## Reuse log

The rows below are rewritten behaviour, not copied PixelFlasher source. Crate licences are listed in `docs/THIRD_PARTY.md`.

| Item | Upstream source (URL, path, commit) | Licence | Where used | Notes |
| --- | --- | --- | --- | --- |
| PixelFlasher behaviour | https://github.com/badabing2005/PixelFlasher — commit `081286d` (`081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54`), tag v10.1.1.1 | AGPL-3.0-or-later | `flashwright-tools`, `flashwright-device` | Rewritten behaviour, not copied code. Source headers at this commit declare `AGPL-3.0-or-later`. The repository `LICENSE` file at this commit is the GNU GPL-3.0 text. |
| Platform-tools check | `pf_modules.py` lines 98–144, same commit | AGPL-3.0-or-later | `flashwright-tools` | Allow, block, and scan-only classification. Rewritten. |
| SDK version parse | `pf_modules.py` lines 395–472, same commit | AGPL-3.0-or-later | `flashwright-tools` | `adb version` banner. Rewritten. |
| Connected devices | `phone.py` lines 5604–5749, same commit | AGPL-3.0-or-later | `flashwright-device` | `adb devices -l` and `fastboot devices -l`. Rewritten. |
| Device state | `phone.py` lines 4305–4371, same commit | AGPL-3.0-or-later | `flashwright-device` | Mode tokens. Rewritten. |
| Props | `phone.py` lines 392–515, same commit | AGPL-3.0-or-later | `flashwright-device` | `getprop` without `su`. Rewritten. |
| Slot and lock | `phone.py` lines 541–800 and `get_bl_status` lines 1222–1234, same commit | AGPL-3.0-or-later | `flashwright-device` | Unknown slot is an error. Unlock is a string compare. Rewritten. |
| Root and Magisk | `phone.py` lines 3706–3728, 936–966, and 2977–2999, same commit | AGPL-3.0-or-later | `flashwright-device` | `su -c id`, `magisk -v`/`-V`, dumpsys package. Rewritten. |
| Reboot and wait | `phone.py` lines 4101–4300, 4448–4483, and 4413, same commit | AGPL-3.0-or-later | `flashwright-device` | §3.5 waits. A missing poll is not an unplug. Rewritten. |
| Command catalogue and write confirm | Behaviour only, same commit | AGPL-3.0-or-later | `flashwright-core` | Typed argv, timeouts, and parsers. Rewritten. The confirm path mints its token inside the core crate. |
| Device alias table | Upstream issue 325, marked unverified | — | `data/device_aliases.toml` | `eos` maps to `aurora`. |
| Package selection | `pf_modules.py` `select_firmware` lines 598–664, same commit | AGPL-3.0-or-later | `flashwright-firmware` | Zip only. Package SHA-256, filename fragment, and codename token. Rewritten. |
| Factory and OTA open | `pf_modules.py` `process_file` from line 670, payload branch around 812, extract 971–1147, same commit | AGPL-3.0-or-later | `flashwright-firmware` | Factory image zip and full OTA `payload.bin`. Prefer `init_boot` over `boot`. Rewritten. Not ported: 7-Zip, Samsung/Odin, custom ROM, database cache, extra images, or the Python payload extractor. |
| Chunked SHA-256 | `runtime.py` lines 3217–3228, same commit | AGPL-3.0-or-later | `flashwright-firmware` | Chunked package hash. Rewritten. This tree uses a 1 MiB buffer. |
| init_boot vs boot | `runtime.py` `has_init_boot` lines 1140–1147, same commit | AGPL-3.0-or-later | `flashwright-firmware` | The firmware crate reads `data/devices.toml` when choosing `init_boot` or `boot`. The safety gate records the same lines separately. |
| payload-dumper-rust | https://github.com/rhythmcache/payload-dumper-rust commit `ac244d82b54fdd76adf9bf0afcd6edc97f277f7e`. Crate `payload_dumper` 0.8.4. Licence file `third_party/payload-dumper-rust/LICENSE`. No NOTICE file in that repository. | Apache-2.0 | `flashwright-firmware` | Used to read a stored `payload.bin` and extract one partition. Features: `local_zip` only. |
| payload_dumper | https://github.com/vm03/payload_dumper — path: none copied — commit `2f0a964b8b77c6244e3e12735f85539c938f9c97` | No licence | Not used | Not used (no licence) |
| AOSP update metadata | https://android.googlesource.com/platform/system/update_engine path `update_metadata.proto` commit `dc84c2552b2d4cf00d2a843cb1c091d99d0499f1`. SHA-256 `09da1556e3edb9197ca88103b22ea07230a634c004605d4aa1efee6a6ed6e60d`. Copy: `third_party/aosp/update_engine/`. | Apache-2.0 | `flashwright-firmware` | Bindings are generated at build time. The payload partition hash is read from this copy. |
| AOSP AVB footer | https://android.googlesource.com/platform/external/avb commit `761178607206f4cb2af79ed9eec52d8cbd814adb`. Notice: `third_party/aosp/avb/NOTICE`. | MIT | `flashwright-firmware` | Footer, vbmeta header, and property descriptors, matching `avbtool info_image`. No avbtool source is copied. Signature checks are not done. |
| busybox | No copy in this repository. Fetched at runtime from the user's own root app. | GPL-2.0 | Not bundled | Fetched at runtime and not bundled (GPL-2.0) |
| Upstream certificates, icons, and third-party logos | None | — | Not used | No upstream certificates, icons, or third-party logos are reused |
| Tauri, including `tauri` 2.12.2, `tauri-build` 2.7.1, `tauri-plugin-dialog` 2.8.1, `tauri-plugin-opener` 2.7.0, and `@tauri-apps/api` 2.12.2 | https://github.com/tauri-apps/tauri | Apache-2.0 OR MIT | `apps/flashwright-gui` | Used as the window shell. No upstream application code was copied. |
| serde 1.0.229, serde_json 1.0.151, serde_jcs 0.1.0 (https://github.com/l1h3r/serde_jcs), sha2 0.10.9, hex 0.4.3, thiserror 2.0.21, uuid 1.11.0 | crates.io packages of the same names | MIT OR Apache-2.0 | `crates/flashwright-core` | Plan hashing uses JCS via `serde_jcs`. The workspace pins uuid 1.11.0 so the 1.88 toolchain can build it. |
| IBM Plex Sans and IBM Plex Mono (OFL-1.1) | https://github.com/IBM/plex commit `763c36ef9117782905ae010056dfbe8fd2653a25`, files under `packages/plex-sans/fonts/complete/woff2` and `packages/plex-mono/fonts/complete/woff2` | SIL Open Font License 1.1 | `apps/flashwright-gui/ui/theme/fonts` | “Plex” is a Reserved Font Name and the family name is unchanged. |
| Vite 6.4.4 and TypeScript | https://github.com/vitejs/vite and https://github.com/microsoft/TypeScript | MIT (Vite), Apache-2.0 (TypeScript) | Wizard bundle tooling | Dev and build tooling only. |
| Flashwright UI skin | Original sheet supplied for this product (`flashwright-ui.css`) | AGPL-3.0-or-later, with the repository | `apps/flashwright-gui/ui/theme` and `assets/brand/flashwright` | Clatty Works brand assets (in-house) |

### Brand asset files

Clatty Works brand assets (in-house). Each file under `assets/brand/flashwright`:

- `assets/brand/flashwright/app-icon/app-icon-16.png`
- `assets/brand/flashwright/app-icon/app-icon-16.svg`
- `assets/brand/flashwright/app-icon/app-icon-24.svg`
- `assets/brand/flashwright/app-icon/app-icon-256.png`
- `assets/brand/flashwright/app-icon/app-icon-32.png`
- `assets/brand/flashwright/app-icon/app-icon-48.png`
- `assets/brand/flashwright/app-icon/app-icon-512.png`
- `assets/brand/flashwright/app-icon/app-icon.ico`
- `assets/brand/flashwright/app-icon/app-icon.svg`
- `assets/brand/flashwright/banner/wizard-banner@2x.png`
- `assets/brand/flashwright/banner/wizard-banner.png`
- `assets/brand/flashwright/banner/wizard-banner.svg`
- `assets/brand/flashwright/flashwright-assets-sheet.png`
- `assets/brand/flashwright/flashwright-ui.css`
- `assets/brand/flashwright/icons/backup-16.png`
- `assets/brand/flashwright/icons/backup-16.svg`
- `assets/brand/flashwright/icons/backup-32.png`
- `assets/brand/flashwright/icons/backup-32.svg`
- `assets/brand/flashwright/icons/download-16.png`
- `assets/brand/flashwright/icons/download-16.svg`
- `assets/brand/flashwright/icons/download-32.png`
- `assets/brand/flashwright/icons/download-32.svg`
- `assets/brand/flashwright/icons/flash-16.png`
- `assets/brand/flashwright/icons/flash-16.svg`
- `assets/brand/flashwright/icons/flash-32.png`
- `assets/brand/flashwright/icons/flash-32.svg`
- `assets/brand/flashwright/icons/help-16.png`
- `assets/brand/flashwright/icons/help-16.svg`
- `assets/brand/flashwright/icons/help-32.png`
- `assets/brand/flashwright/icons/help-32.svg`
- `assets/brand/flashwright/icons/log-16.png`
- `assets/brand/flashwright/icons/log-16.svg`
- `assets/brand/flashwright/icons/log-32.png`
- `assets/brand/flashwright/icons/log-32.svg`
- `assets/brand/flashwright/icons/phone-connect-16.png`
- `assets/brand/flashwright/icons/phone-connect-16.svg`
- `assets/brand/flashwright/icons/phone-connect-32.png`
- `assets/brand/flashwright/icons/phone-connect-32.svg`
- `assets/brand/flashwright/icons/root-16.png`
- `assets/brand/flashwright/icons/root-16.svg`
- `assets/brand/flashwright/icons/root-32.png`
- `assets/brand/flashwright/icons/root-32.svg`
- `assets/brand/flashwright/icons/settings-16.png`
- `assets/brand/flashwright/icons/settings-16.svg`
- `assets/brand/flashwright/icons/settings-32.png`
- `assets/brand/flashwright/icons/settings-32.svg`
- `assets/brand/flashwright/icons/success-16.png`
- `assets/brand/flashwright/icons/success-16.svg`
- `assets/brand/flashwright/icons/success-32.png`
- `assets/brand/flashwright/icons/success-32.svg`
- `assets/brand/flashwright/icons/warning-16.png`
- `assets/brand/flashwright/icons/warning-16.svg`
- `assets/brand/flashwright/icons/warning-32.png`
- `assets/brand/flashwright/icons/warning-32.svg`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal-1600.png`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal-800.png`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal-on-deep-teal-1600.png`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal-on-deep-teal-800.png`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal-on-deep-teal.svg`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal-on-white-1600.png`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal-on-white-800.png`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal-on-white.svg`
- `assets/brand/flashwright/lockup/flashwright-lockup-horizontal.svg`
- `assets/brand/flashwright/src/build.py`
- `assets/brand/flashwright/src/lockup.py`
- `assets/brand/flashwright/src/pixels.py`

## Safety tables

These rows are the PixelFlasher tables and checks the safety gates use. The licence is AGPL-3.0-or-later. The commit is `081286d` (`081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54`), tag v10.1.1.1, https://github.com/badabing2005/PixelFlasher. An unreadable bootloader version on a listed phone is refused. The LU0 and FIPS regions are a Flashwright block; they are not a PixelFlasher table.

| Item | Upstream source (URL, path, lines, commit) | Licence | Where used | Notes |
| --- | --- | --- | --- | --- |
| Device compatibility | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/android_devices.json — entire file — `081286d` | AGPL-3.0-or-later | `data/device_compatibility.toml`, `flashwright-core` | Codename, model, support dates, first API level, bootloader codename, init_boot, and watch flag. Every row is treated as A/B. |
| init_boot lookup | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/runtime.py — lines 1140–1147 — `081286d` | AGPL-3.0-or-later | `flashwright-core` safety gate G04 | `has_init_boot`. |
| Build security patch | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/runtime.py — lines 11791–11856 — `081286d` | AGPL-3.0-or-later | `flashwright-core` safety gates G07 and G08 | Date taken from the build id. A mismatch is a block. |
| Model match | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/pf_modules.py — lines 4224–4258 and 5274–5303 — `081286d` | AGPL-3.0-or-later | `flashwright-core` safety gate G04 | Phone, firmware codename, and file name must agree. |
| Known-bad Magisk | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/constants.py — line 49 — `081286d` | AGPL-3.0-or-later | `data/known_bad_magisk.toml` | `label:version_code` pairs. A match is a block. |
| Banned kernels | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/constants.py — lines 74–101 — `081286d` | AGPL-3.0-or-later | `data/banned_kernels.toml` | The upstream list is missing a comma after `-mokee`, so those two names concatenate. Both names are kept, and the concatenated token is kept. |
| Unofficial Magisk | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/constants.py — lines 59–60 — `081286d` | AGPL-3.0-or-later | `data/off_limits.toml` | Alpha and Delta application ids. |
| Minimum bootloader | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/constants.py — lines 135–146 — `081286d` | AGPL-3.0-or-later | `data/min_bootloader.toml` | Per-slot minimum. Older than the minimum is a block. |
| Bootloader compare | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/runtime.py — lines 11536–11563 and 11698–11724 — `081286d` | AGPL-3.0-or-later | `flashwright-core` safety gates G18 and G19 | `major.minor-patch` order. A missing or unreadable version on a listed phone is a block. |
| Tensor anti-rollback | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/pf_modules.py — lines 6322–6367 — `081286d` | AGPL-3.0-or-later | `data/tensor_arb.toml` | raven, oriole, and bluejay below API 33. A bootloader write is a block. |
| Slot rules | https://github.com/badabing2005/PixelFlasher/blob/081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54/pf_modules.py — lines 6283–6293 and 6422 — `081286d` | AGPL-3.0-or-later | `data/slot_rules.toml` | Both slots, and `--slot all`, are a block. A write names the inactive slot. |
| LU0 and FIPS | Not from PixelFlasher | — | `data/off_limits.toml` | A path segment `lu0` or `fips` is a block. Wiping data, turning verification off, erasing a partition, writing vbmeta, and starting a host shell are also refused. |

## Trademark notice

Pixel and Android are trademarks of Google LLC, used descriptively only.

## Bricking and warranty

Flashing can void warranties or brick devices. Use at your own risk.

## Private builds

Builds stay private until the owner approves a release.
