//! Proposals. Only [`crate::CommitKernel`] commits them.

use std::sync::Arc;

use klotho_core::{BlobId, Hash, HullWitness, IVec3, PoseMm, Sigil, Support, Vel3};
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

/// One transaction grain (K21).
#[derive(Clone, Debug)]
pub enum Proposal {
    /// Signed / device player packet.
    Player(PlayerIntent),
    /// GOAP desire.
    Mind(MindIntent),
    /// Model fill. No Agency.
    Infer(InferIntent),
    /// Quantized rigid-body delta. Kernel does not re-solve.
    PhysDelta {
        /// Mover (duplicated on the witness).
        mover: Sigil,
        /// Proposed pose.
        pose: PoseMm,
        /// Linear velocity.
        vel: Vel3,
        /// Yaw rate, millideg / tick.
        yaw_rate: i32,
        /// Pitch rate, millideg / tick.
        pitch_rate: i32,
        /// Roll rate, millideg / tick.
        roll_rate: i32,
        /// Island id.
        island: u16,
        /// Sleep ticks. Zero when this island mate received an impulse.
        sleep_ticks: u16,
        /// Canonical hull the proposer believes it is moving. Mismatch → `WrongHull`.
        hull: BlobId,
        /// K24 hint. Kernel derives swept; ignores proposer swept.
        witness: HullWitness,
        /// Contact support `(nx, ny, nz, depth_mm)`.
        support: Option<Support>,
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
            Self::PhysDelta { .. } => 2,
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
            Self::PhysDelta { mover, .. }
            | Self::SpaceDelta { mover, .. }
            | Self::MotionDelta { mover, .. } => mover.raw(),
            Self::Residency { place, .. } => place.raw(),
        }
    }

    /// Island id on spatial proposals; 0 for intent grains.
    #[must_use]
    pub fn island(&self) -> u16 {
        match self {
            Self::PhysDelta { island, .. }
            | Self::SpaceDelta { island, .. }
            | Self::MotionDelta { island, .. } => *island,
            Self::Player(_) | Self::Mind(_) | Self::Infer(_) | Self::Residency { .. } => 0,
        }
    }

    /// Total admit comparator (K18 / K34). Not insertion order.
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
            Self::PhysDelta { .. } => ProposalKind::Phys,
            Self::SpaceDelta { .. } => ProposalKind::Space,
            Self::MotionDelta { .. } => ProposalKind::Motion,
            Self::Residency { .. } => ProposalKind::Residency,
        }
    }
}
