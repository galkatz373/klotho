//! World construction / mutation / snapshot-blob failures. Not `KernelFault` or `RejectReason`.

use core::fmt;

/// Why a World write failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum WorldError {
    /// Hard cap of this world's locus budget.
    LocusCap,
    /// Sigil is not in the identity table.
    UnknownLocus,
    /// PlaceSnap was oversized, duplicated, or otherwise unusable. Not truncated.
    PlaceSnap,
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocusCap => write!(f, "LocusCap"),
            Self::UnknownLocus => write!(f, "UnknownLocus"),
            Self::PlaceSnap => write!(f, "PlaceSnap"),
        }
    }
}

impl core::error::Error for WorldError {}

/// Why a projection snapshot blob could not be encoded or decoded.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum SnapError {
    /// Magic bytes were not `KSNP`.
    Magic,
    /// Version byte is not the one this crate decodes.
    Version(u8),
    /// Header pad bytes were not zeros.
    Pad,
    /// Buffer ended before a field.
    Truncated,
    /// Declared or assembled size exceeded a cap. Not truncated: do not allocate `size`.
    Oversize {
        /// Observed or declared size.
        size: usize,
        /// Cap that was applied first.
        cap: usize,
    },
    /// Extra bytes after a well-formed blob.
    Trailing,
    /// Kind, relation, LOD, or channel tag is not in the frozen set.
    Kind,
    /// Duplicate sigil or more than one `Rel::In` on a row.
    Duplicate,
}

impl fmt::Display for SnapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Magic => write!(f, "Magic"),
            Self::Version(v) => write!(f, "Version({v})"),
            Self::Pad => write!(f, "Pad"),
            Self::Truncated => write!(f, "Truncated"),
            Self::Oversize { size, cap } => write!(f, "Oversize({size} > {cap})"),
            Self::Trailing => write!(f, "Trailing"),
            Self::Kind => write!(f, "Kind"),
            Self::Duplicate => write!(f, "Duplicate"),
        }
    }
}

impl core::error::Error for SnapError {}
