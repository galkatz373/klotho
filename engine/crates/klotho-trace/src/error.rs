//! Trace encode / log failures. None of these are `KernelFault`.

use core::fmt;

/// Why a Trace value could not be built, encoded, or decoded.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum TraceError {
    /// `IslandSnap` parallel arrays have unequal lengths.
    SnapLen,
    /// Event bytes are truncated or the tag/version is unknown.
    BadEvent,
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapLen => write!(f, "SnapLen"),
            Self::BadEvent => write!(f, "BadEvent"),
        }
    }
}

impl core::error::Error for TraceError {}
