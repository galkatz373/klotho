//! Cook-time failures. None of these are `KernelFault` or `RejectReason`.

use core::fmt;

use klotho_canon::CookError;
use klotho_prove::ProveError;

/// Why an IntentDoc failed to compile against the kitbash.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CompileError {
    /// Style or bind named a tag the closed library does not have.
    MissingTag(String),
    /// On-disk kitbash bytes do not match the lockfile (supply-chain).
    LockMismatch {
        /// File name under `data/kitbash`.
        file: String,
        /// Lockfile digest.
        expected: String,
        /// blake3 of the bytes on disk.
        actual: String,
    },
    /// Lockfile names a file that is not on disk.
    MissingLockFile(String),
    /// Kitbash catalog failed to parse.
    Catalog(String),
    /// `klotho-canon` cook failed.
    Canon(String),
    /// CAS / provenance failed.
    Prove(String),
    /// Clustered-mesh / grain / hull header is invalid.
    Header(String),
    /// A millimetre extent does not fit in quantized `i16` verts.
    QuantizeOverflow,
    /// Filesystem read failed.
    Io(String),
    /// `.warp` container is truncated, oversize, or has a bad magic/version.
    Warp(String),
}

impl CompileError {
    pub(crate) fn canon(e: CookError) -> Self {
        Self::Canon(e.to_string())
    }

    pub(crate) fn prove(e: ProveError) -> Self {
        Self::Prove(e.to_string())
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTag(t) => write!(f, "MissingTag({t})"),
            Self::LockMismatch {
                file,
                expected,
                actual,
            } => write!(f, "LockMismatch({file}: expected {expected}, got {actual})"),
            Self::MissingLockFile(p) => write!(f, "MissingLockFile({p})"),
            Self::Catalog(s) => write!(f, "Catalog({s})"),
            Self::Canon(s) => write!(f, "Canon({s})"),
            Self::Prove(s) => write!(f, "Prove({s})"),
            Self::Header(s) => write!(f, "Header({s})"),
            Self::QuantizeOverflow => write!(f, "QuantizeOverflow"),
            Self::Io(s) => write!(f, "Io({s})"),
            Self::Warp(s) => write!(f, "Warp({s})"),
        }
    }
}

impl core::error::Error for CompileError {}
