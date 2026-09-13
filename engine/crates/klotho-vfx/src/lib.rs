//! Trace-driven decals, one-shot meshes, and GPU particle/ribbon presentation.
//!
//! Extract is a pure function of Trace + recipe table + pose lookup. GPU
//! output is never read back into Commit.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod extract;

pub use extract::{
    DEFAULT_TTL_TICKS, MAX_DECALS, MAX_ONESHOTS, MAX_PARTICLES, MAX_RIBBONS, RECIPE_BURST,
    RECIPE_IMPACT, RECIPE_PARTICLE, RECIPE_RIBBON, RECIPE_SCORCH, area_effect_golden, extract_vfx,
    present_particles,
};

pub use klotho_manifest::{
    Decal, MaterialRef, OneShotMesh, ParticleEmitter, Ribbon, VisualManifest,
};
