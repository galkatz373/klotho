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
    /// IntentDoc failed structural validation.
    InvalidDoc(String),
    /// Duplicate Law / Affordance / Rite / Beat / seed locus id.
    DuplicateId(String),
    /// A retraction named a declaration that is not in the draft.
    UnknownRetract(String),
    /// Compiled pred exceeds [`crate::PRED_OPS_PER_EVAL`].
    PredTooLarge,
    /// Packed table id space exhausted.
    TableFull,
    /// Tiny-fragment Law `must` is unsatisfiable, or two Laws contradict.
    Contradiction(String),
    /// `Lockable` is declared without a key-or-rite admission pred.
    LockableNeedsKeyOrRite,
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
            Self::InvalidDoc(s) => write!(f, "InvalidDoc({s})"),
            Self::DuplicateId(s) => write!(f, "DuplicateId({s})"),
            Self::UnknownRetract(s) => write!(f, "UnknownRetract({s})"),
            Self::PredTooLarge => write!(f, "PredTooLarge"),
            Self::TableFull => write!(f, "TableFull"),
            Self::Contradiction(s) => write!(f, "Contradiction({s})"),
            Self::LockableNeedsKeyOrRite => write!(f, "LockableNeedsKeyOrRite"),
        }
    }
}

impl core::error::Error for CookError {}
