//! Provenance DAG, license spans, and an in-memory CAS for Klotho (K9).
//!
//! The digest is **blake3 of canonical little-endian bytes**. Widths (`Hash`,
//! `BlobId`) live in `klotho-core`; this crate owns the algorithm and the
//! graph that records how a blob came to exist.
//!
//! License metadata is **machine-auditable**. The graph does **not** prove
//! that a license is legally sufficient. `LicenseSpan::Unknown` is allowed
//! in-memory and **fails export**.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod artifact;
mod cas;
mod dag;
mod digest;
mod encode;
mod error;
mod evidence;
mod license;
mod rights;

pub use artifact::ArtifactKind;
pub use cas::{CATALOG_CAP, Cas, KCAS_VOLUME_CAP, MAX_BLOB_BYTES, MAX_BLOBS, PLACE_SHARD_CAP};
pub use dag::{Activity, Agent, ProvenanceDag, ProvenanceId, ProvenanceKind, ProvenanceNode};
pub use digest::{blob_id_of, hash_bytes};
pub use error::ProveError;
pub use evidence::{evidence_matches, evidence_signature};
pub use klotho_core::{BlobId, Hash};
pub use license::LicenseSpan;
pub use rights::{ReleaseRights, RightsError, RightsRoute};
