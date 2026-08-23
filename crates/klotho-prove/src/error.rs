//! Failures the provenance crate can report. None of these are `KernelFault`.

use core::fmt;

use crate::dag::ProvenanceId;
use klotho_core::BlobId;

/// Recoverable prove/CAS error. Cook maps these to cook-fail, not sim-reject.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ProveError {
    /// A node (or ancestor, after wash) carries [`crate::LicenseSpan::Unknown`].
    /// Export and `.warp` write must fail.
    UnknownLicense,
    /// SPDX id or commissioned holder was empty.
    InvalidLicense,
    /// Parent id is not in the DAG. Nodes are inserted bottom-up.
    MissingParent(ProvenanceId),
    /// Artifact node names a blob the CAS does not hold.
    MissingBlob(BlobId),
    /// Single blob exceeded [`crate::MAX_BLOB_BYTES`].
    BlobTooLarge {
        /// Actual size in bytes.
        size: usize,
    },
    /// Distinct blob count exceeded [`crate::MAX_BLOBS`].
    CasFull,
}

impl fmt::Display for ProveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownLicense => write!(f, "UnknownLicense"),
            Self::InvalidLicense => write!(f, "InvalidLicense"),
            Self::MissingParent(id) => write!(f, "MissingParent({id})"),
            Self::MissingBlob(id) => write!(f, "MissingBlob({id})"),
            Self::BlobTooLarge { size } => write!(f, "BlobTooLarge({size})"),
            Self::CasFull => write!(f, "CasFull"),
        }
    }
}

impl core::error::Error for ProveError {}
