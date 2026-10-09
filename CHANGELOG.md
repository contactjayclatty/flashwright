# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A plan request names the phone and the steps. Flashwright reads every safety fact itself, stores the evidence in the plan hash, and draws a random nonce. A plan expires after fifteen minutes. A used or dry-run plan cannot be replayed.
- Stock backup streams each image to disk, reads it a second time, and checks both against `sha256sum` on the phone. Reads honour the catalogue size cap and the pull timeout.
- Gate G26 blocks a vbmeta flash. Gates G01 through G28 are covered by tests. Each write, including reboot, set-active, and cleanup, is checked again, and the phone is read again before a token is minted.
- Linux CI compares the spawn log with an `strace` `execve` trace. Windows records the same log and does not attach ETW.
- Open a Pixel factory zip or a full OTA zip, extract `init_boot` or `boot`, and check the package SHA-256 and the payload partition hash. Nothing is written to a phone. See `docs/m2.md`.
- Safety checks before a patch or a flash: device and build match, security patch, Magisk, bootloader anti-rollback, and slot rules. A dry run prints WOULD RUN or WOULD BLOCK and does not write. Stock boot images are read from the phone, checked against the phone's own SHA-256, and stored with a manifest.
- Plan hashes cover the phone facts, gate results, backup, timeouts, and a one-time nonce. A dry run uses up a dry-run plan. Confirm reads the phone again before each write.
- Windows update wizard: sample phones, a hashed plan, and a window that walks through connect, choose, firmware, review, and flash.
- M1 device layer: platform-tools policy, argv process runner, device scan and props, mode waits, and USB driver classification. See `docs/m1.md`.
- Typed command catalogue, size-aware timeouts, output parsers, and a review-only confirm that mints the write token inside `flashwright-core`.
- GNU AGPL-3.0 licence text. The project is AGPL-3.0-or-later.
- Disclaimer with the reuse log and third-party notices.
- Ignore rules for Rust and Node/Tauri build outputs, and for firmware files (`*.zip`, `*.img`, `payload.bin`).
