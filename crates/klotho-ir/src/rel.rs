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

impl Rel {
    /// Unit name (`"LockedBy"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::In => "In",
            Self::OwnedBy => "OwnedBy",
            Self::WieldedBy => "WieldedBy",
            Self::KeyedBy => "KeyedBy",
            Self::Knows => "Knows",
            Self::Owes => "Owes",
            Self::Fears => "Fears",
            Self::PartOf => "PartOf",
            Self::DerivedFrom => "DerivedFrom",
            Self::LockedBy => "LockedBy",
            Self::Dead => "Dead",
        }
    }

    /// Parse a unit name or Appendix A string (`"LockedBy"`).
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "In" => Some(Self::In),
            "OwnedBy" => Some(Self::OwnedBy),
            "WieldedBy" => Some(Self::WieldedBy),
            "KeyedBy" => Some(Self::KeyedBy),
            "Knows" => Some(Self::Knows),
            "Owes" => Some(Self::Owes),
            "Fears" => Some(Self::Fears),
            "PartOf" => Some(Self::PartOf),
            "DerivedFrom" => Some(Self::DerivedFrom),
            "LockedBy" => Some(Self::LockedBy),
            "Dead" => Some(Self::Dead),
            _ => None,
        }
    }
}
