//! Deterministic pattern compiler and first-title standard library (KAI-05).
//!
//! Patterns are authoring macros (K63). Expansion is a pure function of locked
//! module bytes and arguments: no RNG, no runtime pattern type, no Rust, no
//! proposers. Generated names come from `(module, instance, local)`; child
//! anchors come from `(instance AnchorId, version, local-id)`.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod def;
mod error;
mod expand;
mod migrate;
mod stdlib;

pub use def::{
    ExpandKind, Expansion, ExpansionCost, HostCaps, JourneyHook, ParamSpec, PatternBudget,
    PatternCatalogRow, PatternFamily, PatternSpan, PatternSpec, StaticValue,
};
pub use error::PatternError;
pub use expand::{bind_args, caps_from_module, expand_bundle, expand_instance, expand_module};
pub use migrate::{migrate_instance, migration_journeys};
pub use stdlib::{first_pattern_ids, latest, lookup, specs};

use crate::def::PatternCatalogRow as Row;

/// Schema-catalog rows, sorted by `(id, version)`.
#[must_use]
pub fn catalog_rows() -> Vec<PatternCatalogRow> {
    let mut rows: Vec<Row> = specs()
        .iter()
        .map(|s| Row {
            id: s.id.to_owned(),
            version: s.version,
            family: s.family.as_str().to_owned(),
            parameters: s.params.iter().map(|p| p.name.to_owned()).collect(),
            requires: s.requires.iter().map(|(p, c)| format!("{p}:{c}")).collect(),
            grants: s.grants.iter().map(|(p, c)| format!("{p}:{c}")).collect(),
            conflicts: s
                .conflicts
                .iter()
                .map(|(p, c)| format!("{p}:{c}"))
                .collect(),
            predicates: s.budget.predicates,
            rite_steps: s.budget.rite_steps,
            per_tick: s.budget.per_tick,
        })
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id).then(a.version.cmp(&b.version)));
    rows
}

#[cfg(test)]
mod tests;
