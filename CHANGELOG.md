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

### Fixed

- A factory or OTA package is hashed and extracted from the open file. The published SHA-256 is required. The phone codename, build date, and security patch come from the scanned phone. The image security patch is read from the boot header, and the build date from `build.prop` when that file is present. A value that cannot be read is acknowledged. It is not skipped and it is not a hard block. A codename outside the device table is refused.

### Added

- The update wizard confirms a plan once from the review step. The window shell grants one capability, and sample phones stay out of the release bundle. Each write is checked with the safety gates before it runs.
- Open a Pixel factory zip or a full OTA zip, extract `init_boot` or `boot`, and check the package SHA-256 and the payload partition hash. Nothing is written to a phone. See `docs/m2.md`.
- A plan request names the phone and the steps. Flashwright reads every safety fact itself, stores the evidence in the plan hash, and draws a random nonce. A plan expires after fifteen minutes. A used or dry-run plan cannot be replayed.
- Stock backup streams each image to disk, reads it a second time, and checks both against `sha256sum` on the phone. Reads honour the catalogue size cap and the pull timeout.
- Gate G26 blocks a vbmeta flash. Gates G01 through G28 are covered by tests. Each write, including reboot, set-active, and cleanup, is checked again, and the phone is read again before a token is minted.
- Linux CI compares the spawn log with an `strace` `execve` trace. Windows records the same log and does not attach ETW.
- Safety checks before a patch or a flash: device and build match, security patch, Magisk, bootloader anti-rollback, and slot rules. A dry run prints WOULD RUN or WOULD BLOCK and does not write. Stock boot images are read from the phone, checked against the phone's own SHA-256, and stored with a manifest. A mismatch stops the job.
- Plan hashes cover the phone facts, gate results, backup, timeouts, and a one-time nonce. A dry run uses up a dry-run plan. Confirm reads the phone again before each write.
- Windows update wizard: sample phones, a hashed plan, and a window that walks through connect, choose, firmware, review, and flash.
- M1 device layer: platform-tools policy, argv process runner, device scan and props, mode waits, and USB driver classification. See `docs/m1.md`.
- Typed command catalogue, size-aware timeouts, output parsers, and a review-only confirm that mints the write token inside `flashwright-core`.
- GNU AGPL-3.0 licence text. The project is AGPL-3.0-or-later.
- Disclaimer with the reuse log and third-party notices.
- Ignore rules for Rust and Node/Tauri build outputs, and for firmware files (`*.zip`, `*.img`, `payload.bin`).
