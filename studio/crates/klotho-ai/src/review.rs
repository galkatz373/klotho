//! Trusted review routing, sampling, and owner-capacity policy (K91).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::{AnchorId, Name, to_ron};
use klotho_prove::hash_bytes;

use crate::{ChangeId, OpKind, RequestId};

/// Human review severity assigned by trusted policy.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Exact mechanical allowlist entry with its required proofs.
    R0,
    /// Homogeneous approved dressing eligible for sampled review.
    R1,
    /// Discipline-owner batch review.
    R2,
    /// Item-level named-owner approval.
    R3,
}

/// Artifact class used by the trusted risk router.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactClass {
    /// No authored artifact changes.
    None,
    /// Approved non-semantic dressing inside an approved zone.
    ApprovedDressing,
    /// New non-hero content.
    NonHero,
    /// Gameplay-visible collision, sockets, navigation, or traversal geometry.
    SemanticGeometry,
    /// Hero asset or story-canon content.
    Critical,
    /// Classifier did not produce a known class.
    Unknown,
}

/// Facts supplied to the router by trusted validators, never by a model.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskInput {
    /// Operation kind.
    pub operation: OpKind,
    /// Exact semantic-operation schema version.
    pub operation_version: u32,
    /// Policy version the caller evaluated against.
    pub policy_version: u32,
    /// Artifact class from the trusted classifier.
    pub artifact: ArtifactClass,
    /// Whether classifier evidence is present and valid.
    pub classifier_evidence: bool,
    /// Whether the operation changes semantic content.
    pub semantic_delta: bool,
    /// Whether approved art changes.
    pub approved_art_delta: bool,
    /// Signed budget delta; positive means more expensive.
    pub budget_delta: i64,
    /// Proof identifiers produced by trusted checks.
    pub proofs: BTreeSet<Name>,
}

/// One exact R0 allowlist row.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct R0Rule {
    /// Operation kind.
    pub operation: OpKind,
    /// Exact operation version.
    pub operation_version: u32,
    /// Proofs required for mechanical acceptance.
    pub required_proofs: BTreeSet<Name>,
}

/// Exhaustive trusted risk policy. Unknown inputs route to R3.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskPolicy {
    /// Policy version.
    pub version: u32,
    /// Exact R0 allowlist.
    pub r0: BTreeSet<R0Rule>,
}

impl Default for RiskPolicy {
    fn default() -> Self {
        Self {
            version: 1,
            r0: BTreeSet::new(),
        }
    }
}

impl RiskPolicy {
    /// Route one operation. Missing or mismatched facts fail upward to R3.
    #[must_use]
    pub fn route(&self, input: &RiskInput) -> RiskLevel {
        if input.policy_version != self.version
            || !input.classifier_evidence
            || matches!(input.artifact, ArtifactClass::Unknown)
        {
            return RiskLevel::R3;
        }
        if !input.semantic_delta
            && !input.approved_art_delta
            && input.budget_delta <= 0
            && self.r0.iter().any(|rule| {
                rule.operation == input.operation
                    && rule.operation_version == input.operation_version
                    && rule.required_proofs.is_subset(&input.proofs)
            })
        {
            return RiskLevel::R0;
        }
        if matches!(
            input.operation,
            OpKind::AddCanonDiff | OpKind::Instantiate | OpKind::SetArgument
        ) || matches!(
            input.artifact,
            ArtifactClass::SemanticGeometry | ArtifactClass::Critical
        ) {
            return RiskLevel::R3;
        }
        if input.operation == OpKind::BindAsset
            && input.artifact == ArtifactClass::ApprovedDressing
            && !input.semantic_delta
            && input.budget_delta <= 0
        {
            return RiskLevel::R1;
        }
        RiskLevel::R2
    }
}

/// Immutable item in a frozen R1 review population.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchItem {
    /// Changed semantic identity.
    pub anchor: AnchorId,
    /// Exact semantic operation hash.
    pub operation_hash: Hash,
    /// Sealed evidence hash.
    pub evidence_hash: Hash,
    /// Transaction lineage.
    pub change: ChangeId,
    /// Originating request; related changes cannot be silently split.
    pub request: RequestId,
}

/// Sampling parameters approved before population freeze.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplingPolicy {
    /// Trusted risk-policy version.
    pub policy_version: u32,
    /// At least this many items are reviewed.
    pub minimum: u32,
    /// Additional population percentage, rounded upward.
    pub rate_percent: u8,
    /// Named discipline owner.
    pub owner: Name,
    /// Sequence deadline after which evidence expires.
    pub expires_at: u64,
}

/// Frozen canonical population. Selection requires a later reviewer nonce.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenBatch {
    /// Policy.
    pub policy: SamplingPolicy,
    /// Canonically sorted immutable population.
    pub items: Vec<BatchItem>,
    /// Root over the frozen population.
    pub batch_root: Hash,
    /// Hash of the complete manifest.
    pub manifest_hash: Hash,
}

impl FrozenBatch {
    /// Freeze a non-empty R1 population before a reviewer supplies a nonce.
    pub fn freeze(
        mut items: Vec<BatchItem>,
        policy: SamplingPolicy,
    ) -> Result<Self, crate::AiError> {
        if items.is_empty() || policy.minimum == 0 || policy.rate_percent > 100 {
            return Err(crate::AiError::RequestState(
                "invalid sample population or policy".into(),
            ));
        }
        items.sort();
        if items.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(crate::AiError::RequestState(
                "duplicate sample population item".into(),
            ));
        }
        let encoded = to_ron(&(policy.policy_version, &items))
            .map_err(|error| crate::AiError::Ser(error.to_string()))?;
        let batch_root = hash_bytes(encoded.as_bytes());
        let manifest = to_ron(&(&policy, &items, batch_root))
            .map_err(|error| crate::AiError::Ser(error.to_string()))?;
        Ok(Self {
            policy,
            items,
            batch_root,
            manifest_hash: hash_bytes(manifest.as_bytes()),
        })
    }

    /// Select the lowest content-hash ranks using a fresh reviewer nonce.
    pub fn select(
        &self,
        reviewer: Name,
        reviewer_nonce: &[u8],
        now: u64,
    ) -> Result<SampleRecord, crate::AiError> {
        if reviewer.as_str().is_empty() || reviewer_nonce.len() < 16 || now > self.policy.expires_at
        {
            return Err(crate::AiError::RequestState(
                "invalid reviewer nonce or expired batch".into(),
            ));
        }
        let percent = (self.items.len() * usize::from(self.policy.rate_percent)).div_ceil(100);
        let count = usize::try_from(self.policy.minimum)
            .unwrap_or(usize::MAX)
            .max(percent)
            .min(self.items.len());
        let mut ranked: Vec<(Hash, usize)> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut bytes = b"klotho-review-v1".to_vec();
                bytes.extend_from_slice(&self.batch_root.0);
                bytes.extend_from_slice(reviewer_nonce);
                bytes.extend_from_slice(&item.anchor.0);
                bytes.extend_from_slice(&item.operation_hash.0);
                (hash_bytes(&bytes), index)
            })
            .collect();
        ranked.sort_by_key(|(rank, index)| (*rank, *index));
        let selected = ranked
            .into_iter()
            .take(count)
            .map(|(_, index)| index)
            .collect();
        Ok(SampleRecord {
            manifest_hash: self.manifest_hash,
            policy_version: self.policy.policy_version,
            reviewer,
            reviewer_nonce_hash: hash_bytes(reviewer_nonce),
            selected,
            disposition: SampleDisposition::Pending,
        })
    }
}

/// Whole-batch result. A single sample failure escalates the population.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleDisposition {
    /// Awaiting reviews.
    Pending,
    /// Every selected item passed.
    Approved,
    /// At least one selected item failed; whole batch is R3.
    Escalated,
}

/// Auditable deterministic selection record.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRecord {
    /// Frozen population manifest.
    pub manifest_hash: Hash,
    /// Policy version.
    pub policy_version: u32,
    /// Named reviewer.
    pub reviewer: Name,
    /// Hash of the nonce (the audit may store the nonce separately).
    pub reviewer_nonce_hash: Hash,
    /// Selected indexes in the frozen canonical population.
    pub selected: Vec<usize>,
    /// Whole-batch disposition.
    pub disposition: SampleDisposition,
}

impl SampleRecord {
    /// Recompute selection. A hand-edited `selected` list fails closed.
    pub fn reproduce(
        &self,
        batch: &FrozenBatch,
        reviewer: Name,
        reviewer_nonce: &[u8],
        now: u64,
    ) -> Result<(), crate::AiError> {
        let expected = batch.select(reviewer, reviewer_nonce, now)?;
        if expected.selected != self.selected
            || expected.manifest_hash != self.manifest_hash
            || expected.policy_version != self.policy_version
        {
            return Err(crate::AiError::RequestState(
                "sample record does not reproduce from frozen population".into(),
            ));
        }
        Ok(())
    }

    /// Record selected-item results. Count mismatch fails closed.
    pub fn decide(&mut self, passed: &[bool]) -> Result<SampleDisposition, crate::AiError> {
        if passed.len() != self.selected.len() {
            return Err(crate::AiError::RequestState(
                "sample result count mismatch".into(),
            ));
        }
        self.disposition = if passed.iter().all(|passed| *passed) {
            SampleDisposition::Approved
        } else {
            SampleDisposition::Escalated
        };
        Ok(self.disposition)
    }
}

/// Signed audit of a frozen-population sample. The nonce is part of the seal.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleAudit {
    /// Frozen population manifest.
    pub manifest_hash: Hash,
    /// Policy version.
    pub policy_version: u32,
    /// Named reviewer.
    pub reviewer: Name,
    /// Fresh reviewer nonce (audit storage, not runtime RNG).
    pub reviewer_nonce: Vec<u8>,
    /// Ranking algorithm identity.
    pub ranking: Name,
    /// Selected indexes.
    pub selected: Vec<usize>,
    /// Whole-batch disposition.
    pub disposition: SampleDisposition,
    /// Sequence time the audit was sealed.
    pub signed_at: u64,
    /// `evidence_signature` of the unsigned payload.
    pub signature: Hash,
}

#[derive(Serialize)]
struct UnsignedSampleAudit<'a> {
    manifest_hash: Hash,
    policy_version: u32,
    reviewer: &'a Name,
    reviewer_nonce: &'a [u8],
    ranking: &'a Name,
    selected: &'a [usize],
    disposition: SampleDisposition,
    signed_at: u64,
}

impl SampleAudit {
    /// Seal a sample after selection. The nonce is committed here, not before freeze.
    pub fn seal(
        batch: &FrozenBatch,
        record: &SampleRecord,
        reviewer_nonce: &[u8],
        signed_at: u64,
    ) -> Result<Self, crate::AiError> {
        batch.verify()?;
        record.reproduce(batch, record.reviewer.clone(), reviewer_nonce, signed_at)?;
        if signed_at > batch.policy.expires_at {
            return Err(crate::AiError::RequestState("sample audit expired".into()));
        }
        refuse_predictable_nonce(batch, reviewer_nonce)?;
        let mut audit = Self {
            manifest_hash: record.manifest_hash,
            policy_version: record.policy_version,
            reviewer: record.reviewer.clone(),
            reviewer_nonce: reviewer_nonce.to_vec(),
            ranking: Name::from("blake3-klotho-review-v1"),
            selected: record.selected.clone(),
            disposition: record.disposition,
            signed_at,
            signature: Hash::ZERO,
        };
        audit.signature = sign_audit(&audit)?;
        Ok(audit)
    }

    /// Recompute the seal against the frozen population.
    pub fn verify(&self, batch: &FrozenBatch) -> Result<(), crate::AiError> {
        batch.verify()?;
        if self.manifest_hash != batch.manifest_hash {
            return Err(crate::AiError::RequestState(
                "sample audit does not match frozen population".into(),
            ));
        }
        if sign_audit(self)? != self.signature {
            return Err(crate::AiError::RequestState(
                "sample audit signature mismatch".into(),
            ));
        }
        let record = SampleRecord {
            manifest_hash: self.manifest_hash,
            policy_version: self.policy_version,
            reviewer: self.reviewer.clone(),
            reviewer_nonce_hash: hash_bytes(&self.reviewer_nonce),
            selected: self.selected.clone(),
            disposition: self.disposition,
        };
        record.reproduce(
            batch,
            self.reviewer.clone(),
            &self.reviewer_nonce,
            self.signed_at,
        )
    }
}

fn sign_audit(audit: &SampleAudit) -> Result<Hash, crate::AiError> {
    let unsigned = UnsignedSampleAudit {
        manifest_hash: audit.manifest_hash,
        policy_version: audit.policy_version,
        reviewer: &audit.reviewer,
        reviewer_nonce: &audit.reviewer_nonce,
        ranking: &audit.ranking,
        selected: &audit.selected,
        disposition: audit.disposition,
        signed_at: audit.signed_at,
    };
    let encoded = to_ron(&unsigned).map_err(|error| crate::AiError::Ser(error.to_string()))?;
    Ok(klotho_prove::evidence_signature(encoded.as_bytes()))
}

impl FrozenBatch {
    /// Recompute roots. Post-freeze item replacement fails closed.
    pub fn verify(&self) -> Result<(), crate::AiError> {
        let encoded = to_ron(&(self.policy.policy_version, &self.items))
            .map_err(|error| crate::AiError::Ser(error.to_string()))?;
        let batch_root = hash_bytes(encoded.as_bytes());
        if batch_root != self.batch_root {
            return Err(crate::AiError::RequestState(
                "frozen population root does not match items".into(),
            ));
        }
        let manifest = to_ron(&(&self.policy, &self.items, batch_root))
            .map_err(|error| crate::AiError::Ser(error.to_string()))?;
        if hash_bytes(manifest.as_bytes()) != self.manifest_hash {
            return Err(crate::AiError::RequestState(
                "frozen population manifest hash does not match".into(),
            ));
        }
        Ok(())
    }
}

/// Lowering a trusted risk assignment is itself R3 and is refused here.
pub fn refuse_lower(assigned: RiskLevel, proposed: RiskLevel) -> Result<(), crate::AiError> {
    if proposed < assigned {
        return Err(crate::AiError::RequestState(
            "risk lowering is R3 and is not applied from a critic or author".into(),
        ));
    }
    Ok(())
}

/// Trusted assignment ignores a model/critic proposal.
#[must_use]
pub fn assigned_level(
    policy: &RiskPolicy,
    input: &RiskInput,
    proposed: Option<RiskLevel>,
) -> RiskLevel {
    let _ = proposed;
    policy.route(input)
}

/// Related changes from one request cannot be split without a trusted reason.
pub fn refuse_split(
    left: &FrozenBatch,
    right: &FrozenBatch,
    reason: Option<&str>,
) -> Result<(), crate::AiError> {
    let overlap = left
        .items
        .iter()
        .any(|a| right.items.iter().any(|b| a.request == b.request));
    if overlap && reason.is_none() {
        return Err(crate::AiError::RequestState(
            "related changes cannot be split across batches without a trusted-policy reason".into(),
        ));
    }
    Ok(())
}

/// Post-freeze replacement of the population is denied.
pub fn refuse_replace(frozen: &FrozenBatch, items: &[BatchItem]) -> Result<(), crate::AiError> {
    frozen.verify()?;
    if items != frozen.items.as_slice() {
        return Err(crate::AiError::RequestState(
            "cannot replace a frozen sample population".into(),
        ));
    }
    Ok(())
}

fn refuse_predictable_nonce(batch: &FrozenBatch, nonce: &[u8]) -> Result<(), crate::AiError> {
    if nonce == batch.batch_root.as_bytes().as_slice() || nonce.iter().all(|b| *b == 0) {
        return Err(crate::AiError::RequestState(
            "reviewer nonce must not be derived from the frozen population".into(),
        ));
    }
    for item in &batch.items {
        if nonce == item.operation_hash.as_bytes().as_slice()
            || nonce == item.evidence_hash.as_bytes().as_slice()
            || nonce == item.anchor.0.as_slice()
        {
            return Err(crate::AiError::RequestState(
                "reviewer nonce was stuffed into the frozen population".into(),
            ));
        }
    }
    Ok(())
}

/// Milestone capacity for one review owner.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerBudget {
    /// Owner identity.
    pub owner: Name,
    /// Maximum simultaneously queued items.
    pub max_items: u32,
    /// Maximum estimated review minutes.
    pub max_minutes: u32,
}

/// Deterministic owner-queue accounting.
#[derive(Default)]
pub struct OwnerQueues {
    budgets: BTreeMap<Name, OwnerBudget>,
    queued: BTreeMap<Name, (u32, u32)>,
}

impl OwnerQueues {
    /// Install or replace an owner budget.
    pub fn set_budget(&mut self, budget: OwnerBudget) {
        self.budgets.insert(budget.owner.clone(), budget);
    }

    /// Reserve capacity before adding work to a review queue.
    pub fn reserve(
        &mut self,
        owner: &Name,
        items: u32,
        minutes: u32,
    ) -> Result<(), crate::AiError> {
        let budget = self.budgets.get(owner).ok_or_else(|| {
            crate::AiError::RequestState("review owner has no capacity budget".into())
        })?;
        let current = self.queued.get(owner).copied().unwrap_or_default();
        if current.0.saturating_add(items) > budget.max_items
            || current.1.saturating_add(minutes) > budget.max_minutes
        {
            return Err(crate::AiError::RequestState(
                "review owner capacity exceeded".into(),
            ));
        }
        self.queued
            .insert(owner.clone(), (current.0 + items, current.1 + minutes));
        Ok(())
    }

    /// Release completed or rejected work.
    pub fn release(&mut self, owner: &Name, items: u32, minutes: u32) {
        let current = self.queued.get(owner).copied().unwrap_or_default();
        self.queued.insert(
            owner.clone(),
            (
                current.0.saturating_sub(items),
                current.1.saturating_sub(minutes),
            ),
        );
    }

    /// Current `(items, minutes)` for an owner.
    #[must_use]
    pub fn usage(&self, owner: &Name) -> (u32, u32) {
        self.queued.get(owner).copied().unwrap_or_default()
    }
}
