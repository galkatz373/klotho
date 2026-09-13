//! Evaluation failures. Never a kernel fault.

use core::fmt;

use klotho_ir::{AnchorId, Diagnostic, FailureClass, diagnose_journey, diagnose_named};

use crate::ids::JourneyId;

/// Why a journey, evidence seal, or selection failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum EvalError {
    /// Journey could not reach an assertion.
    Unreachable {
        /// Journey.
        journey: JourneyId,
        /// Last reachable semantic state.
        last_state: String,
        /// Affordance or assertion that blocked progress.
        blocked: String,
    },
    /// Evidence was produced against a different project/toolchain/Canon.
    Stale {
        /// Which hash field differed.
        field: String,
    },
    /// Evidence signature did not match the sealed payload.
    BadSignature,
    /// Selection would have dropped a declared dependent.
    Select {
        /// Missing dependent.
        missing: JourneyId,
        /// Selected prerequisite.
        of: JourneyId,
    },
    /// Cycle in the journey dependency graph.
    Cycle(String),
    /// Unknown save slot or capture point.
    Unknown(String),
    /// Journey exceeded `max_ticks`.
    Budget {
        /// Observed ticks.
        used: u32,
        /// Cap.
        cap: u32,
    },
    /// Test agent attempted a Projection write or privileged fact.
    Projection,
    /// Test agent attempted to construct Agency.
    Agency,
    /// Minimized script did not reproduce the original failure.
    Replay,
    /// Underlying kernel/debug fault, escaped as text.
    Host(String),
    /// Capture policy, alignment, or metric plugin failure.
    Capture(String),
    /// Numeric quality gate miss.
    Quality {
        /// Metric id.
        metric: String,
        /// Observed value.
        used: i32,
        /// Inclusive cap or floor.
        cap: i32,
    },
    /// Flake policy refusal (retry-to-green, relabel, quarantine).
    Flake(String),
    /// Farm lane SLO / coverage refusal.
    Lane(String),
}

impl EvalError {
    /// Shared diagnostic envelope.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            Self::Unreachable {
                journey,
                last_state,
                blocked,
            } => diagnose_journey(journey.as_str(), last_state, blocked, self.to_string()),
            Self::Stale { field } => diagnose_named(
                "EVAL.Stale",
                FailureClass::Reproducibility,
                "evidence",
                field,
                self.to_string(),
            ),
            Self::BadSignature => diagnose_named(
                "EVAL.Stale",
                FailureClass::Reproducibility,
                "evidence",
                "signature",
                self.to_string(),
            ),
            Self::Select { missing, .. } => diagnose_named(
                "EVAL.Select",
                FailureClass::Journey,
                "journey",
                missing.as_str(),
                self.to_string(),
            ),
            Self::Cycle(token) => diagnose_named(
                "EVAL.Select",
                FailureClass::Journey,
                "journey",
                token,
                self.to_string(),
            ),
            Self::Unknown(token) => diagnose_named(
                "EVAL.Select",
                FailureClass::Schema,
                "eval",
                token,
                self.to_string(),
            ),
            Self::Budget { .. } => diagnose_named(
                "EVAL.Replay",
                FailureClass::Budget,
                "journey",
                "ticks",
                self.to_string(),
            ),
            Self::Projection => diagnose_named(
                "EVAL.Projection",
                FailureClass::Agency,
                "eval",
                "projection",
                self.to_string(),
            ),
            Self::Agency => diagnose_named(
                "EVAL.Agency",
                FailureClass::Agency,
                "eval",
                "agency",
                self.to_string(),
            ),
            Self::Replay => diagnose_named(
                "EVAL.Replay",
                FailureClass::Journey,
                "journey",
                "replay",
                self.to_string(),
            ),
            Self::Host(token) => diagnose_named(
                "EVAL.Replay",
                FailureClass::Journey,
                "host",
                token,
                self.to_string(),
            ),
            Self::Capture(token) => diagnose_named(
                "EVAL.Capture",
                FailureClass::Quality,
                "capture",
                token,
                self.to_string(),
            ),
            Self::Quality { metric, used, cap } => klotho_ir::diagnose_quality(
                metric,
                *used,
                *cap,
                &[metric.as_str()],
                self.to_string(),
            ),
            Self::Flake(token) => diagnose_named(
                "EVAL.Flake",
                FailureClass::Reproducibility,
                "flake",
                token,
                self.to_string(),
            ),
            Self::Lane(token) => diagnose_named(
                "EVAL.Capture",
                FailureClass::Budget,
                "lane",
                token,
                self.to_string(),
            ),
        }
    }

    /// Primary blame anchor, if any.
    #[must_use]
    pub fn primary(&self) -> AnchorId {
        self.to_diagnostic().primary
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable {
                journey,
                last_state,
                blocked,
            } => write!(
                f,
                "Journey({}->{blocked}) at {}",
                last_state,
                journey.as_str()
            ),
            Self::Stale { field } => write!(f, "StaleEvidence({field})"),
            Self::BadSignature => write!(f, "BadEvidenceSignature"),
            Self::Select { missing, of } => {
                write!(f, "SkippedDependent({}->{})", of.as_str(), missing.as_str())
            }
            Self::Cycle(id) => write!(f, "JourneyCycle({id})"),
            Self::Unknown(id) => write!(f, "Unknown({id})"),
            Self::Budget { used, cap } => write!(f, "JourneyBudget({used}/{cap})"),
            Self::Projection => write!(f, "ProjectionWriteDenied"),
            Self::Agency => write!(f, "AgencyMintDenied"),
            Self::Replay => write!(f, "MinimizeReplayMismatch"),
            Self::Host(s) => write!(f, "EvalHost({s})"),
            Self::Capture(s) => write!(f, "Capture({s})"),
            Self::Quality { metric, used, cap } => {
                write!(f, "Quality({metric} {used}/{cap})")
            }
            Self::Flake(s) => write!(f, "Flake({s})"),
            Self::Lane(s) => write!(f, "Lane({s})"),
        }
    }
}

impl core::error::Error for EvalError {}
