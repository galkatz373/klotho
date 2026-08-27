//! Proposals. Only [`crate::CommitKernel`] commits them.

use klotho_core::{BlobId, HullWitness, IVec3, PoseMm, Sigil, Vel3};
use klotho_ir::{InferIntent, MindIntent, PlayerIntent};
use klotho_trace::ProposalKind;

/// One transaction grain (K21).
#[derive(Clone, Debug)]
pub enum Proposal {
    /// Signed / device player packet.
    Player(PlayerIntent),
    /// GOAP desire.
    Mind(MindIntent),
    /// Model fill. No Agency.
    Infer(InferIntent),
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
}

impl Proposal {
    /// K18 class. Lower runs first.
    ///
    /// Player 0, Residency 1, Phys 2, Space 3, Motion 4, Mind 5, Infer 6.
    /// Residency / Phys variants land in later PRs; the holes stay reserved.
    #[must_use]
    pub fn order_key(&self) -> u8 {
        match self {
            Self::Player(_) => 0,
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
            Self::SpaceDelta { mover, .. } | Self::MotionDelta { mover, .. } => mover.raw(),
        }
    }

    /// Island id on spatial proposals; 0 for intent grains.
    #[must_use]
    pub fn island(&self) -> u16 {
        match self {
            Self::SpaceDelta { island, .. } | Self::MotionDelta { island, .. } => *island,
            Self::Player(_) | Self::Mind(_) | Self::Infer(_) => 0,
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
            Self::SpaceDelta { .. } => ProposalKind::Space,
            Self::MotionDelta { .. } => ProposalKind::Motion,
        }
    }
}
