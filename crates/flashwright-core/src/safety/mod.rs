// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Safety gates for a plan.
//!
//! Dry run and confirm both call [`evaluate`]. A dry run prints the result
//! and does not mint a write token.

mod backup;
mod evaluate;
mod tables;

pub use backup::{capture_stock, sha256_hex, BackupSet, BackupState};
pub use evaluate::{
    blocking, bootloader_older, dry_run_lines, evaluate_step, gate_ids, needs_backup,
    parse_bootloader, spl_from_build, CollectedFacts, GateBlock, GateDecision, Severity,
};
pub(crate) use evaluate::{evaluate, evaluate_acked, FactEvidence, SafetyFacts};
pub use tables::{
    log_ported_items, ported_items, tables, PortedItem, SafetyTables, UPSTREAM_COMMIT,
};
