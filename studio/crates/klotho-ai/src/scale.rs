//! Production-scale authoring orchestration and review-capacity accounting.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::{AnchorId, Name, to_ron};
use klotho_prove::hash_bytes;

use crate::{AiError, RiskLevel};

/// One dependency-aware unit of authoring work.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleWork {
    /// Stable work identity.
    pub id: Name,
    /// Work that must complete first.
    pub dependencies: BTreeSet<Name>,
    /// Semantic anchors read by this work.
    pub reads: BTreeSet<AnchorId>,
    /// Semantic anchors exclusively owned while active.
    pub writes: BTreeSet<AnchorId>,
    /// Number of typed semantic operations in this bounded work unit.
    pub semantic_changes: u32,
}

/// Deterministic dependency scheduler with active semantic ownership.
#[derive(Default)]
pub struct OwnershipScheduler {
    pending: BTreeMap<Name, ScaleWork>,
    active: BTreeMap<Name, ScaleWork>,
    completed: BTreeSet<Name>,
    completed_changes: u64,
}

impl OwnershipScheduler {
    /// Add work. Duplicate ids and self-dependencies fail closed.
    pub fn add(&mut self, work: ScaleWork) -> Result<(), AiError> {
        if work.id.as_str().is_empty()
            || work.semantic_changes == 0
            || work.dependencies.contains(&work.id)
            || self.pending.contains_key(&work.id)
            || self.active.contains_key(&work.id)
            || self.completed.contains(&work.id)
        {
            return Err(AiError::RequestState(
                "invalid or duplicate scale work".into(),
            ));
        }
        self.pending.insert(work.id.clone(), work);
        Ok(())
    }

    /// Canonically ordered work that can start without violating dependencies or ownership.
    #[must_use]
    pub fn ready(&self) -> Vec<Name> {
        self.pending
            .values()
            .filter(|work| {
                work.dependencies.is_subset(&self.completed)
                    && self.active.values().all(|live| {
                        work.writes.is_disjoint(&live.writes)
                            && work.writes.is_disjoint(&live.reads)
                            && work.reads.is_disjoint(&live.writes)
                    })
            })
            .map(|work| work.id.clone())
            .collect()
    }

    /// Start one ready unit and claim its write anchors.
    pub fn start(&mut self, id: &Name) -> Result<(), AiError> {
        if !self.ready().contains(id) {
            return Err(AiError::RequestState(
                "scale work is blocked by dependency or ownership".into(),
            ));
        }
        let work = self
            .pending
            .remove(id)
            .ok_or_else(|| AiError::RequestState("unknown scale work".into()))?;
        self.active.insert(id.clone(), work);
        Ok(())
    }

    /// Complete active work and release its ownership.
    pub fn complete(&mut self, id: &Name) -> Result<(), AiError> {
        let work = self
            .active
            .remove(id)
            .ok_or_else(|| AiError::RequestState("scale work is not active".into()))?;
        self.completed_changes = self
            .completed_changes
            .saturating_add(u64::from(work.semantic_changes));
        self.completed.insert(id.clone());
        Ok(())
    }

    /// Number of completed work units.
    #[must_use]
    pub fn completed_len(&self) -> usize {
        self.completed.len()
    }

    /// Number of typed semantic operations represented by completed work.
    #[must_use]
    pub fn completed_changes(&self) -> u64 {
        self.completed_changes
    }
}

/// Module dependency and content-hash index used for incremental recook.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactGraph {
    /// Current content hash by module.
    pub content: BTreeMap<Name, Hash>,
    /// Direct dependents by module.
    pub dependents: BTreeMap<Name, BTreeSet<Name>>,
}

impl ImpactGraph {
    /// Return the changed modules and their transitive dependents in canonical order.
    #[must_use]
    pub fn affected(&self, next: &BTreeMap<Name, Hash>) -> BTreeSet<Name> {
        let mut affected: BTreeSet<Name> = self
            .content
            .keys()
            .chain(next.keys())
            .filter(|name| self.content.get(*name) != next.get(*name))
            .cloned()
            .collect();
        let mut frontier: Vec<Name> = affected.iter().cloned().collect();
        while let Some(module) = frontier.pop() {
            if let Some(rows) = self.dependents.get(&module) {
                for dependent in rows {
                    if affected.insert(dependent.clone()) {
                        frontier.push(dependent.clone());
                    }
                }
            }
        }
        affected
    }
}

/// Review work arriving in one capacity period.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewArrival {
    /// Zero-based period.
    pub period: u32,
    /// Responsible discipline owner.
    pub owner: Name,
    /// Estimated review minutes arriving.
    pub minutes: u32,
}

/// Checked-in service model for one named owner.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCapacity {
    /// Named owner.
    pub owner: Name,
    /// Review minutes served in a normal period.
    pub service_minutes: u32,
    /// Maximum permitted queued minutes.
    pub maximum_backlog: u32,
    /// Periods in which the owner is absent and serves no work.
    pub absent_periods: BTreeSet<u32>,
}

/// Result of deterministic arrival-versus-service simulation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CapacityReport {
    /// Peak queued minutes by owner.
    pub peak_backlog: BTreeMap<Name, u32>,
    /// Remaining queued minutes by owner.
    pub final_backlog: BTreeMap<Name, u32>,
}

/// Simulate review queues, failing when a burst or absence exceeds the funded model.
pub fn simulate_capacity(
    periods: u32,
    capacities: &[ReviewCapacity],
    arrivals: &[ReviewArrival],
) -> Result<CapacityReport, AiError> {
    let by_owner: BTreeMap<_, _> = capacities
        .iter()
        .map(|row| (row.owner.clone(), row))
        .collect();
    if periods == 0
        || by_owner.len() != capacities.len()
        || capacities.iter().any(|row| {
            row.service_minutes == 0
                || row.maximum_backlog == 0
                || row.absent_periods.iter().any(|period| *period >= periods)
        })
        || arrivals
            .iter()
            .any(|row| row.period >= periods || row.minutes == 0)
    {
        return Err(AiError::RequestState(
            "invalid review capacity model".into(),
        ));
    }
    let mut backlog: BTreeMap<Name, u32> = BTreeMap::new();
    let mut peak = BTreeMap::new();
    for period in 0..periods {
        for arrival in arrivals.iter().filter(|row| row.period == period) {
            if !by_owner.contains_key(&arrival.owner) {
                return Err(AiError::RequestState("arrival has no review owner".into()));
            }
            let queued = backlog.entry(arrival.owner.clone()).or_default();
            *queued = queued.saturating_add(arrival.minutes);
        }
        for (owner, capacity) in &by_owner {
            let queued = backlog.entry(owner.clone()).or_default();
            peak.entry(owner.clone())
                .and_modify(|value: &mut u32| *value = (*value).max(*queued))
                .or_insert(*queued);
            if *queued > capacity.maximum_backlog {
                return Err(AiError::RequestState(format!(
                    "review capacity exceeded for {}",
                    owner.as_str()
                )));
            }
            if !capacity.absent_periods.contains(&period) {
                *queued = queued.saturating_sub(capacity.service_minutes);
            }
        }
    }
    Ok(CapacityReport {
        peak_backlog: peak,
        final_backlog: backlog,
    })
}

/// One sealed evidence item entering a production roll-up.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceItem {
    /// Artifact or semantic anchor.
    pub anchor: AnchorId,
    /// Trusted risk assignment.
    pub risk: RiskLevel,
    /// Sealed evidence hash.
    pub evidence: Hash,
}

/// Canonical production evidence summary.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct EvidenceRollup {
    /// Item count by risk.
    pub by_risk: BTreeMap<RiskLevel, u32>,
    /// Hash of the sorted complete population.
    pub root: Hash,
}

/// Aggregate evidence without allowing advisory scores to alter risk.
pub fn rollup_evidence(mut items: Vec<EvidenceItem>) -> Result<EvidenceRollup, AiError> {
    if items.is_empty() || items.iter().any(|item| item.evidence == Hash::ZERO) {
        return Err(AiError::Evidence(
            "empty or unsealed evidence roll-up".into(),
        ));
    }
    items.sort();
    if items
        .windows(2)
        .any(|pair| pair[0].anchor == pair[1].anchor)
    {
        return Err(AiError::Evidence("duplicate evidence anchor".into()));
    }
    let mut by_risk = BTreeMap::new();
    for item in &items {
        *by_risk.entry(item.risk).or_insert(0) += 1;
    }
    let encoded = to_ron(&items).map_err(|error| AiError::Ser(error.to_string()))?;
    Ok(EvidenceRollup {
        by_risk,
        root: hash_bytes(encoded.as_bytes()),
    })
}
