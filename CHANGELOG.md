# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The update wizard confirms a plan once from the review step. The window shell grants one capability, and sample phones stay out of the release bundle. Each write is checked with the safety gates before it runs.
- A patch confirm returns to the firmware step. Review buttons follow the plan dry-run flag. Cancelling a finished job does nothing. Recovery commands name a real slot, a restore uses the chosen backup file, and the package pickers do not block the window.
- Open a Pixel factory zip or a full OTA zip, extract `init_boot` or `boot`, and check the package SHA-256 and the payload partition hash. Nothing is written to a phone. See `docs/m2.md`.
- Safety checks before a patch or a flash: device and build match, security patch, Magisk, bootloader anti-rollback, and slot rules. A dry run prints WOULD RUN or WOULD BLOCK and does not write. Stock init_boot is read, stored with a SHA-256, and compared with the factory image. A mismatch stops the job.
- Windows update wizard: sample phones, a hashed plan, and a window that walks through connect, choose, firmware, review, and flash.
- M1 device layer: platform-tools policy, argv process runner, device scan and props, mode waits, and USB driver classification. See `docs/m1.md`.
- Typed command catalogue, size-aware timeouts, output parsers, and a review-only confirm that mints the write token inside `flashwright-core`.
- GNU AGPL-3.0 licence text. The project is AGPL-3.0-or-later.
- Disclaimer with the reuse log and third-party notices.
- Ignore rules for Rust and Node/Tauri build outputs, and for firmware files (`*.zip`, `*.img`, `payload.bin`).
