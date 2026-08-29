//! Stream/IO/header failures. Not `RejectReason` or `KernelFault`.

use core::fmt;

/// Why a catalog, shard, or volume could not be used.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum StreamError {
    /// Magic bytes were not the expected four-ASCII tag.
    Magic,
    /// Version byte is not the one this crate decodes.
    Version(u8),
    /// Catalog kind byte is not catalog.
    Kind,
    /// File or payload ended before a field.
    Truncated,
    /// Declared or on-disk size exceeded a cap. Not truncated: do not allocate `size`.
    Oversize {
        /// Observed or declared size.
        size: usize,
        /// Cap that was applied first.
        cap: usize,
    },
    /// Stored blob id did not match `blob_id_of` of the bytes.
    BlobIdMismatch,
    /// Shard header `place` did not match the catalog record.
    PlaceMismatch,
    /// Shard `canon_hash` did not match the catalog.
    CanonHashMismatch,
    /// Shard capture prefix did not match the catalog record.
    PrefixMismatch,
    /// Whole-file digest did not match the catalog record.
    HashMismatch,
    /// Catalog has no record for that Place.
    MissingPlace,
    /// Catalog has no record for that blob id.
    MissingBlob,
    /// Catalog names a shard that is not on disk.
    MissingShard,
    /// Row count exceeded `MAX_PLACE_ROWS`, or payload row count disagreed.
    RowCount,
    /// License was missing, Unknown, or malformed on a cooked volume.
    License,
    /// Filename or UTF-8 field was empty, oversize, or not a single path segment.
    Name,
    /// Extra bytes after a well-formed record list.
    Trailing,
    /// Catalog named the same place, volume, blob, or filename twice.
    Duplicate,
    /// Filesystem error.
    Io(String),
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Magic => write!(f, "Magic"),
            Self::Version(v) => write!(f, "Version({v})"),
            Self::Kind => write!(f, "Kind"),
            Self::Truncated => write!(f, "Truncated"),
            Self::Oversize { size, cap } => write!(f, "Oversize({size} > {cap})"),
            Self::BlobIdMismatch => write!(f, "BlobIdMismatch"),
            Self::PlaceMismatch => write!(f, "PlaceMismatch"),
            Self::CanonHashMismatch => write!(f, "CanonHashMismatch"),
            Self::PrefixMismatch => write!(f, "PrefixMismatch"),
            Self::HashMismatch => write!(f, "HashMismatch"),
            Self::MissingPlace => write!(f, "MissingPlace"),
            Self::MissingBlob => write!(f, "MissingBlob"),
            Self::MissingShard => write!(f, "MissingShard"),
            Self::RowCount => write!(f, "RowCount"),
            Self::License => write!(f, "License"),
            Self::Name => write!(f, "Name"),
            Self::Trailing => write!(f, "Trailing"),
            Self::Duplicate => write!(f, "Duplicate"),
            Self::Io(s) => write!(f, "Io({s})"),
        }
    }
}

impl core::error::Error for StreamError {}

impl From<std::io::Error> for StreamError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
