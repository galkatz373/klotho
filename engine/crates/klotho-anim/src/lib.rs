//! Presentation clip sampling and palette helpers.
//!
//! Clip root samples are the hashed locomotion channel. Joint palettes are
//! not hashed. Clip time is [`klotho_core::Tick`] (no hidden integrator).
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod clip;
mod face;
mod ik;
mod motion;
mod palette;

pub use clip::{
    ArtifactAuthority, Clip, ClipSet, EvidenceLane, WALK_MM_PER_TICK, classify_clip,
    classify_clip_change,
};
pub use face::{FaceTrack, Viseme};
pub use ik::look_at_yaw;
pub use motion::{
    Bone, MotionDb, MotionEntry, RetargetProfile, Skeleton, classify_geometry_change,
    motiondb_preserves_clipset,
};
pub use palette::{
    MAX_SKIN_BONES, WEIGHT_SUM, apply_pose, capped_joints, clip_grounded, clip_verb, compose,
    sample_joints, skin_vertex, skin_world,
};
