//! Proposals. Only [`crate::CommitKernel`] commits them.

use std::sync::Arc;

use klotho_core::{
    BlobId, Epoch, Hash, HullWitness, IVec3, PoseMm, ShapeKind, Sigil, Support, Tick, Vel3,
};
use klotho_ir::{InferIntent, MindIntent, PlayerIntent};
use klotho_trace::ProposalKind;
use klotho_world::PlaceSnap;

/// Place load or evict.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum ResidencyOp {
    /// Insert every snap row or none.
    Load,
    /// Drop place-owned rows (migrating attach/pilot kept).
    Evict,
}

/// Maximum body deltas in one atomic physical island (K60).
pub const MAX_PHYS_ISLAND_BODIES: usize = 256;
/// Maximum partition members named by one physical island (K58/K60).
pub const MAX_PHYS_ISLAND_MEMBERS: usize = 256;
/// Maximum gameplay contact claims in one physical island (K60).
pub const MAX_PHYS_ISLAND_CONTACTS: usize = 1_024;
/// Maximum participating constraints in one physical island (K60).
pub const MAX_PHYS_ISLAND_CONSTRAINTS: usize = 512;
/// Maximum constraint-break claims in one physical island (K60).
pub const MAX_PHYS_ISLAND_BREAKS: usize = 256;
/// Maximum attached children included in one island write set (K60).
pub const MAX_PHYS_ISLAND_CHILDREN: usize = 256;
/// Maximum distinct loci written by one physical island (K60).
pub const MAX_PHYS_ISLAND_WRITE_LOCI: usize = 512;

/// One quantized body result inside an atomic physical island.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct BodyDelta {
    /// Body identity.
    pub mover: Sigil,
    /// Proposed pose.
    pub pose: PoseMm,
    /// Proposed linear velocity.
    pub vel: Vel3,
    /// Yaw rate, millidegrees per tick.
    pub yaw_rate: i32,
    /// Pitch rate, millidegrees per tick.
    pub pitch_rate: i32,
    /// Roll rate, millidegrees per tick.
    pub roll_rate: i32,
    /// Sleep ticks. Zero when an island mate received an impulse.
    pub sleep_ticks: u16,
    /// Canonical hull binding observed by the proposer.
    pub hull: BlobId,
    /// K24 hint; the kernel derives swept geometry.
    pub witness: HullWitness,
    /// Quantized contact support.
    pub support: Option<Support>,
}

/// Bounded gameplay-visible contact claim. Kernel reproduces the witness.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ContactClaim {
    /// Canonical lower endpoint. Must be less than `b`.
    pub a: Sigil,
    /// Canonical upper endpoint.
    pub b: Sigil,
    /// Shape binding for `a`.
    pub shape_a: BlobId,
    /// Shape binding for `b`.
    pub shape_b: BlobId,
    /// Cooked shape kind for `a`.
    pub kind_a: ShapeKind,
    /// Cooked shape kind for `b`.
    pub kind_b: ShapeKind,
    /// Deterministic solver feature id.
    pub feature: u16,
    /// Quantized A-side witness. Broad-phase AABB is never enough.
    pub witness: HullWitness,
}

impl ContactClaim {
    pub(crate) fn key(&self) -> (Sigil, Sigil, BlobId, BlobId, u16) {
        (self.a, self.b, self.shape_a, self.shape_b, self.feature)
    }
}

/// Canon constraint participating in an island solve.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ConstraintRef {
    /// Stable constraint identity.
    pub constraint: Sigil,
    /// Canon binding observed by the proposer.
    pub binding: BlobId,
}

/// Proposed semantic break. Constraint admission lands in PHYS-A05.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ConstraintBreakClaim {
    /// Stable constraint identity.
    pub constraint: Sigil,
    /// Quantized solver impulse witness.
    pub impulse: i32,
}

/// One transaction grain (K21).
#[derive(Clone, Debug, PartialEq)]
pub enum Proposal {
    /// Signed / device player packet.
    Player(PlayerIntent),
    /// GOAP desire.
    Mind(MindIntent),
    /// Model fill. No Agency.
    Infer(InferIntent),
    /// Coupled physical solution admitted as one K21 transaction (K59).
    PhysIsland {
        /// Canon epoch observed by the proposer.
        epoch: Epoch,
        /// Authoritative tick this solution targets.
        tick: Tick,
        /// This-tick K58 partition id.
        island: u16,
        /// Exact sorted K58 membership. Actors may be members while Motion owns them.
        members: Vec<Sigil>,
        /// Sorted subset whose physical rows are updated by this solver.
        bodies: Vec<BodyDelta>,
        /// Sorted gameplay-visible contact claims.
        contacts: Vec<ContactClaim>,
        /// Sorted participating constraints. Non-empty is fail-closed until PHYS-A05.
        constraints: Vec<ConstraintRef>,
        /// Sorted break claims. Non-empty is fail-closed until PHYS-A05.
        breaks: Vec<ConstraintBreakClaim>,
    },
    /// Space-admitted motion of one mover.
    SpaceDelta {
        /// Mover (duplicated on the witness).
        mover: Sigil,
        /// Proposed pose.
        pose: PoseMm,
        /// Linear velocity.
        vel: Vel3,
        /// Yaw rate, millideg / tick.
        yaw_rate: i32,
        /// Island id.
        island: u16,
        /// Sleep ticks.
        sleep_ticks: u16,
        /// Canonical hull the proposer believes it is moving. Mismatch → `WrongHull`.
        hull: BlobId,
        /// K24 hint. Kernel derives swept; ignores proposer swept.
        witness: HullWitness,
    },
    /// Motion clip root. Same admission as Space, after Space (K18).
    MotionDelta {
        /// Mover.
        mover: Sigil,
        /// Proposed pose.
        pose: PoseMm,
        /// Linear velocity.
        vel: Vel3,
        /// Yaw rate.
        yaw_rate: i32,
        /// Island id.
        island: u16,
        /// Sleep ticks.
        sleep_ticks: u16,
        /// Clip table index (opaque to the kernel).
        clip: u16,
        /// Root translation hint (not trusted for swept).
        root: IVec3,
        /// Hull id.
        hull: BlobId,
        /// K24 hint.
        witness: HullWitness,
    },
    /// Place load / evict. Conflict set is every snap row plus the Place.
    Residency {
        /// Place being loaded or evicted.
        place: Sigil,
        /// Load or evict.
        op: ResidencyOp,
        /// Prefix stamped on the proposal (must match the live world).
        prefix: Hash,
        /// Cook digest stamped on the proposal (must match the live world and snap).
        canon_hash: Hash,
        /// Shared payload; `place` / `canon_hash` must match this proposal.
        snap: Arc<PlaceSnap>,
    },
}

impl Proposal {
    /// K18 class. Lower runs first.
    ///
    /// Player 0, Residency 1, Phys 2, Space 3, Motion 4, Mind 5, Infer 6.
    #[must_use]
    pub fn order_key(&self) -> u8 {
        match self {
            Self::Player(_) => 0,
            Self::Residency { .. } => 1,
            Self::PhysIsland { .. } => 2,
            Self::SpaceDelta { .. } => 3,
            Self::MotionDelta { .. } => 4,
            Self::Mind(_) => 5,
            Self::Infer(_) => 6,
        }
    }

    /// Mover identity for the total admit key. Player uses the local slot.
    #[must_use]
    pub fn mover_raw(&self) -> u128 {
        match self {
            Self::Player(p) => u128::from(p.player.0),
            Self::Mind(m) => m.locus.raw(),
            Self::Infer(i) => i.locus.map(klotho_core::Sigil::raw).unwrap_or(0),
            Self::PhysIsland { members, .. } => members.first().map_or(0, |s| s.raw()),
            Self::SpaceDelta { mover, .. } | Self::MotionDelta { mover, .. } => mover.raw(),
            Self::Residency { place, .. } => place.raw(),
        }
    }

    /// Island id on spatial proposals; 0 for intent grains.
    #[must_use]
    pub fn island(&self) -> u16 {
        match self {
            Self::PhysIsland { island, .. }
            | Self::SpaceDelta { island, .. }
            | Self::MotionDelta { island, .. } => *island,
            Self::Player(_) | Self::Mind(_) | Self::Infer(_) | Self::Residency { .. } => 0,
        }
    }

    /// Admission equivalence key (K18 / K34). Equal keys never fall back to
    /// insertion order: distinct grains in one class all Conflict, while
    /// byte-identical grains collapse to a single admit (idempotent retry).
    #[must_use]
    pub fn admit_key(&self, proposer_reg_ix: u8) -> (u8, u128, u16, u8) {
        (
            self.order_key(),
            self.mover_raw(),
            self.island(),
            proposer_reg_ix,
        )
    }

    /// Net / reject kind.
    #[must_use]
    pub fn kind(&self) -> ProposalKind {
        match self {
            Self::Player(_) => ProposalKind::Player,
            Self::Mind(_) => ProposalKind::Mind,
            Self::Infer(_) => ProposalKind::Infer,
            Self::PhysIsland { .. } => ProposalKind::Phys,
            Self::SpaceDelta { .. } => ProposalKind::Space,
            Self::MotionDelta { .. } => ProposalKind::Motion,
            Self::Residency { .. } => ProposalKind::Residency,
        }
    }
}
