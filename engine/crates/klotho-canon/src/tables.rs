//! Cooked Law / Affordance / Beat / Rite tables.

use std::collections::BTreeMap;

use klotho_core::{AffordanceId, BodyPhysics, ConstraintPhysics, LawId, Mm, ResourceId, Sigil};
use klotho_ir::{Name, Rel};

use crate::ast::{CookedSlot, PredId, PredProgram, RiteChunk, RiteId};

/// Frozen Canon: packed tables plus compiled preds. Arc'd by the kernel later.
#[derive(Clone, Debug, Default)]
pub struct Canon {
    /// Admission / conservation / ramp laws, first-seen order.
    pub laws: Vec<CookedLaw>,
    /// Declared + implicitly interned affordances.
    pub affordances: Vec<CookedAffordance>,
    /// Cooked rites (CFG-checked). Guard preds sit in [`CookedRite::guards`].
    pub rites: Vec<CookedRite>,
    /// Episode charts.
    pub beats: Vec<CookedBeat>,
    /// Compiled predicates. [`PredId`] indexes this.
    pub preds: Vec<PredProgram>,
    /// Resource names, indexed by [`ResourceId`].
    pub resources: Vec<Name>,
    /// Knows-fact names, indexed by the `u16` in [`crate::Atom::Knows`].
    pub facts: Vec<Name>,
    /// Pin names, indexed by [`CookedSlot::Pin`].
    pub pin_names: Vec<Name>,
    /// Seed Sigil for each pin. `None` if cooked without that locus.
    pub pin_sigils: Vec<Option<Sigil>>,
    /// Canon-bound per-locus physical configuration.
    pub physics: BTreeMap<Sigil, BodyPhysics>,
    /// Canon-bound physical constraints, keyed by constraint identity.
    pub constraints: BTreeMap<Sigil, ConstraintPhysics>,
    law_by_name: BTreeMap<Name, LawId>,
    affordance_by_name: BTreeMap<Name, AffordanceId>,
    rite_by_name: BTreeMap<Name, RiteId>,
    resource_by_name: BTreeMap<Name, ResourceId>,
}

impl Canon {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        laws: Vec<CookedLaw>,
        affordances: Vec<CookedAffordance>,
        rites: Vec<CookedRite>,
        beats: Vec<CookedBeat>,
        preds: Vec<PredProgram>,
        resources: Vec<Name>,
        facts: Vec<Name>,
        pin_names: Vec<Name>,
        pin_sigils: Vec<Option<Sigil>>,
        law_by_name: BTreeMap<Name, LawId>,
        affordance_by_name: BTreeMap<Name, AffordanceId>,
        rite_by_name: BTreeMap<Name, RiteId>,
        resource_by_name: BTreeMap<Name, ResourceId>,
    ) -> Self {
        Self {
            laws,
            affordances,
            rites,
            beats,
            preds,
            resources,
            facts,
            pin_names,
            pin_sigils,
            physics: BTreeMap::new(),
            constraints: BTreeMap::new(),
            law_by_name,
            affordance_by_name,
            rite_by_name,
            resource_by_name,
        }
    }

    /// Bind validated physical configuration to a cooked locus.
    pub fn bind_physics(&mut self, locus: Sigil, body: BodyPhysics) -> bool {
        if !body.is_valid() {
            return false;
        }
        self.physics.insert(locus, body).is_none()
    }

    /// Physical configuration for a cooked locus.
    #[must_use]
    pub fn body_physics(&self, locus: Sigil) -> Option<BodyPhysics> {
        self.physics.get(&locus).copied()
    }

    /// Bind a validated physical constraint. Identity need not be a locus.
    pub fn bind_constraint(&mut self, id: Sigil, constraint: ConstraintPhysics) -> bool {
        if !constraint.is_valid() {
            return false;
        }
        self.constraints.insert(id, constraint).is_none()
    }

    /// Canon constraint, if bound.
    #[must_use]
    pub fn constraint(&self, id: Sigil) -> Option<ConstraintPhysics> {
        self.constraints.get(&id).copied()
    }

    /// Look up a compiled pred.
    #[must_use]
    pub fn pred(&self, id: PredId) -> Option<&PredProgram> {
        self.preds.get(id.0 as usize)
    }

    /// Packed law id.
    #[must_use]
    pub fn law_id(&self, name: &str) -> Option<LawId> {
        self.law_by_name.get(&Name::from(name)).copied()
    }

    /// Packed affordance id (`"Lockable"`).
    #[must_use]
    pub fn affordance_id(&self, name: &str) -> Option<AffordanceId> {
        self.affordance_by_name.get(&Name::from(name)).copied()
    }

    /// Packed rite id (`"lockpick"`).
    #[must_use]
    pub fn rite_id(&self, name: &str) -> Option<RiteId> {
        self.rite_by_name.get(&Name::from(name)).copied()
    }

    /// Packed resource id (`"heat"`).
    #[must_use]
    pub fn resource_id(&self, name: &str) -> Option<ResourceId> {
        self.resource_by_name.get(&Name::from(name)).copied()
    }

    /// Seed pin.
    #[must_use]
    pub fn pin(&self, name: &str) -> Option<Sigil> {
        let i = self.pin_names.iter().position(|n| n.as_str() == name)?;
        self.pin_sigils.get(i).copied().flatten()
    }

    /// Intern byte-identical predicate programs and rewrite every internal
    /// reference to the first stable occurrence.
    ///
    /// Predicate ids are a packed implementation detail; Law, Affordance, and
    /// Rite ids are left untouched. The returned count is the number of table
    /// rows removed. This is used only by the optimized cook.
    pub fn intern_predicates(&mut self) -> usize {
        let before = self.preds.len();
        let mut unique = Vec::with_capacity(before);
        let mut remap = Vec::with_capacity(before);
        for pred in self.preds.drain(..) {
            let index = unique.iter().position(|known| known == &pred);
            let index = match index {
                Some(index) => index,
                None => {
                    unique.push(pred);
                    unique.len() - 1
                }
            };
            remap.push(PredId(index as u16));
        }
        self.preds = unique;

        let map = |id: &mut PredId| *id = remap[id.0 as usize];
        for law in &mut self.laws {
            map(&mut law.when);
            match &mut law.body {
                CookedLawBody::Pred { must, .. } => map(must),
                CookedLawBody::Cap { mark, .. } => map(mark),
                CookedLawBody::Ramp { .. }
                | CookedLawBody::Spread { .. }
                | CookedLawBody::Conserve { .. } => {}
            }
        }
        for affordance in &mut self.affordances {
            for required in &mut affordance.requires {
                map(required);
            }
        }
        for rite in &mut self.rites {
            for guard in rite.guards.values_mut() {
                map(guard);
            }
        }
        before - self.preds.len()
    }
}

/// One cooked Law row.
#[derive(Clone, Debug)]
pub struct CookedLaw {
    /// Packed id (table index).
    pub id: LawId,
    /// Authoring name.
    pub name: Name,
    /// When this law is considered.
    pub when: PredId,
    /// Body. `Pred.must` / `Cap.mark` are [`PredId`]s.
    pub body: CookedLawBody,
}

/// Cooked [`klotho_ir::LawBody`].
#[derive(Clone, Debug)]
pub enum CookedLawBody {
    /// Admission predicate.
    Pred {
        /// Must hold or the proposal rolls back.
        must: PredId,
        /// Optional soft cost.
        ought: Option<CookedCost>,
    },
    /// Self-qty on dirty loci matching `when`.
    Ramp {
        /// Resource.
        res: ResourceId,
        /// Added per tick.
        per_tick: i32,
        /// QtyChanged when `floor(qty/quantum)` changes.
        quantum: i32,
        /// Clamp.
        cap: i32,
    },
    /// Neighbor write + AWAKE.
    Spread {
        /// Resource.
        res: ResourceId,
        /// Added per tick.
        per_tick: i32,
        /// AabbNear radius.
        near: Mm,
        /// Global cap on marked loci.
        cap_global: u16,
        /// Ignite threshold.
        ignite_at: i32,
    },
    /// Admission-time conservation over a relation.
    Conserve {
        /// Resource.
        res: ResourceId,
        /// Relation summing the conserved qty.
        over: Rel,
    },
    /// Kernel counter.
    Cap {
        /// Mark predicate.
        mark: PredId,
        /// Maximum marked loci.
        n: u16,
        /// Optional membership.
        require_rel: Option<(Rel, CookedSlot)>,
    },
}

/// Soft `ought` cost.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct CookedCost {
    /// Resource spent.
    pub res: ResourceId,
    /// Amount.
    pub amount: i32,
}

/// One cooked Affordance row.
#[derive(Clone, Debug)]
pub struct CookedAffordance {
    /// Packed id.
    pub id: AffordanceId,
    /// `"Portable"`, `"Lockable"`, …
    pub name: Name,
    /// Compiled requires.
    pub requires: Vec<PredId>,
    /// Verb / rite tags this capability enables.
    pub grants: Vec<Name>,
    /// Mutually exclusive affordances.
    pub conflicts: Vec<AffordanceId>,
}

/// One cooked Beat row.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CookedBeat {
    /// `"evening_trade"`.
    pub id: Name,
    /// Author notes.
    pub notes: String,
}

/// CFG-checked rite plus compiled Guard/Branch preds.
#[derive(Clone, Debug)]
pub struct CookedRite {
    /// Packed id.
    pub id: RiteId,
    /// Authoring name.
    pub name: Name,
    /// Labeled ISA. Interpreter is `klotho-commit`.
    pub chunk: RiteChunk,
    /// `pc` → compiled pred for `Guard` / `Branch`.
    pub guards: BTreeMap<u16, PredId>,
}

#[cfg(test)]
mod tests {
    use klotho_ir::{CanonDiff, IntentDoc, Law, LawBody, Name, Pred, ProvenanceId, StyleIntent};

    use super::*;

    #[test]
    fn predicate_interning_rewrites_references_without_semantic_ids() {
        let pred = Pred::SourceIs(klotho_ir::SourceKind::Player);
        let law = |id: &str| {
            CanonDiff::AddLaw(Law {
                id: Name::from(id),
                when: pred.clone(),
                body: LawBody::Pred {
                    must: pred.clone(),
                    ought: None,
                },
            })
        };
        let doc = IntentDoc {
            style: StyleIntent::default(),
            canon_diffs: vec![law("a"), law("b")],
            seed: Vec::new(),
            minds: Vec::new(),
            provenance: ProvenanceId(klotho_core::Hash::ZERO),
        };
        let mut canon = crate::cook(&doc).unwrap();
        assert_eq!(canon.preds.len(), 4);
        assert_eq!(canon.intern_predicates(), 3);
        assert_eq!(canon.preds.len(), 1);
        assert_eq!(canon.law_id("a"), Some(klotho_core::LawId(0)));
        assert_eq!(canon.law_id("b"), Some(klotho_core::LawId(1)));
        assert!(canon.laws.iter().all(|law| law.when == PredId(0)));
    }
}
