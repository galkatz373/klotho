//! Advisory model-critic findings (K73 / KAI-18).
//!
//! A critic may rank and explain. It cannot change pass/fail, risk, policy,
//! or approval. Findings must cite a capture hash and a style-constitution rule.

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::Name;

/// One advisory finding. Never a merge gate.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriticFinding {
    /// Capture the finding points at.
    pub capture: Hash,
    /// Style-constitution rule id.
    pub rule: Name,
    /// Advisory milliperceptual score. Ignored by [`QualityVerdict::passed`].
    pub score_milli: i32,
    /// Human/agent note. Not runtime truth.
    pub note: String,
}

/// Hard gates plus optional critic notes.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityVerdict {
    /// Trusted numeric / journey / capture gates.
    pub gates: Vec<GateResult>,
    /// Advisory findings. Append-only; never consulted for pass/fail.
    pub critics: Vec<CriticFinding>,
    /// Named human approval recorded by Distaff, never by a critic.
    pub approval: Option<Name>,
}

/// One trusted gate result.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateResult {
    /// Gate id (`ssim`, `foot_slide_mm`, `loc.overflow`).
    pub id: Name,
    /// Whether the trusted tool passed.
    pub passed: bool,
    /// Observed value.
    pub used: i32,
    /// Inclusive cap or floor, depending on the metric.
    pub cap: i32,
}

impl QualityVerdict {
    /// Pass/fail is the conjunction of trusted gates. Critics are ignored.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.gates.iter().all(|gate| gate.passed)
    }

    /// Append an advisory finding. Does not touch gates, risk, or approval.
    pub fn advise(&mut self, finding: CriticFinding) {
        self.critics.push(finding);
    }

    /// Record a named human approval. Critics cannot call this through the agent protocol.
    pub fn approve(&mut self, owner: Name) {
        self.approval = Some(owner);
    }
}
