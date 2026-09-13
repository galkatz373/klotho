//! Infrastructure flake policy (KAI-18).
//!
//! A comparison or quality failure cannot be retried to green. Only a recorded
//! infrastructure fault may retry, and only within the farm budget. Known
//! flakes cannot be reclassified after the first attempt.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use klotho_ir::Name;

use crate::error::EvalError;

/// Recorded infrastructure fault. Comparison failures are not in this set.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfraFault {
    /// GPU reset / TDR.
    GpuReset,
    /// Worker exceeded its wall-clock slot.
    WorkerTimeout,
    /// CAS temporarily unavailable.
    CasUnavailable,
    /// Machine lost its exclusive login.
    HostPreempted,
}

/// Why an attempt finished.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptKind {
    /// Recorded infrastructure fault.
    Infrastructure(InfraFault),
    /// SSIM/LPIPS or pixel histogram comparison failed.
    Comparison,
    /// Domain quality gate failed.
    QualityGate,
}

/// One evaluation attempt. The original kind is frozen.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    /// Kind recorded at first observation. Later labels cannot replace this.
    pub kind: AttemptKind,
    /// Attempt index starting at 0.
    pub index: u32,
}

/// Farm retry and quarantine policy.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakePolicy {
    /// Maximum infrastructure retries (kai-farm-a: 2).
    pub max_infra_retries: u32,
    /// Faults the farm is allowed to retry.
    pub known_faults: BTreeSet<InfraFault>,
    /// Named owner of a quarantined flake, if any.
    pub quarantine_owner: Option<Name>,
    /// Inclusive expiry sequence. Zero means no quarantine.
    pub quarantine_expires_at: u64,
}

impl Default for FlakePolicy {
    fn default() -> Self {
        Self::kai_farm_a()
    }
}

impl FlakePolicy {
    /// Checked-in kai-farm-a retry policy.
    #[must_use]
    pub fn kai_farm_a() -> Self {
        Self {
            max_infra_retries: 2,
            known_faults: [
                InfraFault::GpuReset,
                InfraFault::WorkerTimeout,
                InfraFault::CasUnavailable,
                InfraFault::HostPreempted,
            ]
            .into_iter()
            .collect(),
            quarantine_owner: None,
            quarantine_expires_at: 0,
        }
    }

    /// `true` when this attempt may run again. Comparison/quality never retry.
    pub fn may_retry(&self, attempts: &[Attempt], now: u64) -> Result<(), EvalError> {
        let Some(last) = attempts.last() else {
            return Ok(());
        };
        match last.kind {
            AttemptKind::Comparison | AttemptKind::QualityGate => Err(EvalError::Flake(
                "comparison or quality failure cannot retry-to-green".into(),
            )),
            AttemptKind::Infrastructure(fault) => {
                if !self.known_faults.contains(&fault) {
                    return Err(EvalError::Flake(
                        "unknown infrastructure fault is not retryable".into(),
                    ));
                }
                let infra = attempts
                    .iter()
                    .filter(|a| matches!(a.kind, AttemptKind::Infrastructure(_)))
                    .count() as u32;
                if infra > self.max_infra_retries {
                    return Err(EvalError::Flake(
                        "infrastructure retry budget exhausted".into(),
                    ));
                }
                if self.quarantine_expires_at != 0 && now > self.quarantine_expires_at {
                    return Err(EvalError::Flake("flake quarantine expired".into()));
                }
                Ok(())
            }
        }
    }

    /// Reclassifying a recorded comparison/quality failure as infrastructure is denied.
    pub fn relabel(&self, original: &AttemptKind, proposed: &AttemptKind) -> Result<(), EvalError> {
        if original == proposed {
            return Ok(());
        }
        match original {
            AttemptKind::Comparison | AttemptKind::QualityGate => Err(EvalError::Flake(
                "cannot relabel a quality or comparison failure as infrastructure".into(),
            )),
            AttemptKind::Infrastructure(_) => {
                if matches!(proposed, AttemptKind::Comparison | AttemptKind::QualityGate) {
                    Ok(())
                } else {
                    Err(EvalError::Flake(
                        "infrastructure fault relabel is owner-only".into(),
                    ))
                }
            }
        }
    }
}

/// Quarantine record. Release-critical gates cannot be quarantined.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakeQuarantine {
    /// Reproducible infrastructure signature.
    pub signature: Name,
    /// Linked issue id.
    pub issue: Name,
    /// Named owner.
    pub owner: Name,
    /// Inclusive expiry sequence; must be ≤ 7 days of farm ticks.
    pub expires_at: u64,
    /// Gate that failed.
    pub gate: Name,
}

impl FlakeQuarantine {
    /// Release-critical semantic/package/rights/privacy/save/migration gates stay red.
    pub fn admit(gate: &str, expires_at: u64, now: u64, max_expiry: u64) -> Result<(), EvalError> {
        const BLOCKED: &[&str] = &[
            "semantic",
            "package",
            "rights",
            "privacy",
            "save",
            "migration",
        ];
        if BLOCKED.contains(&gate) {
            return Err(EvalError::Flake(
                "release-critical gate cannot be quarantined".into(),
            ));
        }
        if expires_at <= now || expires_at > max_expiry {
            return Err(EvalError::Flake("quarantine expiry out of bounds".into()));
        }
        Ok(())
    }
}
