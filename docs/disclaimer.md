# Disclaimer

Flashwright is licensed AGPL-3.0-or-later. Copyright (C) 2026 Jay Clatty (Clatty Works).

Portions derived from PixelFlasher, Copyright badabing2005, AGPL-3.0-or-later, https://github.com/badabing2005/PixelFlasher, commit 081286d.

## Reuse log

M1 reimplements the behaviour listed below in new code. It does not copy PixelFlasher source. Crate licences are listed in `docs/THIRD_PARTY.md`.

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
| payload-dumper-rust | https://github.com/rhythmcache/payload-dumper-rust — path: none copied — commit `ac244d82b54fdd76adf9bf0afcd6edc97f277f7e` (`main`, 2026-09-05) | Apache-2.0 | — | Not used in M1 |
| payload_dumper | https://github.com/vm03/payload_dumper — path: none copied — commit `2f0a964b8b77c6244e3e12735f85539c938f9c97` | No licence | Not used | Not used (no licence) |
| busybox | No copy in this repository. Fetched at runtime from the user's own root app. | GPL-2.0 | Not bundled | Fetched at runtime and not bundled (GPL-2.0) |
| Upstream certificates, icons, and third-party logos | None | — | Not used | No upstream certificates, icons, or third-party logos are reused |
| Tauri, including `tauri` 2.12.2, `tauri-build` 2.7.1, `tauri-plugin-dialog` 2.8.1, `tauri-plugin-opener` 2.7.0, and `@tauri-apps/api` 2.12.2 | https://github.com/tauri-apps/tauri | Apache-2.0 OR MIT | `apps/flashwright-gui` | Used as the window shell. No upstream application code was copied. |
| serde 1.0.229, serde_json 1.0.151, serde_jcs 0.1.0 (https://github.com/l1h3r/serde_jcs), sha2 0.10.9, hex 0.4.3, thiserror 2.0.21, uuid 1.11.0 | crates.io packages of the same names | MIT OR Apache-2.0 | `crates/flashwright-wizard` | Plan hashing uses JCS via `serde_jcs`. The workspace pins uuid 1.11.0 so the 1.88 toolchain can build it. |
| IBM Plex Sans and IBM Plex Mono (OFL-1.1) | https://github.com/IBM/plex commit `763c36ef9117782905ae010056dfbe8fd2653a25`, files under `packages/plex-sans/fonts/complete/woff2` and `packages/plex-mono/fonts/complete/woff2` | SIL Open Font License 1.1 | `apps/flashwright-gui/ui/theme/fonts` | “Plex” is a Reserved Font Name and the family name is unchanged. |
| Vite 6.4.4 and TypeScript | https://github.com/vitejs/vite and https://github.com/microsoft/TypeScript | MIT (Vite), Apache-2.0 (TypeScript) | Wizard bundle tooling | Dev and build tooling only. |
| Flashwright UI skin | Original sheet supplied for this product (`flashwright-ui.css`) | AGPL-3.0-or-later, with the repository | `apps/flashwright-gui/ui/theme` | Original Clatty Works skin. Not a third-party logo. |

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
