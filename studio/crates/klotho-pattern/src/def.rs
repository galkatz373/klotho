//! Pattern definition types. No runtime objects.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use klotho_ir::{
    AnchorId, CanonDiff, IntentDoc, MindSpec, Name, ObjectAnchor, ParameterType, ParameterValue,
    SeedFact,
};

/// Standard-library family.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternFamily {
    /// Door, lever, checkpoint, traversal contract.
    Traversal,
    /// Melee, ranged, destructible, encounter envelope.
    Combat,
    /// Patrol, guard, assist, flee, conversation.
    Ai,
    /// Acquire, escort, clues, handoff, optional.
    Quest,
    /// Conversation, bark, beat, knowledge, lore.
    Narrative,
    /// Place, graph, pocket, hub, dressing, audio.
    World,
    /// Prompt, remap, subtitle, hold/toggle, contrast, scale, reader, motion, focus.
    Ui,
    /// Save, analytics, screenshot, journey, performance.
    Production,
    /// Input buffer, camera, aim-assist, haptics, accessibility (KAI-10).
    Feel,
}

impl PatternFamily {
    /// Catalog snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Traversal => "traversal",
            Self::Combat => "combat",
            Self::Ai => "ai",
            Self::Quest => "quest",
            Self::Narrative => "narrative",
            Self::World => "world",
            Self::Ui => "ui",
            Self::Production => "production",
            Self::Feel => "feel",
        }
    }
}

/// Static default used by the standard library.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum StaticValue {
    /// [`ParameterType::Name`].
    Name(&'static str),
    /// [`ParameterType::I32`].
    I32(i32),
    /// [`ParameterType::Bool`].
    Bool(bool),
}

impl StaticValue {
    /// Runtime parameter value.
    #[must_use]
    pub fn to_value(self) -> ParameterValue {
        match self {
            Self::Name(n) => ParameterValue::Name(Name::from(n)),
            Self::I32(v) => ParameterValue::I32(v),
            Self::Bool(v) => ParameterValue::Bool(v),
        }
    }
}

/// One pattern parameter.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ParamSpec {
    /// Parameter name.
    pub name: &'static str,
    /// Value type.
    pub ty: ParameterType,
    /// Default when omitted.
    pub default: Option<StaticValue>,
}

/// Declared expansion budget. Idle per-tick work is always zero in v1.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct PatternBudget {
    /// Maximum predicate nodes.
    pub predicates: u32,
    /// Maximum rite instructions.
    pub rite_steps: u32,
    /// Per-tick writers (`Ramp`/`Spread`). v1 is 0.
    pub per_tick: u32,
}

/// How a pattern expands to ordinary IR.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ExpandKind {
    /// Affordance + key-or-rite Law + unlock Rite + seed relations.
    LockablePassage,
    /// Beat / Knows marker.
    Marker,
    /// Traversal window Law + Rite.
    TraversalContract,
    /// Combat exchange Rite + Hittable affordance.
    CombatExchange,
    /// Hit-count assembly.
    Destructible,
    /// Encounter envelope Beat.
    EncounterBoundary,
    /// Mind goals on an actor.
    MindPolicy,
    /// Quest Knows + Beat.
    QuestStep,
    /// Narrative Beat + templates.
    Narrative,
    /// Place locus + In relation.
    PlaceShell,
    /// Zone Beat on a place.
    Zone,
    /// Knows-gated UI cue.
    UiCue,
    /// Performance encounter Beat + Cap law.
    ProductionEncounter,
    /// Feel contract: windows, curves, camera, haptics, accessibility.
    FeelContract,
}

/// Closed standard-library row.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct PatternSpec {
    /// Dotted id (`traversal.door_key`).
    pub id: &'static str,
    /// Published version.
    pub version: u32,
    /// Family.
    pub family: PatternFamily,
    /// Parameters.
    pub params: &'static [ParamSpec],
    /// `(param, affordance)` required on the host.
    pub requires: &'static [(&'static str, &'static str)],
    /// `(param, affordance)` granted by expansion.
    pub grants: &'static [(&'static str, &'static str)],
    /// `(param, affordance)` that must not already be granted.
    pub conflicts: &'static [(&'static str, &'static str)],
    /// Cost cap.
    pub budget: PatternBudget,
    /// Journey hook names.
    pub journeys: &'static [&'static str],
    /// Expansion template.
    pub kind: ExpandKind,
    /// Prior version this row migrates from, if any.
    pub from_version: Option<u32>,
}

/// Per-locus capability set used by the planner.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct HostCaps {
    granted: BTreeMap<String, BTreeSet<String>>,
}

impl HostCaps {
    /// Empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `locus` has `cap`.
    pub fn grant(&mut self, locus: &str, cap: &str) {
        self.granted
            .entry(locus.to_owned())
            .or_default()
            .insert(cap.to_owned());
    }

    /// True when `locus` already has `cap`.
    #[must_use]
    pub fn has(&self, locus: &str, cap: &str) -> bool {
        self.granted.get(locus).is_some_and(|set| set.contains(cap))
    }
}

/// Named journey hook emitted by expansion. Execution is KAI-06.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JourneyHook {
    /// Hook name (`unlocked`).
    pub name: Name,
    /// Pattern that declared it.
    pub pattern: Name,
    /// Instance that produced it.
    pub instance: Name,
}

/// Provenance from an expanded item back to the instance.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSpan {
    /// Instance identity.
    pub instance: AnchorId,
    /// Pattern id.
    pub pattern: Name,
    /// Pattern version mixed into child anchors.
    pub version: u32,
    /// Local id inside the pattern (`lockable`, `unlock`).
    pub local_id: Name,
}

/// Observed expansion cost.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct ExpansionCost {
    /// Predicate nodes.
    pub predicates: u32,
    /// Rite instructions.
    pub rite_steps: u32,
    /// Per-tick writers.
    pub per_tick: u32,
}

/// Pure expansion result. Ordinary IR only.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Expansion {
    /// Seed facts, canonical order.
    pub seed: Vec<SeedFact>,
    /// Canon patches, canonical order.
    pub canon_diffs: Vec<CanonDiff>,
    /// Mind specs, canonical order.
    pub minds: Vec<MindSpec>,
    /// Journey hooks, sorted by name.
    pub journeys: Vec<JourneyHook>,
    /// Child object identities.
    pub anchors: Vec<ObjectAnchor>,
    /// Spans back to the instance.
    pub spans: Vec<PatternSpan>,
    /// Observed cost.
    pub cost: ExpansionCost,
    /// Caps granted to host loci.
    pub grants: Vec<(Name, Name)>,
}

impl Expansion {
    /// Concatenate into an [`IntentDoc`] fragment. No pattern types.
    #[must_use]
    pub fn as_doc(&self) -> IntentDoc {
        IntentDoc {
            style: klotho_ir::StyleIntent::default(),
            canon_diffs: self.canon_diffs.clone(),
            seed: self.seed.clone(),
            minds: self.minds.clone(),
            provenance: klotho_ir::ProvenanceId(klotho_core::Hash::ZERO),
        }
    }
}

/// Catalog row consumed by `klotho-schema`.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct PatternCatalogRow {
    /// Pattern id.
    pub id: String,
    /// Version.
    pub version: u32,
    /// Family.
    pub family: String,
    /// Parameter names.
    pub parameters: Vec<String>,
    /// `param:cap` requires.
    pub requires: Vec<String>,
    /// `param:cap` grants.
    pub grants: Vec<String>,
    /// `param:cap` conflicts.
    pub conflicts: Vec<String>,
    /// Predicate budget.
    pub predicates: u32,
    /// Rite-step budget.
    pub rite_steps: u32,
    /// Per-tick budget.
    pub per_tick: u32,
}
