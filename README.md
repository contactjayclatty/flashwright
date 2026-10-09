<p align="center">
  <img src="assets/hero.png" alt="Flashwright. Setup complete." width="100%">
</p>

<p align="center">
  <img src="assets/app-icon.png" alt="Flashwright app icon" width="80">
</p>

<p align="center">
  <img alt="Platform: Windows 11" src="https://img.shields.io/badge/Platform-Windows%2011-0A5C5C?style=flat-square&labelColor=2A2E31">
  <a href="https://www.gnu.org/licenses/agpl-3.0.html"><img alt="Licence: AGPL-3.0" src="https://img.shields.io/badge/Licence-AGPL--3.0-0A5C5C?style=flat-square&labelColor=2A2E31"></a>
  <img alt="Status: in development" src="https://img.shields.io/badge/Status-in%20development-0A5C5C?style=flat-square&labelColor=2A2E31">
</p>

<p align="center">
  <strong>Update, root and flash Android phones on Windows 11.</strong><br>
  Flashwright is a Setup Wizard for Android phones, Google Pixel and others.<br>
  One guided path, from the moment you plug in to <strong>Setup complete.</strong>
</p>

## What it does

<table>
  <tr>
    <td align="center" width="33%" valign="top">
      <img src="assets/icons/update.png" alt="" width="64"><br>
      <strong>Update</strong><br>
      A monthly update that keeps root.
    </td>
    <td align="center" width="33%" valign="top">
      <img src="assets/icons/root.png" alt="" width="64"><br>
      <strong>Root</strong><br>
      Patch the boot image and install Magisk.
    </td>
    <td align="center" width="33%" valign="top">
      <img src="assets/icons/flash.png" alt="" width="64"><br>
      <strong>Flash</strong><br>
      Flash a factory image. Each file shows its SHA-256.
    </td>
  </tr>
  <tr>
    <td align="center" valign="top">
      <img src="assets/icons/backup.png" alt="" width="64"><br>
      <strong>Backups</strong><br>
      Boot and vbmeta, saved for you. Restore is one click.
    </td>
    <td align="center" valign="top">
      <img src="assets/icons/dry-run.png" alt="" width="64"><br>
      <strong>Dry-run preview</strong><br>
      See what would run, or why it would be blocked. Nothing is written yet.
    </td>
    <td align="center" valign="top">
      <img src="assets/icons/recovery.png" alt="" width="64"><br>
      <strong>Recovery</strong><br>
      Re-flash a saved backup, or return the phone to stock.
    </td>
  </tr>
</table>

<p align="center">Expert mode keeps extra controls within reach.</p>

## Status

A Pixel factory package or a full OTA package can be opened. Flashwright extracts init_boot, or boot on older phones, and checks the package hash. Safety checks run before a patch or a flash, and again before every write. They cover the phone, the factory image, the security patch, Magisk, the bootloader, and the slot. A dry run prints WOULD RUN or WOULD BLOCK and does not write. Stock init_boot is read, stored with a SHA-256, and compared with the factory image. A mismatch stops the job. The LU0 and FIPS regions are refused.

The Windows 11 wizard walks through connect, choose, firmware, review, and flash. A plan can be confirmed only from the review step, and only once. Platform-tools are not bundled. This build uses sample phones and does not write to a device.

Sign-off: M5 PROVISIONAL (mocks only). T1.3, T1.11, and T4.15 stay blocked until they are recorded on a phone.

Android, Google, and Pixel are trademarks of Google LLC. Magisk is a project by topjohnwu. They are named descriptively. Flashwright is not affiliated with or endorsed by them.

Platform-tools are located and version-checked before a scan. Writes stay off until an allow-list entry has per-file hashes and has been device-tested. A confirmed plan is single-use: Flashwright re-checks the phone and the input files, then discards the plan if either has changed.

## Features

- **Patch on your phone.** The Magisk app you already installed patches init_boot. You confirm "Patch on your phone?" before anything is copied to the phone.
- **PC check.** Flashwright reads the patched init_boot and checks that Magisk's init and the stock SHA-1 are present.
- **Pixel 9 Pro XL.** That phone (komodo) patches init_boot.

## Status

The patch step is exercised with sample command output. It does not talk to a phone. A hidden or renamed Magisk app stops the plan before any file is copied. The LU0 / FIPS region is refused.

## The wizard

<table>
  <tr>
    <td align="center" width="50%" valign="top">
      <img src="assets/screens/connect.png" alt="Connect step. The phone is plugged in and the wizard shows its model, build and bootloader." width="100%"><br>
      <strong>Connect</strong><br>
      Plug the phone in. Flashwright shows the model, the build and the bootloader.
    </td>
    <td align="center" width="50%" valign="top">
      <img src="assets/screens/choose.png" alt="Choose a job: monthly update and keep root, root the phone, flash firmware, or back up." width="100%"><br>
      <strong>Choose</strong><br>
      Monthly update and keep root, root the phone, flash firmware, or back up.
    </td>
  </tr>
  <tr>
    <td align="center" valign="top">
      <img src="assets/screens/firmware.png" alt="Firmware step. A factory image is selected and each file lists a SHA-256 checksum." width="100%"><br>
      <strong>Firmware</strong><br>
      Pick a factory image. Every file is shown with its SHA-256.
    </td>
    <td align="center" valign="top">
      <img src="assets/screens/dry-run.png" alt="Dry-run preview listing images that would be written. Nothing is written yet." width="100%"><br>
      <strong>Dry-run</strong><br>
      A preview of every image that would be written. Nothing is written yet.
    </td>
  </tr>
  <tr>
    <td align="center" valign="top">
      <img src="assets/screens/done.png" alt="The wizard finished. The screen reads Setup complete." width="100%"><br>
      <strong>Done</strong><br>
      The wizard finishes on Setup complete.
    </td>
    <td align="center" valign="top">
      <img src="assets/screens/backups.png" alt="Backups list with boot and vbmeta images and a restore action." width="100%"><br>
      <strong>Backups</strong><br>
      Boot and vbmeta, kept on the PC, ready to restore.
    </td>
  </tr>
</table>

> Flashing can wipe data. Back up first.

<p align="center">
  AGPL-3.0-or-later<br>
  Copyright (C) 2026 Jay Clatty (Clatty Works)
</p>

<p align="center">
  <img src="assets/clatty-works-mark.png" alt="Clatty Works" width="64"><br>
  <strong>Setup complete.</strong><br>
  Made by <a href="https://github.com/contactjayclatty/clatty-works">Clatty Works</a>
</p>
