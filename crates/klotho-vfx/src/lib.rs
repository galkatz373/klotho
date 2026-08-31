//! Trace-driven decals and one-shot meshes.
//!
//! Extract is a pure function of Trace + recipe table + pose lookup.
//! GPU particles are Era 3 and are not present here.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod extract;

pub use extract::{
    DEFAULT_TTL_TICKS, MAX_DECALS, MAX_ONESHOTS, RECIPE_BURST, RECIPE_IMPACT, RECIPE_SCORCH,
    extract_vfx,
};

pub use klotho_manifest::{Decal, MaterialRef, OneShotMesh, VisualManifest};
