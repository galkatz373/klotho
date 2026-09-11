//! Journey host trait. Implementations never expose Projection writes.

use klotho_core::{Hash, PlayerId};
use klotho_ir::{Analog, IntentTarget, Name, Verb};

use crate::error::EvalError;
use crate::evidence::EvidenceContext;
use crate::journey::{CapturePoint, DeviceAction, JourneyAssertion};

/// Outcome of one step.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct StepOutcome {
    /// Ticks consumed.
    pub ticks: u32,
    /// Escaped last semantic state.
    pub last_state: String,
    /// Affordance that blocked progress, if any.
    pub blocked: String,
}

/// Headless journey session. Public inputs in; semantic facts out.
pub trait JourneyHost {
    /// Device sample through the trusted adapter.
    fn apply_device(&mut self, action: &DeviceAction) -> Result<StepOutcome, EvalError>;

    /// Verb fixture. Agency is stamped by the host, never the caller.
    fn apply_fixture(
        &mut self,
        player: PlayerId,
        verb: Verb,
        target: IntentTarget,
        analog: Analog,
    ) -> Result<StepOutcome, EvalError>;

    /// Step with no player packet.
    fn wait(&mut self, ticks: u32) -> Result<StepOutcome, EvalError>;

    /// Presentation-only camera move.
    fn camera(&mut self, name: &Name) -> Result<StepOutcome, EvalError>;

    /// Save under `slot`.
    fn save(&mut self, slot: &Name) -> Result<(), EvalError>;

    /// Load `slot`.
    fn load(&mut self, slot: &Name) -> Result<(), EvalError>;

    /// Record a capture from a public snapshot / semantic state.
    fn capture(&mut self, point: &CapturePoint) -> Result<Hash, EvalError>;

    /// Check one assertion.
    fn check(&self, assertion: &JourneyAssertion) -> Result<(), EvalError>;

    /// Last reachable semantic state for diagnostics.
    fn last_state(&self) -> String;

    /// Blocked affordance for diagnostics.
    fn blocked_affordance(&self) -> String;

    /// Ticks consumed since start.
    fn ticks(&self) -> u32;

    /// Hashes to bind evidence to.
    fn evidence_context(&self, change: Hash) -> EvidenceContext;
}
