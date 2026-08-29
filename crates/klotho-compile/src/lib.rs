//! Cook: IntentDoc + closed kitbash → Canon + CAS.
//!
//! v1 Weaver is **retrieval**. Missing tags are cook errors, not synthesis
//! (K6 / Q4). Every blob carries a [`LicenseSpan`]. Quantized little-endian
//! verts; cook hash is stable across OS (no host floats in hashed bytes).
//!
//! Depends on ir, prove, canon, manifest, stream. Packs a cooked `.warp`
//! (not `KLTH` CAS blobs) as little-endian sections, or a version-2 catalog
//! plus KCAS volumes and Place shards.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod catalog;
mod cook;
mod encode;
mod error;
mod header;
mod kit;
mod warp;

pub use catalog::{CATALOG_FILE, CatalogManifest, PlaceCatalogEntry, write_catalog};
pub use cook::{Binding, COMPILER_VERSION, Cooked, blob_of, cook_doc, cook_with, digest_of};
pub use error::CompileError;
pub use header::{
    ClipSetInfo, DecodedClip, DecodedGrain, DecodedMesh, GRAIN_HZ, GrainInfo, MAGIC,
    MAX_CLIP_SAMPLES, MAX_CLIPS, MAX_RITE_STEPS, MAX_TRIS, MeshInfo, PREFIX, VERSION,
    decode_clipset, decode_grain, decode_mesh, validate_blob, validate_clipset, validate_grain,
    validate_hull, validate_mesh, validate_rite,
};
pub use kit::{KitEntry, Kitbash};
pub use klotho_manifest::MaterialTag;
pub use klotho_prove::{Cas, LicenseSpan, MAX_BLOB_BYTES, MAX_BLOBS, ProvenanceDag};
pub use warp::{
    WARP_CAP_DESKTOP, WARP_CAP_MOBILE, WARP_MAGIC, WARP_MAX_LOCI, WARP_VERSION, pack_warp,
    unpack_warp, write_warp,
};
