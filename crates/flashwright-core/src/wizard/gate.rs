// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

//! One accept path for confirm and dry-run.
//!
//! [`crate::wizard::WizardSession::confirm_and_run`] and the window engine both
//! call [`check`] before a plan is consumed or a token is minted.

use crate::CoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Life {
    Issued,
    Consumed,
    Discarded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Check {
    pub previously_consumed: bool,
    pub in_matching_review: bool,
    pub life: Life,
    pub hash_eq: bool,
    pub expired: bool,
    pub running: bool,
    pub device_ok: bool,
    pub inputs_ok: bool,
    pub plan_dry: bool,
    pub call_dry: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Caller marks the plan consumed before any step.
    Consume,
    /// Caller marks the plan discarded. The error already explains why.
    Discard(DiscardReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiscardReason {
    Expired,
    DeviceChanged,
    InputChanged,
}

pub(crate) fn check(input: Check) -> Result<Verdict, CoreError> {
    if input.previously_consumed || input.life == Life::Consumed {
        return Err(CoreError::AlreadyUsed);
    }
    if !input.in_matching_review {
        return Err(CoreError::WrongState);
    }
    if input.life == Life::Discarded {
        return Err(CoreError::Discarded);
    }
    if !input.hash_eq {
        return Err(CoreError::Rejected {
            reason: "That plan code was not issued by Flashwright.".to_string(),
        });
    }
    if input.expired {
        return Ok(Verdict::Discard(DiscardReason::Expired));
    }
    if input.running {
        return Err(CoreError::Rejected {
            reason: "A job is already running.".to_string(),
        });
    }
    if !input.device_ok {
        return Ok(Verdict::Discard(DiscardReason::DeviceChanged));
    }
    if !input.inputs_ok {
        return Ok(Verdict::Discard(DiscardReason::InputChanged));
    }
    if input.call_dry && !input.plan_dry {
        return Err(CoreError::NotDryRun);
    }
    if !input.call_dry && input.plan_dry {
        return Err(CoreError::DryRunPlan);
    }
    Ok(Verdict::Consume)
}

pub(crate) fn discard_error(reason: DiscardReason) -> CoreError {
    match reason {
        DiscardReason::Expired => CoreError::Rejected {
            reason: "The plan has expired. Build it again.".to_string(),
        },
        DiscardReason::DeviceChanged => CoreError::Rejected {
            reason: "The phone changed after the plan was built.".to_string(),
        },
        DiscardReason::InputChanged => CoreError::Rejected {
            reason: "The package changed after the plan was built.".to_string(),
        },
    }
}
