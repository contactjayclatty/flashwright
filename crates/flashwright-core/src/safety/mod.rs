// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Safety gates for a plan.
//!
//! Dry run and confirm both call [`evaluate`]. A dry run prints the result
//! and does not mint a write token.

mod backup;
mod evaluate;
mod tables;

pub use backup::{
    pull_stock_init_boot, sha256_hex, BackupState, BytesInitBoot, FactoryInitBoot, InitBootRecord,
};
pub use evaluate::{
    blocking, bootloader_older, dry_run_lines, evaluate, gate_ids, needs_backup, parse_bootloader,
    spl_from_build, GateBlock, GateDecision, LegacyChecks, SafetyFacts, Severity,
};
pub use tables::{
    log_ported_items, ported_items, tables, PortedItem, SafetyTables, UPSTREAM_COMMIT,
};
