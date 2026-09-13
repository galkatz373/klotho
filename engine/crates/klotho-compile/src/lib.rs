//! Cook: IntentDoc or flattened IntentProject + closed kitbash → Canon + CAS.
//!
//! v1 Weaver is **retrieval**. Missing tags are cook errors, not synthesis
//! (K6 / Q4). Every blob carries a [`LicenseSpan`]. Quantized little-endian
//! verts; cook hash is stable across OS (no host floats in hashed bytes).
//! DCC ingest is [`cook_with_dcc`] plus [`encode_mesh_i16`] / [`encode_hull`] /
//! [`encode_clipset`]; this crate does not parse glTF.
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
mod epoch;
mod error;
mod header;
mod kit;
mod package;
mod placement;
mod warp;

#[cfg(test)]
mod kai04_corpus;

pub use catalog::{
    CATALOG_FILE, CatalogManifest, IncrementalCatalog, LicenseCoverage, PlaceCatalogEntry,
    license_coverage, write_catalog, write_catalog_incremental,
};
pub use cook::{
    Binding, COMPILER_VERSION, Cooked, DccArtifact, blob_of, cook_doc, cook_project, cook_with,
    cook_with_dcc, digest_of,
};
pub use encode::{encode_clipset, encode_hull, encode_mesh_i16};
pub use epoch::{
    COMPILER_EPOCH_PACK_VERSION, CanonEpochPack, cook_epoch_pack, cook_next_epoch_pack,
};
pub use error::{CompileError, check_ship_allowlist};
pub use header::{
    ClipSetInfo, DecodedClip, DecodedGrain, DecodedMesh, DecodedSkinnedMesh, GRAIN_HZ, GrainInfo,
    MAGIC, MAX_CLIP_SAMPLES, MAX_CLIPS, MAX_RITE_STEPS, MAX_SKIN_BONES, MAX_SKIN_VERTS, MAX_TRIS,
    MeshInfo, PREFIX, SKIN_WEIGHT_SUM, SkinnedMeshInfo, VERSION, decode_clipset, decode_grain,
    decode_mesh, decode_skinned_mesh, encode_skinned_mesh, peek_kind, validate_blob,
    validate_clipset, validate_grain, validate_hull, validate_mesh, validate_rite,
    validate_skinned_mesh,
};
pub use kit::{KitEntry, Kitbash};
pub use klotho_manifest::MaterialTag;
pub use klotho_prove::{Cas, LicenseSpan, MAX_BLOB_BYTES, MAX_BLOBS, ProvenanceDag};
pub use package::{
    AccessibilitySettings, CreditEntry, CreditsRoll, DESKTOP_SKUS, DesktopPackage, DesktopSku,
    HudSpec, InstallRecord, LocaleTable, REQUIRED_LOCALE_KEYS, ShipContent, install_package,
    pack_desktop, repair_package, uninstall_package,
};
pub use placement::{
    ChunkKey, InstanceGroup, MAX_INSTANCE_GROUPS, MAX_PLACEMENT_RECORDS, MaterializedWorld,
    PLACEMENT_MAGIC, PLACEMENT_VERSION, PlacementChunk, PlacementRecord, PlacementWrite,
    RawPlacement, decode_chunk, diff_chunks, materialize, place_sigil, unique_blob_count,
    write_placement_chunks,
};
pub use warp::{
    WARP_CAP_DESKTOP, WARP_CAP_MOBILE, WARP_MAGIC, WARP_MAX_LOCI, WARP_VERSION, pack_warp,
    unpack_warp, write_warp,
};
