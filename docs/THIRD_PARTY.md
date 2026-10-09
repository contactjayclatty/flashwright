# Third-party material

Flashwright is AGPL-3.0-or-later. See `LICENSE`.

Portions derived from PixelFlasher, Copyright badabing2005, AGPL-3.0-or-later, https://github.com/badabing2005/PixelFlasher, commit 081286d (`081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54`).

The safety gates use PixelFlasher's device table, Magisk list, kernel list, bootloader minimums, and slot checks from that commit, translated into the files under `data/` and checked in `flashwright-core`. Each file, line range, and commit is in `docs/disclaimer.md`. The LU0 and FIPS block is Flashwright policy, not a PixelFlasher table.

PixelFlasher source is not copied. The behaviour rows, and the items that are not used, are in `docs/disclaimer.md`.

Factory and full OTA packages are opened with `payload_dumper` 0.8.4 (`local_zip` only). Its Apache-2.0 licence is `third_party/payload-dumper-rust/LICENSE`. That repository has no NOTICE file. The comparison proto under `third_party/payload-dumper-rust/proto/` is not compiled. After comments and whitespace are removed, it matches the AOSP proto except for `InstallOperation.Type` value `ZSTD = 14`.

The compiled proto is `third_party/aosp/update_engine/update_metadata.proto` from `platform/system/update_engine` commit `dc84c2552b2d4cf00d2a843cb1c091d99d0499f1` (SHA-256 `09da1556e3edb9197ca88103b22ea07230a634c004605d4aa1efee6a6ed6e60d`). The boot-image footer reader follows AOSP `external/avb` commit `761178607206f4cb2af79ed9eec52d8cbd814adb` (MIT, `third_party/aosp/avb/NOTICE`). The unlicensed Python `payload_dumper` is not used.

The Magisk app patch follows the same PixelFlasher commit. Each reused item is logged in `docs/disclaimer.md`: `pf_modules.py` `patch_magisk_script` (2698–2939), the app-method choice in `patch_boot_img` (4783–4906), `magisk_not_found` (3687–3715), `runtime.py` `extract_magiskboot` (8702–8732), `runtime.py` `sha1` (3198–3211), and the post-patch check in `pf_modules.py` (5020–5119). `extract_sha1`, `compare_sha1`, and `drive_magisk` are not ported. Magisk itself is GPL-3.0 and is not shipped. Flashwright does not run magiskboot. The patched init_boot is checked by the read-only parser in `flashwright-bootimg`.

## Direct crates

Versions are the ones resolved in `Cargo.lock` on 9 Oct 2026.

| Crate | Version | Licence |
| --- | --- | --- |
| tokio | 1.53.2 | MIT |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| toml | 0.8.23 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| sha1 | 0.10.7 | MIT OR Apache-2.0 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| lz4_flex | 0.11.6 | MIT |
| zip | 9.0.0 | MIT |
| tracing | 0.1.44 | MIT |
| libc | 0.2.190 | MIT OR Apache-2.0 |
| windows | 0.62.2 | MIT OR Apache-2.0 |
| serde_json | 1.0.151 | MIT OR Apache-2.0 |
| serde_jcs | 0.1.0 | MIT OR Apache-2.0 |
| uuid | 1.11.0 | Apache-2.0 OR MIT |
| payload_dumper | 0.8.4 | Apache-2.0 |
| prost | 0.14.4 | Apache-2.0 |
| prost-build | 0.14.4 | Apache-2.0 |
| protoc-bin-vendored | 3.3.0 | Apache-2.0 (bundled `protoc` is BSD-3-Clause) |

`zip` is built with `deflate-flate2-zlib-rs` only. `flate2` in the parser uses the Rust backend. `lz4_flex` is built with `safe-decode`.

## Update wizard

| Item | Source | Licence | Where |
| --- | --- | --- | --- |
| Tauri 2.12.2, tauri-build 2.7.1, tauri-plugin-dialog 2.8.1, tauri-plugin-opener 2.7.0, `@tauri-apps/api` 2.12.2, `@tauri-apps/cli` 2.12.1 | https://github.com/tauri-apps/tauri | Apache-2.0 OR MIT | `apps/flashwright-gui` |
| serde_json 1.0.151 | https://github.com/serde-rs/json | MIT OR Apache-2.0 | `crates/flashwright-core` |
| serde_jcs 0.1.0 | https://github.com/l1h3r/serde_jcs | MIT OR Apache-2.0 | Plan canonicalisation |
| sha2 0.10.9, hex 0.4.3 | RustCrypto | MIT OR Apache-2.0 | Plan digest |
| uuid 1.11.0 | https://github.com/uuid-rs/uuid | MIT OR Apache-2.0 | Plan ids |
| IBM Plex Sans and IBM Plex Mono | https://github.com/IBM/plex `763c36ef9117782905ae010056dfbe8fd2653a25` | SIL OFL-1.1 | `apps/flashwright-gui/ui/theme/fonts` |
| Vite 6.4.4 | https://github.com/vitejs/vite | MIT | Wizard bundle |
| TypeScript | https://github.com/microsoft/TypeScript | Apache-2.0 | Wizard typecheck |
| Playwright Core | https://github.com/microsoft/playwright | Apache-2.0 | Screenshot capture only |

IBM’s licence reserves the font name “Plex”. The files keep the upstream family name.

Clatty Works brand assets (in-house). The file list is in `docs/disclaimer.md`. They are not a third-party logo.
