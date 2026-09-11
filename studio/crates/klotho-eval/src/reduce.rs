//! Minimize a failing journey. The reduced script must replay exactly.

use crate::error::EvalError;
use crate::host::JourneyHost;
use crate::journey::{JourneySpec, JourneyStep};
use crate::run::run_journey;
use klotho_core::Hash;

/// Drop steps while `run_journey` still fails with the same error.
pub fn minimize<H: JourneyHost, F: Fn() -> H>(
    seed: F,
    spec: &JourneySpec,
    change: Hash,
) -> Result<(JourneySpec, EvalError), EvalError> {
    let original = match run_journey(&mut seed(), spec, change) {
        Ok(_) => return Err(EvalError::Replay),
        Err(e) => e,
    };
    let mut kept = spec.clone();
    let mut i = 0;
    while i < kept.steps.len() {
        let mut trial = kept.clone();
        trial.steps.remove(i);
        match run_journey(&mut seed(), &trial, change) {
            Err(err) if same_failure(&err, &original) => {
                kept = trial;
            }
            _ => i += 1,
        }
    }
    match run_journey(&mut seed(), &kept, change) {
        Err(err) if same_failure(&err, &original) => Ok((kept, err)),
        _ => Err(EvalError::Replay),
    }
}

fn same_failure(a: &EvalError, b: &EvalError) -> bool {
    match (a, b) {
        (
            EvalError::Unreachable {
                last_state: la,
                blocked: ba,
                ..
            },
            EvalError::Unreachable {
                last_state: lb,
                blocked: bb,
                ..
            },
        ) => la == lb && ba == bb,
        (EvalError::Budget { used: ua, cap: ca }, EvalError::Budget { used: ub, cap: cb }) => {
            ua == ub && ca == cb
        }
        _ => a == b,
    }
}

/// True when `steps` contains no Projection-write variant (type-level closed).
#[must_use]
pub fn steps_are_public_input(steps: &[JourneyStep]) -> bool {
    steps.iter().all(|s| {
        matches!(
            s,
            JourneyStep::Device { .. }
                | JourneyStep::Fixture { .. }
                | JourneyStep::Wait { .. }
                | JourneyStep::Camera { .. }
                | JourneyStep::Save { .. }
                | JourneyStep::Load { .. }
        )
    })
}
