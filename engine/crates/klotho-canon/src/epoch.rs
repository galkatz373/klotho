//! Packed-id remapping between two cooked Canon epochs.

use std::collections::BTreeMap;

use klotho_core::{AffordanceId, ResourceId};
use klotho_ir::Name;

use crate::{Canon, RiteId};

/// Deterministic mapping from the packed ids in one Canon to the next.
///
/// Projection rows store packed ids, so an epoch transition must translate
/// them by stable authoring name. An absent mapping means that the declaration
/// was removed and the corresponding derived row is discarded.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct EpochMap {
    resources: Vec<Option<ResourceId>>,
    affordances: Vec<Option<AffordanceId>>,
    rites: Vec<Option<RiteId>>,
    facts: Vec<Option<u16>>,
}

impl EpochMap {
    /// Build a map by matching stable authoring names in `from` and `to`.
    #[must_use]
    pub fn between(from: &Canon, to: &Canon) -> Self {
        let resource_names = index_names(
            to.resources
                .iter()
                .enumerate()
                .filter_map(|(i, n)| u8::try_from(i).ok().map(|i| (n, ResourceId(i)))),
        );
        let affordance_names = index_names(to.affordances.iter().map(|a| (&a.name, a.id)));
        let rite_names = index_names(to.rites.iter().map(|r| (&r.name, r.id)));
        let fact_names = index_names(
            to.facts
                .iter()
                .enumerate()
                .filter_map(|(i, n)| u16::try_from(i).ok().map(|i| (n, i))),
        );
        Self {
            resources: from
                .resources
                .iter()
                .map(|n| resource_names.get(n).copied())
                .collect(),
            affordances: from
                .affordances
                .iter()
                .map(|a| affordance_names.get(&a.name).copied())
                .collect(),
            rites: from
                .rites
                .iter()
                .map(|r| rite_names.get(&r.name).copied())
                .collect(),
            facts: from
                .facts
                .iter()
                .map(|n| fact_names.get(n).copied())
                .collect(),
        }
    }

    /// Translate an old resource id, or `None` if it was removed.
    #[must_use]
    pub fn resource(&self, old: ResourceId) -> Option<ResourceId> {
        self.resources.get(usize::from(old.0)).copied().flatten()
    }

    /// Translate an old affordance id, or `None` if it was removed.
    #[must_use]
    pub fn affordance(&self, old: AffordanceId) -> Option<AffordanceId> {
        self.affordances.get(usize::from(old.0)).copied().flatten()
    }

    /// Translate an old Rite id, or `None` if it was removed.
    #[must_use]
    pub fn rite(&self, old: RiteId) -> Option<RiteId> {
        self.rites.get(usize::from(old.0)).copied().flatten()
    }

    /// Translate an old Knows-fact id, or `None` if it was removed.
    #[must_use]
    pub fn fact(&self, old: u16) -> Option<u16> {
        self.facts.get(usize::from(old)).copied().flatten()
    }
}

fn index_names<'a, T: Copy>(rows: impl Iterator<Item = (&'a Name, T)>) -> BTreeMap<Name, T> {
    rows.map(|(name, id)| (name.clone(), id)).collect()
}

#[cfg(test)]
mod tests {
    use klotho_ir::{CanonDiff, Law, LawBody, Pred, Verb};

    use super::*;
    use crate::cook_diffs;

    fn ramp(id: &str, resource: &str) -> CanonDiff {
        CanonDiff::AddLaw(Law {
            id: Name::from(id),
            when: Pred::EqVerb(Verb::Use),
            body: LawBody::Ramp {
                res: Name::from(resource),
                per_tick: 1,
                quantum: 1,
                cap: 10,
            },
        })
    }

    #[test]
    fn resource_ids_follow_names_across_repack() {
        let old = cook_diffs(&[ramp("old", "removed"), ramp("keep", "health")]).unwrap();
        let new = cook_diffs(&[ramp("keep", "health")]).unwrap();
        let map = EpochMap::between(&old, &new);
        assert_eq!(map.resource(ResourceId(0)), None);
        assert_eq!(map.resource(ResourceId(1)), Some(ResourceId(0)));
    }
}
