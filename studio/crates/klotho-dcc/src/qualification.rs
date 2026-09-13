//! Blinded hero-route qualification with an explicitly funded fallback (K92).

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::Hash;
use serde::{Deserialize, Serialize};

use crate::{AssetCandidateId, DccError, SourceRoute};

/// Required representative hero cases.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeroCaseKind {
    /// Principal character.
    PrincipalCharacter,
    /// Signature outfit or weapon.
    SignatureEquipment,
    /// Hero environment kit.
    HeroEnvironment,
    /// Facial/rig deformation case.
    FacialRig,
    /// Damage/variant case.
    DamagedVariant,
}

impl HeroCaseKind {
    /// Complete qualification set.
    pub const ALL: [Self; 5] = [
        Self::PrincipalCharacter,
        Self::SignatureEquipment,
        Self::HeroEnvironment,
        Self::FacialRig,
        Self::DamagedVariant,
    ];
}

/// Blinded measurement. Route/provider is deliberately absent until reveal.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCandidate {
    /// Candidate content id.
    pub candidate: AssetCandidateId,
    /// Accepted at the unchanged quality bar.
    pub accepted: bool,
    /// Ranked defect codes, highest severity first.
    pub ranked_defects: Vec<String>,
    /// Hands-on rework minutes across disciplines.
    pub rework_minutes: u32,
    /// End-to-end elapsed minutes.
    pub elapsed_minutes: u32,
    /// Iterations consumed.
    pub iterations: u16,
    /// Objective topology/UV/material/rig/LOD/facial gate evidence.
    pub fitness_evidence: Hash,
    /// Style comparison evidence.
    pub style_evidence: Hash,
    /// Measured runtime cost evidence.
    pub runtime_evidence: Hash,
    /// Provenance/legal result.
    pub rights_evidence: Hash,
}

/// One blinded brief and its candidate set.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeroCase {
    /// Representative case.
    pub kind: HeroCaseKind,
    /// Approved reference/brief hash.
    pub brief: Hash,
    /// At least two blinded candidates.
    pub candidates: Vec<QualificationCandidate>,
    /// Art director selection after blind scoring.
    pub selected: AssetCandidateId,
}

/// Route reveal performed only after blinded scoring.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteReveal {
    /// Candidate id.
    pub candidate: AssetCandidateId,
    /// Actual production route.
    pub route: SourceRoute,
    /// Immutable provider/vendor/activity record.
    pub activity: Hash,
}

/// Program capacity reserved for a commissioned human/vendor fallback.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationFunding {
    /// Modeling/material/rig/animation/art capacity.
    pub discipline_fte_weeks: u16,
    /// Reserved vendor capacity.
    pub vendor_weeks: u16,
    /// Approval record for the program envelope.
    pub approved_plan: Hash,
}

/// Complete qualification report.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeroQualification {
    /// Named art director.
    pub art_director: String,
    /// Blind cases.
    pub cases: Vec<HeroCase>,
    /// Post-score route reveal.
    pub reveal: Vec<RouteReveal>,
    /// Funded fallback.
    pub fallback: QualificationFunding,
}

impl HeroQualification {
    /// Validate representative coverage, blinding/reveal integrity, unchanged
    /// quality gates, and funded commissioned fallback.
    pub fn validate(&self) -> Result<(), DccError> {
        if self.art_director.trim().is_empty()
            || self.fallback.discipline_fte_weeks == 0
            || self.fallback.vendor_weeks == 0
            || self.fallback.approved_plan == Hash::ZERO
        {
            return Err(DccError::Policy("hero fallback is not funded".into()));
        }
        let reveal: BTreeMap<_, _> = self.reveal.iter().map(|row| (row.candidate, row)).collect();
        if reveal.len() != self.reveal.len()
            || self.reveal.iter().any(|row| row.activity == Hash::ZERO)
        {
            return Err(DccError::Policy("invalid hero route reveal".into()));
        }
        let mut seen = BTreeSet::new();
        for case in &self.cases {
            if !seen.insert(case.kind) || case.brief == Hash::ZERO || case.candidates.len() < 2 {
                return Err(DccError::Policy("incomplete hero case".into()));
            }
            let selected = case
                .candidates
                .iter()
                .find(|candidate| candidate.candidate == case.selected)
                .ok_or_else(|| DccError::Policy("selected hero candidate missing".into()))?;
            if !selected.accepted {
                return Err(DccError::Policy(
                    "selected hero candidate failed quality".into(),
                ));
            }
            let mut has_proposed = false;
            let mut has_baseline = false;
            for candidate in &case.candidates {
                if candidate.elapsed_minutes == 0
                    || candidate.iterations == 0
                    || [
                        candidate.fitness_evidence,
                        candidate.style_evidence,
                        candidate.runtime_evidence,
                        candidate.rights_evidence,
                    ]
                    .contains(&Hash::ZERO)
                {
                    return Err(DccError::Policy("hero evidence incomplete".into()));
                }
                let route = reveal
                    .get(&candidate.candidate)
                    .ok_or_else(|| DccError::Policy("hero route not revealed".into()))?
                    .route;
                has_baseline |= matches!(route, SourceRoute::Commissioned);
                has_proposed |= matches!(
                    route,
                    SourceRoute::Retrieval
                        | SourceRoute::GeneratedLocal
                        | SourceRoute::GeneratedRemote
                );
            }
            if !has_baseline || !has_proposed {
                return Err(DccError::Policy(
                    "hero case lacks proposed route or commissioned baseline".into(),
                ));
            }
        }
        if seen != HeroCaseKind::ALL.into_iter().collect() {
            return Err(DccError::Policy(
                "hero representative set incomplete".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> Hash {
        Hash([byte; 32])
    }

    #[test]
    fn representative_blind_bakeoff_requires_funded_fallback() {
        let mut reveal = Vec::new();
        let cases = HeroCaseKind::ALL
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                let generated = AssetCandidateId(hash((index * 2 + 1) as u8));
                let commissioned = AssetCandidateId(hash((index * 2 + 2) as u8));
                reveal.push(RouteReveal {
                    candidate: generated,
                    route: SourceRoute::GeneratedLocal,
                    activity: hash(40),
                });
                reveal.push(RouteReveal {
                    candidate: commissioned,
                    route: SourceRoute::Commissioned,
                    activity: hash(41),
                });
                let row = |candidate| QualificationCandidate {
                    candidate,
                    accepted: true,
                    ranked_defects: Vec::new(),
                    rework_minutes: 30,
                    elapsed_minutes: 120,
                    iterations: 2,
                    fitness_evidence: hash(20),
                    style_evidence: hash(21),
                    runtime_evidence: hash(22),
                    rights_evidence: hash(23),
                };
                HeroCase {
                    kind,
                    brief: hash(30),
                    candidates: vec![row(generated), row(commissioned)],
                    selected: generated,
                }
            })
            .collect();
        let mut report = HeroQualification {
            art_director: "art.director".into(),
            cases,
            reveal,
            fallback: QualificationFunding {
                discipline_fte_weeks: 15,
                vendor_weeks: 4,
                approved_plan: hash(50),
            },
        };
        assert_eq!(report.validate(), Ok(()));
        report.fallback.vendor_weeks = 0;
        assert!(report.validate().is_err());
    }
}
