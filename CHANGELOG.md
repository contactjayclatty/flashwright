# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Phone patch: after you confirm "Patch on your phone?", the Magisk app you already installed patches init_boot. Flashwright then checks that image on the PC with magiskboot, which you supply. Pixel 9 Pro XL (komodo) uses init_boot. A hidden or renamed Magisk app is refused. See `docs/m3.md`.
- Safety checks before a patch or a flash: device and build match, security patch, Magisk, bootloader anti-rollback, and slot rules. A dry run prints WOULD RUN or WOULD BLOCK and does not write. Stock init_boot is read, stored with a SHA-256, and compared with the factory image. A mismatch stops the job.
- Windows update wizard: sample phones, a hashed plan, and a window that walks through connect, choose, firmware, review, and flash.
- M1 device layer: platform-tools policy, argv process runner, device scan and props, mode waits, and USB driver classification. See `docs/m1.md`.
- Typed command catalogue, size-aware timeouts, output parsers, and a review-only confirm that mints the write token inside `flashwright-core`.
- GNU AGPL-3.0 licence text. The project is AGPL-3.0-or-later.
- Disclaimer with the reuse log and third-party notices.
- Ignore rules for Rust and Node/Tauri build outputs, and for firmware files (`*.zip`, `*.img`, `payload.bin`).
