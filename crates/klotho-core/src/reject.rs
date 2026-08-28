//! Legal rejects vs kernel bugs. `step` never `Err`s on a legal reject (K19).

use crate::{AffordanceId, LawId, ResourceId};

/// Why a proposal was not committed. Recorded on `TraceDelta.rejects`.
///
/// Metaphor does not leak into this ABI: names stay literal.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum RejectReason {
    /// A continuous Law failed on the would-be post-state (K21 rollback).
    Law(LawId),
    /// Required cooked capability is absent on the locus.
    MissingAffordance(AffordanceId),
    /// Verb arrived outside its timing window.
    TimingMiss,
    /// Quantity / conservation failure (`SPEND` without stock, etc.).
    Resource(ResourceId),
    /// Infer or mind claimed a fact the locus does not `Knows`.
    HallucinatedFact,
    /// Job or intent older than `Budget.eval_slo_ticks`.
    StaleEpoch,
    /// `WAIT.channel` / agency claimed by a proposer that does not own it (K10).
    UnclaimedAgency,
    /// Hint said no overlap; kernel found `OpaqueClosed` in the derived swept volume (K24).
    WitnessMismatch,
    /// Proposer named a hull `BlobId` that is not the mover's canonical hull (K24).
    WrongHull,
    /// Same-tick write conflict after ordered commit (K18 / K21).
    Conflict,
    /// Pred-ops, rite-steps, or `us_sim` exhausted; fail closed.
    Budget,
    /// More contact groups this tick than [`crate::MAX_ISLANDS`].
    TooManyIslands,
    /// One contact group exceeded [`crate::MAX_ISLAND_SIZE`]. The island is
    /// omitted (not split).
    IslandTooLarge,
    /// Place load/evict failed closed (hash mismatch, cap, malformed snap, missing place).
    Residency,
    /// `canon_hash` or prefix does not match the live world.
    EpochMismatch,
}

/// Kernel invariant violation. The only `Err` `step` is allowed to return.
///
/// Legal rejects are **not** faults. If you find yourself constructing this
/// for "the door was locked", you have a design bug.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum KernelFault {
    /// Broken kernel invariant. String is static so faults stay out of Trace.
    Invariant(&'static str),
}

impl core::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Law(id) => write!(f, "Law({})", id.0),
            Self::MissingAffordance(id) => write!(f, "MissingAffordance({})", id.0),
            Self::TimingMiss => write!(f, "TimingMiss"),
            Self::Resource(id) => write!(f, "Resource({})", id.0),
            Self::HallucinatedFact => write!(f, "HallucinatedFact"),
            Self::StaleEpoch => write!(f, "StaleEpoch"),
            Self::UnclaimedAgency => write!(f, "UnclaimedAgency"),
            Self::WitnessMismatch => write!(f, "WitnessMismatch"),
            Self::WrongHull => write!(f, "WrongHull"),
            Self::Conflict => write!(f, "Conflict"),
            Self::Budget => write!(f, "Budget"),
            Self::TooManyIslands => write!(f, "TooManyIslands"),
            Self::IslandTooLarge => write!(f, "IslandTooLarge"),
            Self::Residency => write!(f, "Residency"),
            Self::EpochMismatch => write!(f, "EpochMismatch"),
        }
    }
}

impl core::fmt::Display for KernelFault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Invariant(msg) => write!(f, "kernel invariant: {msg}"),
        }
    }
}

impl core::error::Error for KernelFault {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_rejects_are_copy_and_named_literally() {
        let reasons = [
            RejectReason::WrongHull,
            RejectReason::Conflict,
            RejectReason::WitnessMismatch,
            RejectReason::UnclaimedAgency,
            RejectReason::Budget,
            RejectReason::TooManyIslands,
            RejectReason::IslandTooLarge,
            RejectReason::Residency,
            RejectReason::EpochMismatch,
        ];
        for r in reasons {
            let s = r.to_string();
            assert!(!s.contains(' '), "{s}");
        }
    }
}
