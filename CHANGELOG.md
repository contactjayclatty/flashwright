# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Process spawn accepts only a hash-checked `adb` or `fastboot` and a catalogue command. A helper path, a shell, or an interpreter is refused. `SystemRunner`, free-form invocations, and `flash_args` are no longer public.
- `getvar`, `getvar_all`, and `state` take a validated serial and a fastboot variable. `build_plan` is async and stamps a nonce into the plan hash.
- Confirm re-checks the phone and the input files. The same plan hash cannot be confirmed again. G21 and G22 block a write when platform-tools are not verified, including when no adb server is listening on port 5037.
- Fastboot `getvar` is parsed from stdout and stderr, and the slot compare is exact.
- Sideload uses a 900 second quiet window after a progress percent, and a 300 second watchdog before that.
- `xtask lint-spawn` scans `apps/` as well as the crates, and it rejects an aliased `Command::new` and `#[expect(clippy::disallowed_methods)]` outside the spawn module.

### Added

- Safety checks before a patch or a flash: device and build match, security patch, Magisk, bootloader anti-rollback, and slot rules. A dry run prints WOULD RUN or WOULD BLOCK and does not write. Stock init_boot is read, stored with a SHA-256, and compared with the factory image. A mismatch stops the job.
- Windows update wizard: sample phones, a hashed plan, and a window that walks through connect, choose, firmware, review, and flash.
- M1 device layer: platform-tools policy, argv process runner, device scan and props, mode waits, and USB driver classification. See `docs/m1.md`.
- Typed command catalogue, size-aware timeouts, output parsers, and a review-only confirm that mints the write token inside `flashwright-core`.
- GNU AGPL-3.0 licence text. The project is AGPL-3.0-or-later.
- Disclaimer with the reuse log and third-party notices.
- Ignore rules for Rust and Node/Tauri build outputs, and for firmware files (`*.zip`, `*.img`, `payload.bin`).
