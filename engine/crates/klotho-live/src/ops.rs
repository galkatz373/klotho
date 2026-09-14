//! Network/failure matrices, incident response, and live deployment rehearsal.

use std::collections::BTreeSet;

use klotho_core::Hash;
use serde::{Deserialize, Serialize};

use crate::{LiveError, MultiplayerProfile};

/// One deterministic network-emulation case.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkCase {
    /// Stable case id.
    pub id: String,
    /// Emulated RTT.
    pub rtt_ms: u16,
    /// Packet loss in permille.
    pub loss_permille: u16,
    /// Reordered packets per thousand.
    pub reorder_permille: u16,
    /// Fire age tested against bounded rewind.
    pub fire_age_ticks: u16,
    /// Expected reconnect result.
    pub reconnect_expected: bool,
    /// Whether the public fixture observed the expected deterministic result.
    pub passed: bool,
}

/// Required regional/service failure.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// Matchmaker unavailable in the primary region.
    MatchmakerRegion,
    /// Session service unavailable during reconnect.
    SessionReconnect,
    /// Configuration service returns invalid signature.
    ConfigSignature,
    /// Telemetry sink is unavailable; gameplay must continue.
    TelemetrySink,
    /// Moderation queue is delayed but retains the report commitment.
    ModerationQueue,
}

/// Versioned P0 network and core failure matrix.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkMatrix {
    /// Exact public matrix version.
    pub version: u32,
    /// Network cases.
    pub cases: Vec<NetworkCase>,
    /// Required service failures rehearsed successfully.
    pub failures: Vec<FailureKind>,
}

/// Matrix gate summary.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct MatrixReport {
    /// Number of passing network cases.
    pub passed_cases: usize,
    /// Maximum observed RTT.
    pub max_rtt_ms: u16,
    /// Maximum observed loss.
    pub max_loss_permille: u16,
    /// Required failure kinds covered.
    pub failures: BTreeSet<FailureKind>,
}

impl NetworkMatrix {
    /// Validate the matrix against the approved title profile.
    pub fn evaluate(&self, profile: &MultiplayerProfile) -> Result<MatrixReport, LiveError> {
        profile.validate()?;
        if self.version == 0 || self.cases.is_empty() {
            return Err(LiveError::Gate("network matrix is empty".into()));
        }
        let mut ids = BTreeSet::new();
        let mut max_rtt = 0;
        let mut max_loss = 0;
        for case in &self.cases {
            if case.id.trim().is_empty()
                || !ids.insert(&case.id)
                || case.rtt_ms > profile.max_rtt_ms
                || case.loss_permille > profile.max_loss_permille
                || case.loss_permille > 1_000
                || case.reorder_permille > 1_000
                || case.fire_age_ticks > profile.rewind_ticks.saturating_add(1)
                || !case.passed
            {
                return Err(LiveError::Gate(format!("network case {} failed", case.id)));
            }
            max_rtt = max_rtt.max(case.rtt_ms);
            max_loss = max_loss.max(case.loss_permille);
        }
        let failures: BTreeSet<_> = self.failures.iter().copied().collect();
        let required = [
            FailureKind::MatchmakerRegion,
            FailureKind::SessionReconnect,
            FailureKind::ConfigSignature,
            FailureKind::TelemetrySink,
            FailureKind::ModerationQueue,
        ];
        if required.iter().any(|kind| !failures.contains(kind)) {
            return Err(LiveError::Gate(
                "service failure matrix is incomplete".into(),
            ));
        }
        Ok(MatrixReport {
            passed_cases: self.cases.len(),
            max_rtt_ms: max_rtt,
            max_loss_permille: max_loss,
            failures,
        })
    }
}

/// Human-owned incident response instructions.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentRunbook {
    /// Detection and severity assignment.
    pub detect: String,
    /// Player communication.
    pub communicate: String,
    /// Service containment.
    pub contain: String,
    /// Config/epoch rollback.
    pub rollback: String,
    /// Support escalation.
    pub support: String,
}

impl IncidentRunbook {
    /// Require every operational branch.
    pub fn validate(&self) -> Result<(), LiveError> {
        if [
            &self.detect,
            &self.communicate,
            &self.contain,
            &self.rollback,
            &self.support,
        ]
        .iter()
        .any(|step| step.trim().is_empty())
        {
            return Err(LiveError::Deploy("incident runbook is incomplete".into()));
        }
        Ok(())
    }
}

/// Staged service/config deployment with one whole-candidate rollback target.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Deployment {
    /// Candidate hash.
    pub candidate: Hash,
    /// Previous candidate.
    pub previous: Hash,
    /// Strictly increasing percentages ending at 100.
    pub stages: Vec<u8>,
    /// Current stage index.
    pub index: usize,
    /// Frozen after rollback or failed gate.
    pub frozen: bool,
}

impl Deployment {
    /// Begin a staged deployment after runbook validation.
    pub fn start(
        candidate: Hash,
        previous: Hash,
        stages: Vec<u8>,
        runbook: &IncidentRunbook,
    ) -> Result<Self, LiveError> {
        runbook.validate()?;
        if candidate == Hash::ZERO
            || previous == Hash::ZERO
            || candidate == previous
            || stages.is_empty()
            || stages[0] == 0
            || stages.windows(2).any(|window| window[0] >= window[1])
            || stages.last() != Some(&100)
        {
            return Err(LiveError::Deploy("deployment envelope is invalid".into()));
        }
        Ok(Self {
            candidate,
            previous,
            stages,
            index: 0,
            frozen: false,
        })
    }

    /// Advance only after the current stage's hard gates pass.
    pub fn advance(&mut self, hard_gates_passed: bool) -> Result<u8, LiveError> {
        if !hard_gates_passed {
            self.frozen = true;
            return Err(LiveError::Deploy("deployment hard gate failed".into()));
        }
        if self.frozen {
            return Err(LiveError::Deploy("deployment is frozen".into()));
        }
        let next = self
            .index
            .checked_add(1)
            .filter(|index| *index < self.stages.len())
            .ok_or_else(|| LiveError::Deploy("deployment is complete".into()))?;
        self.index = next;
        Ok(self.stages[next])
    }

    /// Restore the entire previous candidate and freeze deployment.
    pub fn rollback(&mut self) -> Hash {
        self.candidate = self.previous;
        self.frozen = true;
        self.index = 0;
        self.candidate
    }
}

/// Recorded deployment/incident rehearsal.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rehearsal {
    /// Stable rehearsal id.
    pub id: String,
    /// Staged deploy passed.
    pub staged_deploy: bool,
    /// Whole-candidate rollback passed.
    pub rollback: bool,
    /// Regional failover passed.
    pub regional_failover: bool,
    /// Support handoff passed.
    pub support_handoff: bool,
}

/// P0 public gate. Private service/security/moderation/scale evidence is absent.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PublicGate {
    /// Network matrix report.
    pub matrix: MatrixReport,
    /// Rehearsals evaluated.
    pub rehearsals: usize,
}

impl PublicGate {
    /// Require all public deterministic and operations rehearsals.
    pub fn evaluate(matrix: MatrixReport, rehearsals: &[Rehearsal]) -> Result<Self, LiveError> {
        if rehearsals.is_empty()
            || rehearsals.iter().any(|rehearsal| {
                rehearsal.id.trim().is_empty()
                    || !rehearsal.staged_deploy
                    || !rehearsal.rollback
                    || !rehearsal.regional_failover
                    || !rehearsal.support_handoff
            })
        {
            return Err(LiveError::Gate("live rehearsal failed".into()));
        }
        Ok(Self {
            matrix,
            rehearsals: rehearsals.len(),
        })
    }
}
