# Third-party material

Flashwright is AGPL-3.0-or-later. See `LICENSE`.

Portions derived from PixelFlasher, Copyright badabing2005, AGPL-3.0-or-later, https://github.com/badabing2005/PixelFlasher, commit 081286d (`081286d6baf8ac969df29e8b0ab3a5b6ae4cfb54`).

M1 does not copy PixelFlasher source. The behaviour rows, and the items that are not used, are in `docs/disclaimer.md`.

M1 does not use payload-dumper-rust, the AOSP update-engine proto, or an avbtool port.

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
| zip | 9.0.0 | MIT |
| tracing | 0.1.44 | MIT |
| libc | 0.2.190 | MIT OR Apache-2.0 |
| windows | 0.62.2 | MIT OR Apache-2.0 |

`zip` is built with `deflate-flate2-zlib-rs` only.

## Update wizard

| Item | Source | Licence | Where |
| --- | --- | --- | --- |
| Tauri 2.12.2, tauri-build 2.7.1, tauri-plugin-dialog 2.8.1, tauri-plugin-opener 2.7.0, `@tauri-apps/api` 2.12.2, `@tauri-apps/cli` 2.12.1 | https://github.com/tauri-apps/tauri | Apache-2.0 OR MIT | `apps/flashwright-gui` |
| serde_json 1.0.151 | https://github.com/serde-rs/json | MIT OR Apache-2.0 | `crates/flashwright-wizard` |
| serde_jcs 0.1.0 | https://github.com/l1h3r/serde_jcs | MIT OR Apache-2.0 | Plan canonicalisation |
| sha2 0.10.9, hex 0.4.3 | RustCrypto | MIT OR Apache-2.0 | Plan digest |
| uuid 1.27.0 | https://github.com/uuid-rs/uuid | MIT OR Apache-2.0 | Plan ids |
| IBM Plex Sans and IBM Plex Mono | https://github.com/IBM/plex `763c36ef9117782905ae010056dfbe8fd2653a25` | SIL OFL-1.1 | `apps/flashwright-gui/ui/theme/fonts` |
| Vite 6.4.4 | https://github.com/vitejs/vite | MIT | Wizard bundle |
| TypeScript | https://github.com/microsoft/TypeScript | Apache-2.0 | Wizard typecheck |
| Playwright Core | https://github.com/microsoft/playwright | Apache-2.0 | Screenshot capture only |

IBM’s licence reserves the font name “Plex”. The files keep the upstream family name.

The wizard skin (`flashwright-ui.css`) is an original Clatty Works sheet shipped with this repository. The icon and banner marks are original. They are not a third-party logo.
