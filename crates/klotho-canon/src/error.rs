//! Cook-time failures. None of these are `KernelFault` or `RejectReason`.

use core::fmt;

/// Why a Canon document failed to cook.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CookError {
    /// Graph mixes unlabeled ops and explicit pcs.
    MixedLabeling,
    /// Two nodes share a pc.
    DuplicatePc(u16),
    /// `entry` is not a defined pc.
    MissingEntry(u16),
    /// Jump / fail target is not a defined pc.
    MissingTarget {
        /// Source pc.
        from: u16,
        /// Missing destination.
        to: u16,
    },
    /// Node is not reachable from `entry`.
    Unreachable(u16),
    /// A non-halt op would fall off the end of the graph.
    FallOff(u16),
    /// Successor graph contains a cycle.
    Cycle,
    /// `Name(id)` is not a seed locus / declared id.
    UnboundName(String),
}

impl fmt::Display for CookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MixedLabeling => write!(f, "MixedLabeling"),
            Self::DuplicatePc(pc) => write!(f, "DuplicatePc({pc})"),
            Self::MissingEntry(pc) => write!(f, "MissingEntry({pc})"),
            Self::MissingTarget { from, to } => write!(f, "MissingTarget({from}->{to})"),
            Self::Unreachable(pc) => write!(f, "Unreachable({pc})"),
            Self::FallOff(pc) => write!(f, "FallOff({pc})"),
            Self::Cycle => write!(f, "Cycle"),
            Self::UnboundName(n) => write!(f, "UnboundName({n})"),
        }
    }
}

impl core::error::Error for CookError {}
