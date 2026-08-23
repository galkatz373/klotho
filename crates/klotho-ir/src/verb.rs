//! Closed v1 verb set (Hearth + Ash). New verbs are an engine change, not a component.

use serde::{Deserialize, Serialize};

/// What an intent asks to do. `Time` is the WAIT-window verb; only
/// [`crate::PlayerIntent`] may claim the Timing channel (K10).
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
