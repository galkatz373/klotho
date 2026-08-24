//! Closed v1 verb set (Hearth + Ash). New verbs are an engine change, not a component.

use serde::{Deserialize, Serialize};

/// What an intent asks to do. `Time` is the WAIT-window verb; only
/// [`crate::PlayerIntent`] may claim the Timing channel (K10).
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Verb {
    /// Observer yaw/pitch.
    Look,
    /// Locomotion request (Space/Motion will propose a delta).
    Move,
    /// Generic use; starts a rite when Canon says so (lockpick, ignite).
    Use,
    /// Pick up a Portable relic.
    Carry,
    /// Inverse of Carry.
    Drop,
    /// Settle `Owes` in copper.
    Pay,
    /// Ash hitscan.
    Fire,
    /// Advance a `WAIT` window. Infer/Mind with this verb is `UnclaimedAgency`.
    Time,
    /// Verb, not an affordance bit. Unlock is `RelDel LockedBy`.
    Open,
    /// DialogueChoice channel.
    Talk,
    /// Mind: investigate NoiseHigh.
    Investigate,
}

impl Verb {
    /// Stable discriminant. ClipSet blobs and kitbash lock this order.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Inverse of [`Self::as_u8`]. `None` for unknown future values.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Look),
            1 => Some(Self::Move),
            2 => Some(Self::Use),
            3 => Some(Self::Carry),
            4 => Some(Self::Drop),
            5 => Some(Self::Pay),
            6 => Some(Self::Fire),
            7 => Some(Self::Time),
            8 => Some(Self::Open),
            9 => Some(Self::Talk),
            10 => Some(Self::Investigate),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(Verb::Look.as_u8(), 0);
        assert_eq!(Verb::Move.as_u8(), 1);
        assert_eq!(Verb::Investigate.as_u8(), 10);
        assert_eq!(Verb::from_u8(1), Some(Verb::Move));
        assert_eq!(Verb::from_u8(11), None);
    }
}
