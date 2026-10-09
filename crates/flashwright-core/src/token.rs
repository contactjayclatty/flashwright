// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! Write capability minted only after a plan is confirmed.
//!
//! The marker is `PhantomData<Cell<()>>`: `Send` and not `Sync`. The runtime
//! is a multi-thread Tokio scheduler, so a `!Send` token cannot move into a
//! spawned task. The executor task owns the token and passes it by reference.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::Mutex;

use uuid::Uuid;

use crate::cmd::WriteCmd;

static OPEN_RUNS: Mutex<BTreeMap<u128, bool>> = Mutex::new(BTreeMap::new());

/// One confirmed execution. Closed when its [`WriteToken`] is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunId(u128);

impl RunId {
    pub fn get(self) -> u128 {
        self.0
    }

    pub(crate) fn fresh() -> Self {
        Self(Uuid::now_v7().as_u128())
    }
}

/// Plan that passed the confirm checks. Only this crate can construct it.
pub(crate) struct ConfirmedPlan {
    plan_hash: String,
    serial: String,
    run_id: RunId,
    steps: Vec<Vec<u8>>,
}

impl ConfirmedPlan {
    pub(crate) fn plan_hash(&self) -> &str {
        &self.plan_hash
    }

    pub(crate) fn serial(&self) -> &str {
        &self.serial
    }

    pub(crate) fn run_id(&self) -> RunId {
        self.run_id
    }

    pub(crate) fn steps(&self) -> &[Vec<u8>] {
        &self.steps
    }
}

/// Capability for one write run. Fields are private. It is not cloned or copied.
pub struct WriteToken {
    plan_hash: String,
    serial: String,
    run_id: u128,
    marker: PhantomData<Cell<()>>,
}

impl WriteToken {
    pub(crate) fn mint(plan: &ConfirmedPlan, run_id: RunId) -> Self {
        OPEN_RUNS
            .lock()
            .expect("run registry")
            .insert(run_id.get(), true);
        Self {
            plan_hash: plan.plan_hash.clone(),
            serial: plan.serial.clone(),
            run_id: run_id.get(),
            marker: PhantomData,
        }
    }

    pub fn plan_hash(&self) -> &str {
        &self.plan_hash
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn run_id(&self) -> u128 {
        self.run_id
    }
}

impl Drop for WriteToken {
    fn drop(&mut self) {
        if let Ok(mut runs) = OPEN_RUNS.lock() {
            runs.insert(self.run_id, false);
        }
    }
}

pub(crate) fn run_is_open(run_id: u128) -> bool {
    OPEN_RUNS
        .lock()
        .expect("run registry")
        .get(&run_id)
        .copied()
        .unwrap_or(false)
}

/// Count runs whose token has not been dropped. Tests use this.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn open_run_count() -> usize {
    OPEN_RUNS
        .lock()
        .expect("run registry")
        .values()
        .filter(|open| **open)
        .count()
}

pub(crate) fn mint_confirmed(
    plan_hash: &str,
    serial: &str,
    steps: &[WriteCmd],
) -> (ConfirmedPlan, WriteToken) {
    let run_id = RunId::fresh();
    let steps = steps
        .iter()
        .map(|step| serde_jcs::to_vec(step).expect("write command serialises"))
        .collect();
    let plan = ConfirmedPlan {
        plan_hash: plan_hash.to_string(),
        serial: serial.to_string(),
        run_id,
        steps,
    };
    let token = WriteToken::mint(&plan, run_id);
    (plan, token)
}

pub fn dry_run_lines(argv_lines: &[Vec<String>]) -> Vec<String> {
    argv_lines
        .iter()
        .map(|argv| format!("WOULD RUN: {}", argv.join(" ")))
        .collect()
}

/// Serialises tests that mint or count open runs. The registry is process-wide.
#[cfg(test)]
fn run_gate() -> &'static tokio::sync::Mutex<()> {
    static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    &GATE
}

#[cfg(test)]
pub(crate) async fn test_gate() -> tokio::sync::MutexGuard<'static, ()> {
    run_gate().lock().await
}

#[cfg(test)]
pub(crate) fn test_gate_blocking() -> tokio::sync::MutexGuard<'static, ()> {
    run_gate().blocking_lock()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_mints_nothing() {
        let _gate = test_gate_blocking();
        let before = OPEN_RUNS.lock().unwrap().clone();
        let lines = dry_run_lines(&[vec!["-s".into(), "S".into(), "reboot".into()]]);
        assert_eq!(lines, vec!["WOULD RUN: -s S reboot".to_string()]);
        assert_eq!(*OPEN_RUNS.lock().unwrap(), before);
    }

    #[test]
    fn drop_closes_the_run() {
        let _gate = test_gate_blocking();
        let (plan, token) = mint_confirmed("flp1-abc", "serial", &[]);
        assert!(run_is_open(token.run_id()));
        assert_eq!(token.plan_hash(), plan.plan_hash());
        let id = token.run_id();
        drop(token);
        assert!(!run_is_open(id));
    }
}
