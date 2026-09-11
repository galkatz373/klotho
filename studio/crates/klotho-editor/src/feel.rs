//! Distaff parameter-sweep and A/B play for typed feel (KAI-10).
//!
//! Candidates are immutable. An optimizer or model cannot retune a contract
//! in place. The combat/camera/design owner Pins one candidate after latency
//! and classification gates pass.

use klotho_core::Hash;
use klotho_eval::{ApprovalRef, CheckLayer};
use klotho_input::{DeviceLane, FeelGate, LatencyLog, LatencySample};
use klotho_ir::{FeelContract, Name};
use klotho_prove::hash_bytes;

use crate::EditorError;

/// One immutable feel candidate on a Distaff branch.
#[derive(Clone, Debug)]
pub struct FeelCandidate {
    /// Author-facing candidate id.
    pub id: Name,
    /// Frozen contract. Never mutated after insert.
    contract: FeelContract,
    /// Content hash of the contract bytes.
    pub branch: Hash,
    /// Trusted latency samples.
    latency: LatencyLog,
    /// Cancel-window uses observed in play.
    pub cancel_uses: u32,
    /// Successful action admissions.
    pub action_hits: u32,
    /// Failed action attempts.
    pub action_misses: u32,
    /// Camera hull/occlusion hits.
    pub occlusion_hits: u32,
    /// Optional human rating 1..=5.
    pub human_rating: Option<u8>,
}

impl FeelCandidate {
    /// Freeze `contract` as a candidate. Invalid contracts fail closed.
    pub fn freeze(id: Name, contract: FeelContract) -> Result<Self, EditorError> {
        contract
            .validate()
            .map_err(|e| EditorError::Boot(format!("feel candidate {}: {e}", id.as_str())))?;
        let encoded = klotho_ir::to_ron(&contract)
            .map_err(|e| EditorError::Boot(format!("feel encode: {e}")))?;
        Ok(Self {
            id,
            branch: hash_bytes(encoded.as_bytes()),
            contract,
            latency: LatencyLog::new(),
            cancel_uses: 0,
            action_hits: 0,
            action_misses: 0,
            occlusion_hits: 0,
            human_rating: None,
        })
    }

    /// Frozen contract.
    #[must_use]
    pub fn contract(&self) -> &FeelContract {
        &self.contract
    }

    /// Recorded latency samples.
    #[must_use]
    pub fn latency(&self) -> &LatencyLog {
        &self.latency
    }
}

/// Parameter sweep over immutable candidate branches.
#[derive(Clone, Debug)]
pub struct FeelSweep {
    /// Action being tuned.
    pub action: Name,
    /// Pinned device/display lane.
    pub lane: DeviceLane,
    candidates: Vec<FeelCandidate>,
    /// Named combat/camera/design owner.
    owner: Option<Name>,
    /// Approved candidate id.
    approved: Option<Name>,
}

impl FeelSweep {
    /// Empty sweep for `action` against `lane`.
    #[must_use]
    pub fn new(action: Name, lane: DeviceLane) -> Self {
        Self {
            action,
            lane,
            candidates: Vec::new(),
            owner: None,
            approved: None,
        }
    }

    /// Spindle Use suite on the first-title wired lane.
    pub fn spindle_action_suite() -> Result<Self, EditorError> {
        let mut sweep = Self::new(Name::from("use"), DeviceLane::FIRST_TITLE_WIRED);
        sweep.push(FeelCandidate::freeze(
            Name::from("spindle-use"),
            FeelContract::spindle_use(),
        )?)?;
        Ok(sweep)
    }

    /// Frozen candidates, insertion order.
    #[must_use]
    pub fn candidates(&self) -> &[FeelCandidate] {
        &self.candidates
    }

    /// Approved candidate, if the feel owner Pinned one.
    #[must_use]
    pub fn approved(&self) -> Option<&FeelCandidate> {
        let id = self.approved.as_ref()?;
        self.candidates.iter().find(|c| c.id == *id)
    }

    /// Insert an immutable candidate. Duplicate ids fail closed.
    pub fn push(&mut self, candidate: FeelCandidate) -> Result<(), EditorError> {
        if self.candidates.iter().any(|c| c.id == candidate.id) {
            return Err(EditorError::Boot(format!(
                "duplicate feel candidate {}",
                candidate.id.as_str()
            )));
        }
        self.candidates.push(candidate);
        Ok(())
    }

    /// Record a trusted latency sample on `id`.
    pub fn record_latency(&mut self, id: &Name, sample: LatencySample) -> Result<(), EditorError> {
        let c = self.candidate_mut(id)?;
        c.latency.push(sample);
        Ok(())
    }

    /// Record play counters on `id`.
    pub fn record_play(
        &mut self,
        id: &Name,
        cancel_uses: u32,
        hits: u32,
        misses: u32,
        occlusion_hits: u32,
    ) -> Result<(), EditorError> {
        let c = self.candidate_mut(id)?;
        c.cancel_uses = c.cancel_uses.saturating_add(cancel_uses);
        c.action_hits = c.action_hits.saturating_add(hits);
        c.action_misses = c.action_misses.saturating_add(misses);
        c.occlusion_hits = c.occlusion_hits.saturating_add(occlusion_hits);
        Ok(())
    }

    /// Human rating. Models cannot call this; Distaff requires a named owner later.
    pub fn rate(&mut self, id: &Name, rating: u8) -> Result<(), EditorError> {
        if !(1..=5).contains(&rating) {
            return Err(EditorError::Boot("feel rating must be 1..=5".into()));
        }
        self.candidate_mut(id)?.human_rating = Some(rating);
        Ok(())
    }

    /// Latency gate for every candidate against the pinned lane.
    pub fn gate_latency(&self) -> Result<(), FeelGate> {
        if self.candidates.is_empty() {
            return Err(FeelGate::NoSamples);
        }
        for c in &self.candidates {
            c.latency.meets(self.lane)?;
        }
        Ok(())
    }

    /// A/B pair of immutable branches. Neither contract is rewritten.
    pub fn ab_play<'a>(
        &'a self,
        left: &Name,
        right: &Name,
    ) -> Result<FeelAbSession<'a>, EditorError> {
        if left == right {
            return Err(EditorError::Boot("A/B needs two candidates".into()));
        }
        let a = self.candidate(left)?;
        let b = self.candidate(right)?;
        if a.branch == b.branch {
            return Err(EditorError::Boot("A/B branches are identical".into()));
        }
        Ok(FeelAbSession { left: a, right: b })
    }

    /// Named human feel owner. An optimizer or model may not call this.
    pub fn set_owner(&mut self, owner: Name) {
        self.owner = Some(owner);
    }

    /// Pin `id` after latency gates. Requires a named owner.
    pub fn approve(&mut self, id: &Name, owner: Name) -> Result<ApprovalRef, EditorError> {
        if self.owner.as_ref() != Some(&owner) {
            return Err(EditorError::Boot(
                "feel approval requires the named combat/camera/design owner".into(),
            ));
        }
        self.candidate(id)?;
        self.gate_latency()
            .map_err(|e| EditorError::Boot(e.to_string()))?;
        self.approved = Some(id.clone());
        Ok(ApprovalRef {
            by: owner,
            of: id.clone(),
        })
    }

    /// Models/optimizers have no retune API. The candidate hash is the lock.
    #[must_use]
    pub fn retune_forbidden(&self, id: &Name, proposed: &FeelContract) -> bool {
        self.candidate(id)
            .ok()
            .is_some_and(|c| c.contract() != proposed)
    }

    fn candidate(&self, id: &Name) -> Result<&FeelCandidate, EditorError> {
        self.candidates
            .iter()
            .find(|c| c.id == *id)
            .ok_or_else(|| EditorError::UnknownLocus(id.clone()))
    }

    fn candidate_mut(&mut self, id: &Name) -> Result<&mut FeelCandidate, EditorError> {
        self.candidates
            .iter_mut()
            .find(|c| c.id == *id)
            .ok_or_else(|| EditorError::UnknownLocus(id.clone()))
    }
}

/// Side-by-side A/B over two frozen candidates.
#[derive(Clone, Copy, Debug)]
pub struct FeelAbSession<'a> {
    /// Left branch.
    pub left: &'a FeelCandidate,
    /// Right branch.
    pub right: &'a FeelCandidate,
}

impl FeelAbSession<'_> {
    /// Evidence layer Distaff shows for this session.
    #[must_use]
    pub fn layer(&self) -> CheckLayer {
        CheckLayer::Approval
    }
}

/// Map a clip classification onto Distaff's review copy. Visual-only changes
/// must not claim semantic journey evidence.
#[must_use]
pub fn evidence_copy(lane: klotho_anim::EvidenceLane) -> &'static str {
    match lane {
        klotho_anim::EvidenceLane::VisualOnly => {
            "visual-only: Rite WAIT unchanged; no semantic journey rerun"
        }
        klotho_anim::EvidenceLane::SemanticJourneys => {
            "semantic: Pin plus affected deterministic journeys"
        }
    }
}
