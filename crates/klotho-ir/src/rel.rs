//! Relation labels. Unary marks (`Dead`) are self-relations at seed time.

use serde::{Deserialize, Serialize};

/// Projection relation. Not a component. Wire tags match declaration order.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Rel {
    /// Containment in a Place.
    In = 0,
    /// Ownership.
    OwnedBy = 1,
    /// Grasp / wield.
    WieldedBy = 2,
    /// Key binding (`iron_key` keys `oak_door`).
    KeyedBy = 3,
    /// Mind knows a fact. Object is a fact name at authoring time.
    Knows = 4,
    /// Trade debt.
    Owes = 5,
    /// Mind attitude.
    Fears = 6,
    /// Assembly.
    PartOf = 7,
    /// Provenance-ish in-world derivation (not the prove DAG).
    DerivedFrom = 8,
    /// Lock. `LockedBy` self ⇒ `OpaqueClosed`.
    LockedBy = 9,
    /// Ash death mark. Seed as `Rel(actor, Dead, actor)` or a Beat-written edge.
    Dead = 10,
    /// Vehicle / possessed body. Object is the pilot.
    PilotedBy = 11,
    /// Attach parent. Object is the carrier locus.
    AttachedTo = 12,
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
            Self::PilotedBy => "PilotedBy",
            Self::AttachedTo => "AttachedTo",
        }
    }

    /// Frozen wire tag (`In` = 0).
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Inverse of [`Self::as_u8`]. `None` for unknown future values.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::In),
            1 => Some(Self::OwnedBy),
            2 => Some(Self::WieldedBy),
            3 => Some(Self::KeyedBy),
            4 => Some(Self::Knows),
            5 => Some(Self::Owes),
            6 => Some(Self::Fears),
            7 => Some(Self::PartOf),
            8 => Some(Self::DerivedFrom),
            9 => Some(Self::LockedBy),
            10 => Some(Self::Dead),
            11 => Some(Self::PilotedBy),
            12 => Some(Self::AttachedTo),
            _ => None,
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
            "PilotedBy" => Some(Self::PilotedBy),
            "AttachedTo" => Some(Self::AttachedTo),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_tags_are_append_only() {
        for i in 0u8..=10 {
            assert!(Rel::from_u8(i).is_some(), "old tag {i}");
        }
        assert_eq!(Rel::PilotedBy.as_u8(), 11);
        assert_eq!(Rel::AttachedTo.as_u8(), 12);
        assert_eq!(Rel::from_u8(11), Some(Rel::PilotedBy));
        assert_eq!(Rel::from_u8(12), Some(Rel::AttachedTo));
        assert_eq!(Rel::from_u8(13), None);
        assert_eq!(Rel::PartOf.as_u8(), 7);
    }
}
