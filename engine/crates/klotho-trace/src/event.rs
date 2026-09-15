//! Admitted Trace events. Rejected proposals never land here.

use klotho_core::{PoseMm, ResourceId, Sigil, Tick, Vel3};

use crate::error::TraceError;

/// Who proposed. Discriminants are frozen; net and `TraceDelta.rejects` use them.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum ProposalKind {
    /// Signed `PlayerIntent`.
    Player = 1,
    /// GOAP `MindIntent`.
    Mind = 2,
    /// Space proposer.
    Space = 3,
    /// Motion proposer.
    Motion = 4,
    /// `InferIntent`.
    Infer = 5,
    /// Phys proposer.
    Phys = 6,
    /// Place load / evict.
    Residency = 7,
}

impl ProposalKind {
    /// Frozen discriminant.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Inverse of [`Self::as_u8`].
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Player),
            2 => Some(Self::Mind),
            3 => Some(Self::Space),
            4 => Some(Self::Motion),
            5 => Some(Self::Infer),
            6 => Some(Self::Phys),
            7 => Some(Self::Residency),
            _ => None,
        }
    }
}

/// Why a semantic pose was committed (interaction rate, not 60 Hz).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum PoseReason {
    /// Generic interact (use, unlatch).
    Interact = 1,
    /// Landed after a move.
    Land = 2,
    /// Picked up / `RelAdd WieldedBy`.
    Pick = 3,
    /// Dropped / `RelDel WieldedBy`.
    Drop = 4,
    /// Door hinge yaw.
    Hinge = 5,
}

/// How a rite finished. `FailBudget` is the cap path, not a Law reject.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum RiteEnd {
    /// `COMPLETE Success` / `HALT Success`.
    Success = 0,
    /// `COMPLETE Fail` / `HALT Fail`.
    Fail = 1,
    /// Per-rite or per-tick step cap exceeded.
    FailBudget = 2,
    /// Place evict / epoch halt. Not [`Self::FailBudget`].
    Evicted = 3,
}

impl RiteEnd {
    /// Frozen discriminant.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Inverse of [`Self::as_u8`]. Unknown tags are `None`.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Success),
            1 => Some(Self::Fail),
            2 => Some(Self::FailBudget),
            3 => Some(Self::Evicted),
            _ => None,
        }
    }
}

/// Relation tag. Values match `klotho_ir::Rel` declaration order (0 = `In`).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct RelTag(pub u8);

impl RelTag {
    /// `In`.
    pub const IN: Self = Self(0);
    /// `OwnedBy`.
    pub const OWNED_BY: Self = Self(1);
    /// `WieldedBy`.
    pub const WIELDED_BY: Self = Self(2);
    /// `KeyedBy`.
    pub const KEYED_BY: Self = Self(3);
    /// `Knows`.
    pub const KNOWS: Self = Self(4);
    /// `Owes`.
    pub const OWES: Self = Self(5);
    /// `Fears`.
    pub const FEARS: Self = Self(6);
    /// `PartOf`.
    pub const PART_OF: Self = Self(7);
    /// `DerivedFrom`.
    pub const DERIVED_FROM: Self = Self(8);
    /// `LockedBy`.
    pub const LOCKED_BY: Self = Self(9);
    /// `Dead`.
    pub const DEAD: Self = Self(10);
    /// `PilotedBy`.
    pub const PILOTED_BY: Self = Self(11);
    /// `AttachedTo`.
    pub const ATTACHED_TO: Self = Self(12);
}

/// One admitted event, stamped with the tick it committed on.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct TraceEvent {
    /// Commit tick.
    pub tick: Tick,
    /// Payload.
    pub body: TraceBody,
}

impl TraceEvent {
    /// Stamp a body.
    #[must_use]
    pub const fn new(tick: Tick, body: TraceBody) -> Self {
        Self { tick, body }
    }
}

/// Event payload. Tags are frozen in [`crate::encode`].
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum TraceBody {
    /// Rite admitted; `RiteMachine` row created.
    RiteBegan {
        /// Acting locus.
        actor: Sigil,
        /// Cooked rite table index.
        rite: u16,
        /// Bound target, if any.
        target: Option<Sigil>,
    },
    /// `WAIT` committed the burst (K21). Resume is a new transaction.
    RiteAdvanced {
        /// Acting locus.
        actor: Sigil,
        /// Cooked rite table index.
        rite: u16,
        /// PC after the wait.
        pc: u16,
        /// Ticks remaining on this WAIT.
        wait_left: u16,
    },
    /// Rite left the table.
    RiteEnded {
        /// Acting locus.
        actor: Sigil,
        /// Cooked rite table index.
        rite: u16,
        /// How it ended.
        status: RiteEnd,
    },
    /// Quantity crossed a quantum, hit cap/ignite, or was written on interact.
    QtyChanged {
        /// Locus.
        id: Sigil,
        /// Resource.
        res: ResourceId,
        /// New value.
        to: i32,
        /// Quantum that fired (`heat` = 10).
        quantum: i32,
    },
    /// Coarse 2 Hz snapshot of awake posed island members.
    IslandSnap(IslandSnap),
    /// Interaction-rate semantic pose (6DOF).
    PoseCommitted {
        /// Locus.
        s: Sigil,
        /// Committed pose.
        pose: PoseMm,
        /// Why this pose is in Trace.
        reason: PoseReason,
    },
    /// Diegetic in-play save request. Runtime writes the K19 quadruple.
    SaveRequested,
    /// Mind knows a fact.
    Learned {
        /// Mind locus.
        mind: Sigil,
        /// Interned fact id.
        fact: u16,
    },
    /// Relation inserted.
    RelAdd {
        /// Subject.
        a: Sigil,
        /// Edge.
        rel: RelTag,
        /// Object.
        b: Sigil,
    },
    /// Relation deleted.
    RelDel {
        /// Subject.
        a: Sigil,
        /// Edge.
        rel: RelTag,
        /// Object.
        b: Sigil,
    },
    /// Rite `EMIT` (NoiseLow, Unlocked, …). `kind` is a Canon intern.
    Emitted {
        /// Interned event kind.
        kind: u16,
        /// Primary slot.
        a: Sigil,
        /// Optional second slot.
        b: Option<Sigil>,
    },
    /// Verified player origin of a Canon-bound contact action. Proposers do
    /// not author this event or acquire Agency from it.
    MotionActionAuthorized {
        /// Actor owning the action.
        actor: Sigil,
        /// Canon Rite identity.
        rite: u16,
        /// Exact Rite instance.
        instance: Tick,
        /// Original player claim channel.
        channel: u8,
    },
    /// Admitted semantic instrument contact; sampled bones stay off Trace.
    MotionContactAdmitted {
        /// Actor carrying the instrument.
        actor: Sigil,
        /// Canon instrument binding identity.
        instrument: klotho_core::Hash,
        /// Semantic target.
        target: Sigil,
        /// Active Rite identity.
        rite: u16,
        /// Exact action instance start tick.
        instance: Tick,
        /// Authenticated action channel wire tag.
        channel: u8,
        /// Authoritative contact interval.
        boundary: u16,
    },
    /// Host-committed utterance. Client cosmetics cannot add facts.
    Uttered {
        /// Speaker.
        speaker: Sigil,
        /// Interned fact ids claimed.
        fact_ids: Vec<u16>,
    },
    /// Recorded Place load. Apply is later.
    PlaceLoaded {
        /// Place locus.
        place: Sigil,
        /// Recorded row count.
        n: u32,
    },
    /// Recorded Place evict. Apply is later.
    PlaceEvicted {
        /// Place locus.
        place: Sigil,
    },
    /// Recorded spawn. Apply inserts a Relic at `sigil` and pose `at`.
    Spawned {
        /// Interned fact name (not a cooked locus).
        template: u16,
        /// Newly allocated locus.
        sigil: Sigil,
        /// Pose at emit.
        at: PoseMm,
    },
    /// Recorded despawn. Apply is later.
    Despawned {
        /// Retired sigil.
        sigil: Sigil,
        /// Recorded generation.
        generation: u8,
    },
}

/// Ticks between coarse [`IslandSnap`] events at Hearth 60 Hz (2 Hz).
pub const ISLAND_SNAP_PERIOD_TICKS: u64 = 30;

/// Parallel-array island snapshot. Lengths must match.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct IslandSnap {
    /// Island id.
    pub island: u16,
    /// Members, in the order of the other columns.
    pub members: Vec<Sigil>,
    /// Pose per member.
    pub poses: Vec<PoseMm>,
    /// Linear velocity per member.
    pub vels: Vec<Vel3>,
    /// Yaw rate (millideg / tick) per member.
    pub yaw_rates: Vec<i32>,
    /// Sleep ticks per member. `0` is awake.
    pub sleep_ticks: Vec<u16>,
}

impl IslandSnap {
    /// Build a snap. All columns must have the same length as `members`.
    pub fn new(
        island: u16,
        members: Vec<Sigil>,
        poses: Vec<PoseMm>,
        vels: Vec<Vel3>,
        yaw_rates: Vec<i32>,
        sleep_ticks: Vec<u16>,
    ) -> Result<Self, TraceError> {
        let n = members.len();
        if poses.len() != n || vels.len() != n || yaw_rates.len() != n || sleep_ticks.len() != n {
            return Err(TraceError::SnapLen);
        }
        Ok(Self {
            island,
            members,
            poses,
            vels,
            yaw_rates,
            sleep_ticks,
        })
    }

    /// Number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// No members.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}
