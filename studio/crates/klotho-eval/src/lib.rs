//! Journeys, public-input execution, evidence bundles, and affected-test selection.
//!
//! The library does not enable `klotho-world/mutate`, append Trace, or mint
//! [`klotho_ir::Agency`]. Kernel-backed runs go through
//! [`klotho_debug::JourneyKernel`].
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod bot;
mod contract;
mod error;
mod evidence;
mod host;
mod ids;
mod journey;
mod kernel;
mod reach;
mod reduce;
mod run;
mod script;
mod select;

pub use bot::{AutomatedPlayer, BotManifest};
pub use contract::{
    AcceptanceContract, BudgetTarget, ChangeScope, InvariantRef, QualityTarget, SemanticClaim,
};
pub use error::EvalError;
pub use evidence::{
    ApprovalRef, ArtifactRef, CheckEvidence, CheckLayer, EvidenceBuilder, EvidenceBundle,
    EvidenceContext,
};
pub use host::{JourneyHost, StepOutcome};
pub use ids::JourneyId;
pub use journey::{
    CaptureKind, CapturePoint, DeviceAction, JourneyAssertion, JourneySpec, JourneyStep,
    StartStateRef,
};
pub use kernel::KernelHost;
pub use reach::{
    ReachabilityReport, RouteHost, critical_path_journey, human_critical_path_journey, reachability,
};
pub use reduce::{minimize, steps_are_public_input};
pub use run::{JourneyResult, end_capture, run_journey};
pub use script::ScriptHost;
pub use select::{ChangeImpact, JourneyIndex, select};

#[cfg(test)]
mod tests;
