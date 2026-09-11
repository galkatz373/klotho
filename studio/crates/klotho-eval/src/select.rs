//! Affected-test selection. Declared dependents are never skipped.

use std::collections::{BTreeMap, BTreeSet};

use klotho_ir::AnchorId;

use crate::error::EvalError;
use crate::ids::JourneyId;
use crate::journey::JourneySpec;

/// Indexed journeys plus dependency edges.
#[derive(Clone, Debug, Default)]
pub struct JourneyIndex {
    specs: BTreeMap<JourneyId, JourneySpec>,
}

impl JourneyIndex {
    /// Empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a spec.
    pub fn insert(&mut self, spec: JourneySpec) {
        self.specs.insert(spec.id.clone(), spec);
    }

    /// Borrow a spec.
    #[must_use]
    pub fn get(&self, id: &JourneyId) -> Option<&JourneySpec> {
        self.specs.get(id)
    }

    /// Every spec, sorted by id.
    pub fn iter(&self) -> impl Iterator<Item = &JourneySpec> {
        self.specs.values()
    }
}

/// Semantic blast radius of a change.
#[derive(Clone, Debug, Default)]
pub struct ChangeImpact {
    /// Written anchors.
    pub anchors: BTreeSet<AnchorId>,
    /// Written modules.
    pub modules: BTreeSet<AnchorId>,
    /// Journeys named by the acceptance contract.
    pub declared: BTreeSet<JourneyId>,
}

/// Select the smallest sound set. Includes declared journeys, overlapping
/// coverage, prerequisites, and every dependent of a selected journey.
pub fn select(index: &JourneyIndex, impact: &ChangeImpact) -> Result<Vec<JourneyId>, EvalError> {
    let mut selected: BTreeSet<JourneyId> = impact.declared.clone();
    for spec in index.iter() {
        let cover = spec.anchors.iter().any(|a| impact.anchors.contains(a))
            || spec.modules.iter().any(|m| impact.modules.contains(m));
        if cover {
            selected.insert(spec.id.clone());
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        let snapshot: Vec<JourneyId> = selected.iter().cloned().collect();
        for id in snapshot {
            let Some(spec) = index.get(&id) else {
                continue;
            };
            for pre in &spec.depends_on {
                if selected.insert(pre.clone()) {
                    changed = true;
                }
            }
            for spec in index.iter() {
                if spec.depends_on.iter().any(|d| d == &id) && selected.insert(spec.id.clone()) {
                    changed = true;
                }
            }
        }
    }

    for spec in index.iter() {
        for pre in &spec.depends_on {
            if selected.contains(pre) && !selected.contains(&spec.id) {
                return Err(EvalError::Select {
                    missing: spec.id.clone(),
                    of: pre.clone(),
                });
            }
        }
    }
    for id in &impact.declared {
        if !selected.contains(id) && index.get(id).is_some() {
            return Err(EvalError::Select {
                missing: id.clone(),
                of: id.clone(),
            });
        }
    }

    topo_sort(index, &selected)
}

fn topo_sort(
    index: &JourneyIndex,
    selected: &BTreeSet<JourneyId>,
) -> Result<Vec<JourneyId>, EvalError> {
    let mut remaining: BTreeSet<JourneyId> = selected.clone();
    let mut out = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let ready: Vec<JourneyId> = remaining
            .iter()
            .filter(|id| {
                index.get(id).is_none_or(|spec| {
                    spec.depends_on
                        .iter()
                        .all(|d| !remaining.contains(d) || !selected.contains(d))
                })
            })
            .cloned()
            .collect();
        let Some(next) = ready.into_iter().min() else {
            let token = remaining
                .iter()
                .next()
                .map(|id| id.as_str().to_owned())
                .unwrap_or_default();
            return Err(EvalError::Cycle(token));
        };
        remaining.remove(&next);
        out.push(next);
    }
    Ok(out)
}
