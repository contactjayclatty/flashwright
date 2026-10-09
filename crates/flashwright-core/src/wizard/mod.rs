// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Wizard state machine and the catalogue confirm session.
//!
//! The window calls [`Engine`]. Every real flash still goes through
//! [`WizardSession::confirm_and_run`], which is the only public write entry.
//! The window engine uses the same accept checks and mints its token with the
//! same private constructor. Neither path is reachable as a free function.

mod device;
mod engine;
mod gate;
mod links;
mod model;
mod session;
mod steps;

pub use device::{
    inactive_slot, unlocked_from_props, DeviceInfo, DeviceSummary, DeviceTransport, EmptyTransport,
    Mode, Partition, Slot,
};
pub use engine::Engine;
pub use links::external_url;
pub use model::{
    BackupSet, BurstStats, ChoiceView, DriverStatus, ExternalLink, FirmwareRef, FirmwareReport,
    JobView, LogLine, Notice, PlanKind, RecoveryOption, Snapshot, ToolsStatus, WizardEvent,
    WizardPlan,
};
pub use session::{
    plan_code, plan_hash, Clock, FirmwareClaim, FixedClock, ImageSeal, Phase, PlanPreview,
    PlanRequest, PlanStep, PlanStepView, RunReport, SystemClock, WizardSession,
};
pub use steps::{GateView, Route, Step, StepClass, Tool, SCHEMA};

#[cfg(test)]
mod qa;

use std::marker::PhantomData;

/// Dry-run plans cannot be confirmed. The type parameter carries that.
pub struct DryRunKind;

/// Real plans are the only ones [`require_real_plan`] accepts.
pub struct RealKind;

/// Issued plan marker. Fields are private. A dry plan is a different type
/// from a real plan, so it cannot be passed where a real plan is required.
pub struct IssuedPlan<K> {
    _kind: PhantomData<K>,
}

impl IssuedPlan<DryRunKind> {
    pub fn example() -> Self {
        Self { _kind: PhantomData }
    }
}

impl IssuedPlan<RealKind> {
    pub fn example() -> Self {
        Self { _kind: PhantomData }
    }
}

/// Compile-time mirror of the dry-run refusal. A dry plan does not type-check.
pub fn require_real_plan(_plan: IssuedPlan<RealKind>) {}
