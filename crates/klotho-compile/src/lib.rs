//! Cook: IntentDoc + closed kitbash → Canon + CAS.
//!
//! v1 Weaver is **retrieval**. Missing tags are cook errors, not synthesis
//! (K6 / Q4). Every blob carries a [`LicenseSpan`]. Quantized little-endian
//! verts; cook hash is stable across OS (no host floats in hashed bytes).
//!
//! Depends on ir, prove, canon, manifest. `.warp` packing is PR 19.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cook;
mod encode;
mod error;
mod header;
mod kit;

pub use cook::{Binding, COMPILER_VERSION, Cooked, blob_of, cook_doc, cook_with, digest_of};
pub use error::CompileError;
pub use header::{
    DecodedMesh, GRAIN_HZ, GrainInfo, MAGIC, MAX_TRIS, MeshInfo, PREFIX, VERSION, decode_mesh,
    validate_grain, validate_hull, validate_mesh, validate_rite,
};
pub use kit::{KitEntry, Kitbash};
pub use klotho_manifest::MaterialTag;
pub use klotho_prove::{Cas, LicenseSpan, ProvenanceDag};
