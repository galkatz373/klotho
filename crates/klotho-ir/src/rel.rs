//! Relation labels. Unary marks (`Dead`) are self-relations at seed time.

use serde::{Deserialize, Serialize};

/// Projection relation. Not a component.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Rel {
    /// Containment in a Place.
    In,
    /// Ownership.
    OwnedBy,
    /// Grasp / wield.
    WieldedBy,
    /// Key binding (`iron_key` keys `oak_door`).
    KeyedBy,
    /// Mind knows a fact. Object is a fact name at authoring time.
    Knows,
    /// Trade debt.
    Owes,
    /// Mind attitude.
    Fears,
    /// Assembly.
    PartOf,
    /// Provenance-ish in-world derivation (not the prove DAG).
    DerivedFrom,
    /// Lock. `LockedBy` self ⇒ `OpaqueClosed`.
    LockedBy,
    /// Ash death mark. Seed as `Rel(actor, Dead, actor)` or a Beat-written edge.
    Dead,
}
