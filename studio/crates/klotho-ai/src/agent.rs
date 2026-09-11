//! Budgeted, resumable authoring request scheduler.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use klotho_core::Hash;
use klotho_eval::AcceptanceContract as EvaluationContract;
use klotho_prove::hash_bytes;

use crate::ids::{ChangeId, TxId};
use crate::model::{BackendId, ModelCapability};
use crate::ops::{ChangeScope, TxBudget};
use crate::policy::DisclosurePolicy;

/// Stable request identity derived without RNG.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct RequestId(pub [u8; 16]);

impl RequestId {
    /// Derive from canonical request bytes and a monotonic scheduler sequence.
    #[must_use]
    pub fn derive(bytes: &[u8]) -> Self {
        let mut input = b"klotho-ai-request-v1".to_vec();
        input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        input.extend_from_slice(bytes);
        let hash = hash_bytes(&input);
        let mut id = [0; 16];
        id.copy_from_slice(&hash.0[..16]);
        Self(id)
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for RequestId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() != 32 {
            return Err(serde::de::Error::custom(
                "request id must be 32 hex characters",
            ));
        }
        let mut bytes = [0; 16];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let pair = std::str::from_utf8(pair).map_err(serde::de::Error::custom)?;
            bytes[index] = u8::from_str_radix(pair, 16).map_err(serde::de::Error::custom)?;
        }
        Ok(Self(bytes))
    }
}

/// Built-in worker roles.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// Acceptance and operation planning.
    Planner,
    /// Gameplay semantic operations.
    Gameplay,
    /// Place and world authoring.
    World,
    /// Dialogue and narrative authoring.
    Narrative,
    /// Typed asset requests.
    Asset,
    /// Journey authoring and execution choice.
    Test,
    /// Cook specialization advice.
    Optimizer,
    /// Advisory quality findings.
    Critic,
}

impl AgentRole {
    /// Complete role set.
    pub const ALL: [Self; 8] = [
        Self::Planner,
        Self::Gameplay,
        Self::World,
        Self::Narrative,
        Self::Asset,
        Self::Test,
        Self::Optimizer,
        Self::Critic,
    ];
}

/// Hard request caps. Wall time is driven by the host's monotonic clock.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestBudget {
    /// Wall-clock allowance.
    pub wall_ms: u64,
    /// Combined model-token allowance.
    pub tokens: u64,
    /// Provider cost allowance in millionths of a US dollar.
    pub micro_usd: u64,
    /// Typed tool-call allowance.
    pub tool_calls: u32,
    /// Candidate artifact allowance.
    pub artifacts: u32,
    /// Repair attempts per stage.
    pub repairs: u8,
}

impl Default for RequestBudget {
    fn default() -> Self {
        Self {
            wall_ms: 15 * 60 * 1_000,
            tokens: 64_000,
            micro_usd: 5_000_000,
            tool_calls: 64,
            artifacts: 16,
            repairs: 3,
        }
    }
}

/// Accounted resource use.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    /// Elapsed monotonic milliseconds while active.
    pub wall_ms: u64,
    /// Model tokens.
    pub tokens: u64,
    /// Provider cost.
    pub micro_usd: u64,
    /// Tool calls.
    pub tool_calls: u32,
    /// Produced artifacts.
    pub artifacts: u32,
    /// Repair attempts.
    pub repairs: u8,
}

impl Usage {
    /// True if any cap is exceeded.
    #[must_use]
    pub fn exceeds(&self, cap: &RequestBudget) -> bool {
        self.wall_ms > cap.wall_ms
            || self.tokens > cap.tokens
            || self.micro_usd > cap.micro_usd
            || self.tool_calls > cap.tool_calls
            || self.artifacts > cap.artifacts
            || self.repairs > cap.repairs
    }
}

/// One creative request. Natural language is not authoritative.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreativeRequest {
    /// Request text.
    pub text: String,
    /// Required acceptance contract; empty contracts are rejected.
    pub acceptance: EvaluationContract,
    /// Writable authoring scope.
    pub scope: ChangeScope,
    /// Isolated transaction operation cap.
    pub transaction_budget: TxBudget,
    /// End-to-end caps.
    pub budget: RequestBudget,
    /// Requested worker role.
    pub role: AgentRole,
    /// Required modality.
    pub model_capability: ModelCapability,
    /// Data disclosure rule.
    pub disclosure: DisclosurePolicy,
    /// Optional model preference.
    pub preferred_backend: Option<BackendId>,
    /// Requests that must already have candidates before this one runs.
    pub dependencies: Vec<RequestId>,
}

/// Request lifecycle.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    /// Queued for context/model work.
    Queued,
    /// Host intentionally paused; transaction remains resumable.
    Paused,
    /// Model/tool work is active.
    Running,
    /// Reviewable isolated candidate exists.
    Candidate,
    /// Cancelled without touching the live project.
    Cancelled,
    /// Timed out; may resume with remaining non-wall budgets reset explicitly.
    TimedOut,
    /// Failed closed.
    Failed(String),
}

/// Persistent scheduler row.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestRecord {
    /// Identity.
    pub id: RequestId,
    /// Immutable input.
    pub request: CreativeRequest,
    /// Current state.
    pub state: RequestState,
    /// Accounted use.
    pub usage: Usage,
    /// Transaction, after branch creation.
    pub transaction: Option<TxId>,
    /// Change, after branch creation.
    pub change: Option<ChangeId>,
    /// Backend actually used.
    pub backend: Option<BackendId>,
    /// Monotonic host timestamp at the latest start/resume.
    pub active_since_ms: Option<u64>,
    /// Human-facing, non-authoritative progress note.
    pub summary: String,
}

/// Poll result.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct AiProgress {
    /// Request.
    pub request: RequestId,
    /// State.
    pub state: RequestState,
    /// Accounted use.
    pub usage: Usage,
    /// Candidate change if present.
    pub change: Option<ChangeId>,
    /// Progress note.
    pub summary: String,
}

/// Deterministic request ledger and anchor-ownership guard.
#[derive(Default)]
pub struct AgentScheduler {
    sequence: u64,
    records: BTreeMap<RequestId, RequestRecord>,
    owners: BTreeMap<klotho_ir::AnchorId, RequestId>,
}

impl AgentScheduler {
    /// Enqueue a request and claim every explicitly scoped anchor.
    pub fn enqueue(
        &mut self,
        request: CreativeRequest,
    ) -> Result<RequestId, crate::error::AiError> {
        let acceptance_empty = request.acceptance.claims.is_empty()
            && request.acceptance.journeys.is_empty()
            && request.acceptance.invariants.is_empty()
            && request.acceptance.quality.is_empty()
            && request.acceptance.budgets.is_empty()
            && request.acceptance.non_regression.is_empty();
        if request.text.trim().is_empty()
            || acceptance_empty
            || request
                .acceptance
                .claims
                .iter()
                .any(|claim| claim.id.as_str().is_empty() || claim.text.trim().is_empty())
        {
            return Err(crate::error::AiError::MissingAcceptance);
        }
        let accepted = &request.acceptance.allowed_scope;
        let acceptance_unrestricted = accepted.modules.is_empty() && accepted.anchors.is_empty();
        let request_unrestricted =
            request.scope.modules.is_empty() && request.scope.anchors.is_empty();
        if !acceptance_unrestricted
            && (request_unrestricted
                || request
                    .scope
                    .modules
                    .iter()
                    .any(|id| !accepted.modules.contains(id))
                || request
                    .scope
                    .anchors
                    .iter()
                    .any(|id| !accepted.anchors.contains(id)))
        {
            return Err(crate::error::AiError::ContractScopeMismatch);
        }
        for dependency in &request.dependencies {
            if !self.records.contains_key(dependency) {
                return Err(crate::error::AiError::UnknownRequest(*dependency));
            }
        }
        self.sequence = self.sequence.saturating_add(1);
        let mut bytes = klotho_ir::to_ron(&request)
            .map_err(|e| crate::error::AiError::Ser(e.to_string()))?
            .into_bytes();
        bytes.extend_from_slice(&self.sequence.to_le_bytes());
        let id = RequestId::derive(&bytes);
        let anchors: BTreeSet<_> = request.scope.anchors.iter().copied().collect();
        for anchor in &anchors {
            if let Some(owner) = self.owners.get(anchor) {
                return Err(crate::error::AiError::Ownership {
                    anchor: *anchor,
                    by: *owner,
                });
            }
        }
        for anchor in anchors {
            self.owners.insert(anchor, id);
        }
        self.records.insert(
            id,
            RequestRecord {
                id,
                request,
                state: RequestState::Queued,
                usage: Usage::default(),
                transaction: None,
                change: None,
                backend: None,
                active_since_ms: None,
                summary: String::new(),
            },
        );
        Ok(id)
    }

    /// Borrow a row.
    pub fn get(&self, id: RequestId) -> Option<&RequestRecord> {
        self.records.get(&id)
    }

    /// Mutably borrow a row for the service driver.
    pub(crate) fn get_mut(&mut self, id: RequestId) -> Option<&mut RequestRecord> {
        self.records.get_mut(&id)
    }

    /// Find a candidate by change identity.
    pub(crate) fn find_change(&self, change: ChangeId) -> Option<&RequestRecord> {
        self.records
            .values()
            .find(|record| record.change == Some(change))
    }

    /// Whether every declared dependency has produced a candidate.
    pub(crate) fn dependencies_ready(&self, id: RequestId) -> Result<bool, crate::error::AiError> {
        let row = self
            .records
            .get(&id)
            .ok_or(crate::error::AiError::UnknownRequest(id))?;
        Ok(row.request.dependencies.iter().all(|dependency| {
            self.records
                .get(dependency)
                .is_some_and(|record| matches!(record.state, RequestState::Candidate))
        }))
    }

    /// Release semantic ownership after a terminal state.
    pub(crate) fn release(&mut self, id: RequestId) {
        self.owners.retain(|_, owner| *owner != id);
    }
}

/// Hash the immutable request for provenance.
pub(crate) fn request_hash(request: &CreativeRequest) -> Result<Hash, crate::error::AiError> {
    let text = klotho_ir::to_ron(request)
        .map_err(|error| crate::error::AiError::Ser(error.to_string()))?;
    Ok(hash_bytes(text.as_bytes()))
}
