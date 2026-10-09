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

- Phone patch: after you confirm "Patch on your phone?", the Magisk app you already installed patches init_boot. Flashwright then reads that image and checks Magisk's init and the stock SHA-1. Pixel 9 Pro XL (komodo) uses init_boot. A hidden or renamed Magisk app is refused. See `docs/m3.md`.
- A pull stores one file name, not a host path. The phone script hashes the stock image before patching. Each device names its patch partition in the catalogue. The boot image parser refuses overlays, duplicate entries, and a config larger than 4 KiB. Pushed Magisk components must match the pulled app.
- Confirm checks `safety::evaluate_step` before every write. The step list, including cleanup, is fixed by core and stored in the plan hash. A failed step stops later writes. Cleanup from that list still runs.
- `safety::evaluate_step` is the public gate check. It takes only facts Flashwright collected and hashed for that phone's serial. `with_platform_tools` opens a session on the adb and fastboot installed on this machine. A changed phone or backup discards the plan. Firmware facts come from the package and the boot image. A stock backup gets a new id and is never replaced. An update or sideload uses the boot image size and the inactive slot. `getvar` reads stderr, matches yes and no exactly, and runs only in fastboot. A failed device read stops the plan. An optional linked dry run is part of the plan hash. Windows USB library hashes stay locked until the tool starts.
- The update wizard confirms a plan once from the review step. The window shell grants one capability, and sample phones stay out of the release bundle. Each write is checked with the safety gates before it runs.
- Open a Pixel factory zip or a full OTA zip, extract `init_boot` or `boot`, and check the package SHA-256 and the payload partition hash. Nothing is written to a phone. See `docs/m2.md`.
- A plan request names the phone and the steps. Flashwright reads every safety fact itself, stores the evidence in the plan hash, and draws a random nonce. A plan expires after fifteen minutes. A used or dry-run plan cannot be replayed.
- Confirm reads the phone and the backup again. If either differs from the hashed snapshot, the plan is discarded. The hash covers the security patch, fingerprints, timestamps, bootloaders, slot, Magisk, and API level, and the facts are bound to the plan's serial. A gate with no read blocks. `safety::evaluate_step` is the write check.
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
