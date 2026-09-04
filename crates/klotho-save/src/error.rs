//! Save I/O failures. Not `RejectReason` or `KernelFault`.

use core::fmt;

use klotho_world::SnapError;

/// Why a save blob could not be encoded, decoded, or loaded.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum SaveError {
    /// Magic bytes were not `KSAV`.
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
    /// `trace_prefix_hash` does not match the expected prefix.
    PrefixMismatch,
    /// `canon_hash` does not match the expected cook digest.
    CanonMismatch,
    /// Extra bytes after a well-formed record.
    Trailing,
    /// A suffix event is at or before the snapshot tick.
    TickWindow,
    /// A length-prefixed Trace event failed to decode.
    BadEvent,
    /// Kind, relation, LOD, or channel tag is not in the frozen set.
    Kind,
    /// Duplicate sigil or more than one `Rel::In` on a snap row.
    Duplicate,
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Magic => write!(f, "Magic"),
            Self::Version(v) => write!(f, "Version({v})"),
            Self::Pad => write!(f, "Pad"),
            Self::Truncated => write!(f, "Truncated"),
            Self::Oversize { size, cap } => write!(f, "Oversize({size} > {cap})"),
            Self::PrefixMismatch => write!(f, "PrefixMismatch"),
            Self::CanonMismatch => write!(f, "CanonMismatch"),
            Self::Trailing => write!(f, "Trailing"),
            Self::TickWindow => write!(f, "TickWindow"),
            Self::BadEvent => write!(f, "BadEvent"),
            Self::Kind => write!(f, "Kind"),
            Self::Duplicate => write!(f, "Duplicate"),
        }
    }
}

impl core::error::Error for SaveError {}

impl From<SnapError> for SaveError {
    fn from(e: SnapError) -> Self {
        match e {
            SnapError::Magic => Self::Magic,
            SnapError::Version(v) => Self::Version(v),
            SnapError::Pad => Self::Pad,
            SnapError::Truncated => Self::Truncated,
            SnapError::Oversize { size, cap } => Self::Oversize { size, cap },
            SnapError::Trailing => Self::Trailing,
            SnapError::Kind => Self::Kind,
            SnapError::Duplicate => Self::Duplicate,
        }
    }
}
