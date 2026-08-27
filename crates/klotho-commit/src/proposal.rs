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
    /// K18 sort key. Lower runs first: Player → Space → Motion → Mind → Infer.
    #[must_use]
    pub fn order_key(&self) -> u8 {
        match self {
            Self::Player(_) => 0,
            Self::SpaceDelta { .. } => 1,
            Self::MotionDelta { .. } => 2,
            Self::Mind(_) => 3,
            Self::Infer(_) => 4,
        }
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
