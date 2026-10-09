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
