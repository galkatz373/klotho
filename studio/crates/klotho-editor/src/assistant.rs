//! Distaff's conversational request, semantic review, and grouped Pin model.

use std::collections::BTreeSet;

use klotho_ai::{
    AgentRole, AiError, ArtifactClass, AuthorOp, AuthoringSnapshot, ChangeId, ChangeScope,
    CreativeRequest, DisclosurePolicy, KlothoAi, ModelCapability, RequestBudget, RequestId,
    RiskInput, RiskLevel, RiskPolicy, SemanticDiff, TxBudget, apply_ops,
};
use klotho_author::flatten_bundle;
use klotho_core::Hash;
use klotho_eval::{
    AcceptanceContract, BudgetTarget, ChangeScope as EvaluationScope, InvariantRef, JourneyId,
    QualityTarget, SemanticClaim,
};
use klotho_ir::{AnchorId, Name, to_ron};
use klotho_prove::hash_bytes;

use crate::{EditorError, EditorSession};

/// An explicit assumption shown before generation when it affects behavior.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Assumption {
    /// Human-readable assumption.
    pub text: String,
    /// Whether ambiguity changes game behavior.
    pub affects_behavior: bool,
    /// Explicit designer answer for behavior-affecting assumptions.
    pub accepted: Option<bool>,
}

/// Editable acceptance contract that never exposes source serialization.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct AcceptanceEditor {
    /// Semantic claims.
    pub claims: Vec<SemanticClaim>,
    /// Required journeys.
    pub journeys: Vec<JourneyId>,
    /// Required invariants.
    pub invariants: Vec<InvariantRef>,
    /// Quality references.
    pub quality: Vec<QualityTarget>,
    /// Budget caps.
    pub budgets: Vec<BudgetTarget>,
    /// Non-regression journeys.
    pub non_regression: Vec<JourneyId>,
    /// Allowed semantic scope.
    pub scope: ChangeScope,
}

impl AcceptanceEditor {
    fn contract(&self) -> AcceptanceContract {
        AcceptanceContract {
            claims: self.claims.clone(),
            journeys: self.journeys.clone(),
            invariants: self.invariants.clone(),
            quality: self.quality.clone(),
            budgets: self.budgets.clone(),
            non_regression: self.non_regression.clone(),
            allowed_scope: EvaluationScope {
                modules: self.scope.modules.iter().copied().collect(),
                anchors: self.scope.anchors.iter().copied().collect(),
            },
        }
    }
}

/// Request form presented by Distaff.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct RequestDraft {
    /// Designer request.
    pub text: String,
    /// Visible assumptions.
    pub assumptions: Vec<Assumption>,
    /// Editable evidence contract.
    pub acceptance: AcceptanceEditor,
    /// Requested specialist.
    pub role: AgentRole,
    /// End-to-end cap.
    pub budget: RequestBudget,
}

impl RequestDraft {
    /// Submit only after behavior-affecting assumptions have explicit answers.
    pub fn submit(self, ai: &mut KlothoAi) -> Result<RequestId, EditorError> {
        if self
            .assumptions
            .iter()
            .any(|assumption| assumption.affects_behavior && assumption.accepted != Some(true))
        {
            return Err(EditorError::Ai(AiError::RequestState(
                "behavior-affecting assumption needs a designer answer".into(),
            )));
        }
        let scope = self.acceptance.scope.clone();
        Ok(ai.request(CreativeRequest {
            text: self.text,
            acceptance: self.acceptance.contract(),
            scope,
            transaction_budget: TxBudget::default(),
            budget: self.budget,
            role: self.role,
            model_capability: ModelCapability::Reasoning,
            disclosure: DisclosurePolicy::LocalOnly,
            preferred_backend: None,
            dependencies: Vec::new(),
        })?)
    }
}

/// One semantic plan row. It describes meaning, never a source edit.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PlanStep {
    /// Stable ordinal in the candidate.
    pub index: usize,
    /// Author-facing action.
    pub action: String,
    /// Primary semantic identity.
    pub target: AnchorId,
}

/// Before/after artifact pair at a named journey checkpoint.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CaptureComparison {
    /// Checkpoint name.
    pub checkpoint: Name,
    /// Before artifact hash.
    pub before: Hash,
    /// After artifact hash.
    pub after: Hash,
}

/// Estimated and actual request cost shown in review.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct CostView {
    /// Model and tool wall time.
    pub wall_ms: u64,
    /// Model tokens.
    pub tokens: u64,
    /// Provider cost in millionths of a US dollar.
    pub micro_usd: u64,
    /// Signed project budget delta.
    pub project_budget_delta: i64,
}

/// Exact lineage shown beside a review candidate.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ProvenanceView {
    /// Immutable request hash.
    pub request: Hash,
    /// Transaction base.
    pub base: Hash,
    /// Proposed authoring snapshot.
    pub proposed: Hash,
    /// Trusted evidence attached to the full candidate.
    pub evidence: Vec<Hash>,
}

/// One independently reviewable semantic group.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ReviewGroup {
    /// Author-facing group id.
    pub id: Name,
    /// Operation indexes in this group.
    pub operations: Vec<usize>,
    /// Trusted route for the highest-risk operation.
    pub risk: RiskLevel,
}

/// Review state. Partial selection always requires fresh evidence.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ReviewState {
    /// Candidate has not been accepted or rejected.
    Pending,
    /// Selected subset needs a trusted evaluation rerun.
    EvidenceRequired,
    /// Selected operations are evidence-complete and may be Pinned.
    ReadyToPin,
    /// Pinned into the authoring document.
    Pinned,
    /// Rejected with a human reason.
    Rejected(String),
}

/// Change-centric Distaff review model.
pub struct DistaffReview {
    change: ChangeId,
    diff: SemanticDiff,
    selected: BTreeSet<usize>,
    selection_hash: Hash,
    base: AuthoringSnapshot,
    selected_doc: klotho_ir::IntentDoc,
    full_evidence: Vec<Hash>,
    evidence: Vec<Hash>,
    /// Semantic plan.
    pub plan: Vec<PlanStep>,
    /// Review groups.
    pub groups: Vec<ReviewGroup>,
    /// Before/after captures.
    pub captures: Vec<CaptureComparison>,
    /// Cost display.
    pub cost: CostView,
    /// Request, transaction, and trusted-evidence lineage.
    pub provenance: ProvenanceView,
    /// Current state.
    pub state: ReviewState,
}

impl DistaffReview {
    /// Build a review from a Klotho AI candidate and trusted routing facts.
    pub fn open(
        ai: &KlothoAi,
        request: RequestId,
        change: ChangeId,
        policy: &RiskPolicy,
        risk_inputs: &[RiskInput],
        captures: Vec<CaptureComparison>,
        project_budget_delta: i64,
    ) -> Result<Self, EditorError> {
        let package = ai.review(change)?;
        let (base, proposed) = ai.candidate_snapshots(change)?;
        if risk_inputs.len() != package.diff.ops.len() {
            return Err(EditorError::Ai(AiError::RequestState(
                "every operation needs trusted routing facts".into(),
            )));
        }
        let progress = ai.poll(request)?;
        if progress.change != Some(change) {
            return Err(EditorError::Ai(AiError::RequestState(
                "request and candidate do not match".into(),
            )));
        }
        let plan = package
            .diff
            .ops
            .iter()
            .enumerate()
            .map(|(index, op)| PlanStep {
                index,
                action: action_text(op),
                target: op.primary_anchor(),
            })
            .collect();
        let groups = package
            .diff
            .ops
            .iter()
            .enumerate()
            .map(|(index, _)| ReviewGroup {
                id: Name::new(format!("change_{}", index + 1)).expect("non-empty review group"),
                operations: vec![index],
                risk: policy.route(&risk_inputs[index]),
            })
            .collect();
        let selected: BTreeSet<_> = (0..package.diff.ops.len()).collect();
        let state = if package.evidence.is_empty() {
            ReviewState::EvidenceRequired
        } else {
            ReviewState::ReadyToPin
        };
        let selected_doc = flatten_bundle(&proposed.bundle())?.doc;
        Ok(Self {
            change,
            selection_hash: package.diff.current_hash,
            base,
            selected_doc,
            diff: package.diff.clone(),
            selected,
            full_evidence: package.evidence.clone(),
            evidence: package.evidence.clone(),
            plan,
            groups,
            captures,
            cost: CostView {
                wall_ms: progress.usage.wall_ms,
                tokens: progress.usage.tokens,
                micro_usd: progress.usage.micro_usd,
                project_budget_delta,
            },
            provenance: ProvenanceView {
                request: ai.request_hash(request)?,
                base: package.diff.base_hash,
                proposed: package.diff.current_hash,
                evidence: package.evidence,
            },
            state,
        })
    }

    /// Actual semantic diff; raw expansion remains a deliberate API choice.
    #[must_use]
    pub fn diff(&self) -> &SemanticDiff {
        &self.diff
    }

    /// Evidence hashes bound to the currently selected operation set.
    #[must_use]
    pub fn evidence(&self) -> &[Hash] {
        &self.evidence
    }

    /// Select semantic groups. A proper subset gets a new identity and invalidates evidence.
    pub fn select_groups(&mut self, ids: &[Name]) -> Result<Hash, EditorError> {
        let wanted: BTreeSet<_> = ids.iter().cloned().collect();
        let known: BTreeSet<_> = self.groups.iter().map(|group| group.id.clone()).collect();
        if wanted.is_empty() || !wanted.is_subset(&known) {
            return Err(EditorError::Ai(AiError::RequestState(
                "invalid review group selection".into(),
            )));
        }
        self.selected = self
            .groups
            .iter()
            .filter(|group| wanted.contains(&group.id))
            .flat_map(|group| group.operations.iter().copied())
            .collect();
        if self.selected.len() == self.diff.ops.len() {
            self.selection_hash = self.diff.current_hash;
            let mut snapshot = self.base.clone();
            apply_ops(&mut snapshot, self.change, &self.diff.ops)?;
            self.selected_doc = flatten_bundle(&snapshot.bundle())?.doc;
            self.evidence.clone_from(&self.full_evidence);
            self.state = if self.evidence.is_empty() {
                ReviewState::EvidenceRequired
            } else {
                ReviewState::ReadyToPin
            };
        } else {
            let ops: Vec<_> = self
                .selected
                .iter()
                .map(|index| self.diff.ops[*index].clone())
                .collect();
            let refs: Vec<_> = ops.iter().collect();
            let encoded = to_ron(&(self.change, refs))
                .map_err(|error| EditorError::Ai(AiError::Ser(error.to_string())))?;
            self.selection_hash = hash_bytes(encoded.as_bytes());
            let mut snapshot = self.base.clone();
            apply_ops(&mut snapshot, self.change, &ops)?;
            self.selected_doc = flatten_bundle(&snapshot.bundle())?.doc;
            self.evidence.clear();
            self.state = ReviewState::EvidenceRequired;
        }
        Ok(self.selection_hash)
    }

    /// Attach a trusted rerun to the selected subset.
    pub fn attach_rerun(&mut self, ai: &KlothoAi, evidence: Hash) -> Result<(), EditorError> {
        let record = ai.evaluation.get(evidence)?;
        if record.bundle.change != self.selection_hash
            || record.bundle.project_hash != self.selection_hash
            || record.bundle.checks().is_empty()
            || record.bundle.checks().iter().any(|check| !check.passed)
        {
            return Err(EditorError::Ai(AiError::Evidence(
                "stale or failed partial evidence".into(),
            )));
        }
        self.evidence = vec![evidence];
        self.state = ReviewState::ReadyToPin;
        Ok(())
    }

    /// Pin the selected, evidence-complete semantic operations atomically.
    pub fn pin(
        &mut self,
        session: &mut EditorSession,
        reason: impl Into<String>,
    ) -> Result<(), EditorError> {
        if self.state != ReviewState::ReadyToPin || self.evidence.is_empty() {
            return Err(EditorError::Ai(AiError::RequestState(
                "review is not evidence-complete".into(),
            )));
        }
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(EditorError::Ai(AiError::RequestState(
                "Pin reason is empty".into(),
            )));
        }
        session.pin_ai_document(self.selected_doc.clone(), reason)?;
        self.state = ReviewState::Pinned;
        Ok(())
    }

    /// Reject without mutating the authoring document.
    pub fn reject(&mut self, reason: impl Into<String>) -> Result<(), EditorError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(EditorError::Ai(AiError::RequestState(
                "rejection reason is empty".into(),
            )));
        }
        self.state = ReviewState::Rejected(reason);
        Ok(())
    }

    /// Reject selected semantic groups with a required human reason.
    pub fn reject_groups(
        &mut self,
        ids: &[Name],
        reason: impl Into<String>,
    ) -> Result<(), EditorError> {
        self.select_groups(ids)?;
        self.reject(reason)
    }
}

fn action_text(op: &AuthorOp) -> String {
    match op {
        AuthorOp::AddModule { module } => format!("Add module {}", module.id.as_str()),
        AuthorOp::Instantiate { instance } => {
            format!("Instantiate pattern {}", instance.pattern.as_str())
        }
        AuthorOp::SetArgument { key, .. } => format!("Set pattern option {}", key.as_str()),
        AuthorOp::AddLocus { name, kind, .. } => format!("Add {kind:?} locus {}", name.as_str()),
        AuthorOp::AddFact { .. } => "Add authored fact".into(),
        AuthorOp::AddCanonDiff { .. } => "Change Canon rules".into(),
        AuthorOp::BindAsset { .. } => "Bind approved asset candidate".into(),
        AuthorOp::AddJourney { journey } => format!("Add journey {}", journey.id.as_str()),
        AuthorOp::AddReference { .. } => "Add approved reference".into(),
        AuthorOp::Remove { reason, .. } => format!("Remove authored object: {reason}"),
        AuthorOp::Rename { to, .. } => format!("Rename locus to {}", to.as_str()),
    }
}

/// Default trusted routing facts for a purely semantic editor candidate.
#[must_use]
pub fn conservative_risk_inputs(diff: &SemanticDiff, policy: &RiskPolicy) -> Vec<RiskInput> {
    diff.ops
        .iter()
        .map(|op| RiskInput {
            operation: op.kind(),
            operation_version: 1,
            policy_version: policy.version,
            artifact: ArtifactClass::None,
            classifier_evidence: true,
            semantic_delta: true,
            approved_art_delta: false,
            budget_delta: 0,
            proofs: BTreeSet::new(),
        })
        .collect()
}
