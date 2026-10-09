# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Windows update wizard: sample phones, a hashed plan, and a window that walks through connect, choose, firmware, review, and flash.
- M1 device layer: platform-tools policy, argv process runner, device scan and props, mode waits, and USB driver classification. See `docs/m1.md`.
- Typed command catalogue, size-aware timeouts, output parsers, and a review-only confirm that mints the write token inside `flashwright-core`.
- GNU AGPL-3.0 licence text. The project is AGPL-3.0-or-later.
- Disclaimer with the reuse log and third-party notices.
- Ignore rules for Rust and Node/Tauri build outputs, and for firmware files (`*.zip`, `*.img`, `payload.bin`).
