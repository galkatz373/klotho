//! Beat-driven cinematic Observer tracks.
//!
//! A track samples the one global simulation [`Tick`]. It neither owns a
//! clock nor writes Projection: its [`CinematicManifest`] is disposable
//! presentation consumed by render, UI, and host policy.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod camera;
mod face;
mod track;

pub use camera::CameraFeel;
pub use face::{FaceSample, sample_face};
pub use track::{CinematicManifest, Keyframe, ObserverTrack, PlayerPhys, TrackError};

pub use klotho_ir::{Beat, LocusKind, Name};
pub use klotho_manifest::{Observer, PoseMm, Sigil, Tick};
