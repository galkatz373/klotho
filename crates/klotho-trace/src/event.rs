//! Admitted Trace events. Rejected proposals never land here.

use klotho_core::{Mm, PoseMm, ResourceId, Sigil, Tick, VelFx, YawMd};

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
    /// 10 Hz (or sleep/interact) island physics snapshot. Not 60 Hz into the log.
    IslandSnap(IslandSnap),
    /// Interaction-rate semantic pose.
    PoseCommitted {
        /// Locus.
        s: Sigil,
        /// Ground-plane millimetres.
        xz: (Mm, Mm),
        /// Yaw.
        yaw: YawMd,
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
    /// Host-committed utterance. Client cosmetics cannot add facts.
    Uttered {
        /// Speaker.
        speaker: Sigil,
        /// Interned fact ids claimed.
        fact_ids: Vec<u16>,
    },
}

/// Parallel-array island snapshot. Lengths must match.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct IslandSnap {
    /// Island id.
    pub island: u16,
    /// Members, in the order of the other columns.
    pub members: Vec<Sigil>,
    /// Pose per member.
    pub poses: Vec<PoseMm>,
    /// `(vel_x, vel_z)` per member.
    pub vels: Vec<(VelFx, VelFx)>,
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
        vels: Vec<(VelFx, VelFx)>,
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
