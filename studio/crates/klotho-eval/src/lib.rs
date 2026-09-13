//! Journeys, public-input execution, evidence bundles, and affected-test selection.
//!
//! The library does not enable `klotho-world/mutate`, append Trace, or mint
//! [`klotho_ir::Agency`]. Kernel-backed runs go through
//! [`klotho_debug::JourneyKernel`].
//!
//! KAI-18 adds aligned multimodal capture evaluation, pinned SSIM/LPIPS
//! plugins, farm-derived lane SLOs, and infrastructure flake policy.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod a11y;
mod bot;
mod capture;
mod contract;
mod critic;
mod dialogue;
mod error;
mod evidence;
mod flake;
mod host;
mod ids;
mod journey;
mod kernel;
mod lane;
mod metrics;
mod quality;
mod reach;
mod reduce;
mod run;
mod script;
mod select;

pub use a11y::{FocusHost, capture_matrix_gate, focus_journey, human_focus_journey, remap_journey};
pub use bot::{AutomatedPlayer, BotManifest};
pub use capture::{
    BackendBaseline, CaptureDelta, CapturePolicy, CaptureSet, CaptureView, compare_captures,
};
pub use contract::{
    AcceptanceContract, BudgetTarget, ChangeScope, InvariantRef, QualityTarget, SemanticClaim,
};
pub use critic::{CriticFinding, GateResult, QualityVerdict};
pub use dialogue::{ConversationHost, conversation_journey};
pub use error::EvalError;
pub use evidence::{
    ApprovalRef, ArtifactRef, CheckEvidence, CheckLayer, EvidenceBuilder, EvidenceBundle,
    EvidenceContext,
};
pub use flake::{Attempt, AttemptKind, FlakePolicy, FlakeQuarantine, InfraFault};
pub use host::{JourneyHost, StepOutcome};
pub use ids::JourneyId;
pub use journey::{
    CaptureKind, CapturePoint, DeviceAction, JourneyAssertion, JourneySpec, JourneyStep,
    StartStateRef,
};
pub use kernel::KernelHost;
pub use lane::{Coverage, EvalTier, FarmInventory, LaneSlo, derive_slo};
pub use metrics::{
    LPIPS_ID, MetricLock, MetricPlugin, RgbaFrame, SSIM_ID, lpips_milli, luma, ssim_milli,
};
pub use quality::{
    CUT_CAP_MM, HERO_HALF_D_MM, HERO_HALF_W_MM, LPIPS_CEILING_MILLI, SSIM_FLOOR_MILLI,
    animation_gates, audio_gates, camera_gates, loc_a11y_cell, loc_a11y_gates, test_frame,
    visual_gates,
};
pub use reach::{
    ReachabilityReport, RouteHost, critical_path_journey, human_critical_path_journey, reachability,
};
pub use reduce::{minimize, steps_are_public_input};
pub use run::{JourneyResult, end_capture, run_journey};
pub use script::ScriptHost;
pub use select::{ChangeImpact, JourneyIndex, select};

#[cfg(test)]
mod kai18_tests;
#[cfg(test)]
mod tests;
