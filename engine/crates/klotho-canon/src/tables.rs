//! Cooked Law / Affordance / Beat / Rite tables.

use std::collections::BTreeMap;

use klotho_core::{AffordanceId, LawId, Mm, ResourceId, Sigil};
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
            law_by_name,
            affordance_by_name,
            rite_by_name,
            resource_by_name,
        }
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
