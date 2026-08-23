//! World construction / mutation failures. Not `KernelFault` or `RejectReason`.

use core::fmt;

/// Why a World write failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum WorldError {
    /// Hard cap of [`crate::MAX_LOCI`] loci.
    LocusCap,
    /// Sigil is not in the identity table.
    UnknownLocus,
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocusCap => write!(f, "LocusCap"),
            Self::UnknownLocus => write!(f, "UnknownLocus"),
        }
    }
}

impl core::error::Error for WorldError {}
