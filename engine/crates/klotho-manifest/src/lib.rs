//! Disposable presentation: [`VisualManifest`], [`SonicManifest`], [`UiManifest`],
//! [`LocManifest`].
//!
//! The renderer is a pure function of `VisualManifest` + [`Observer`] +
//! [`GpuBudget`]. No Sigils on the hot path except [`VisualManifest::debug_sigils`].
//! Manifest SoA lives in the crate-private [`tables`] module — gameplay
//! (`examples/hearth-slice`, `examples/ash-slice`, `klotho-author`,
//! ember/drift/chorus/netlock, `klotho-editor`) may not import
//! `klotho_manifest::tables` (`forbidden_gameplay_imports`).
//! Tables allowlist: render, audio, compile, vfx, cinematic.
//!
//! `#![forbid(unsafe_code)]`. Presenter / GPU upload is PR 12.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub(crate) mod tables;

mod a11y;
mod feel;
mod loc;
mod material;
mod observer;
mod sonic;
mod ui;
mod visual;

pub use a11y::{A11yEvidence, CaptionBand, FocusRole, ScreenReaderNode};
pub use feel::FeelManifest;
pub use loc::{ClosedCaptionCue, LocManifest, SubtitleCue, VoCue};
pub use material::MaterialTag;
pub use observer::{EYE_HEIGHT_MM, GpuBudget, Observer};
pub use sonic::{AdaptiveMusic, BedRef, GrainVoice, MusicCue, SonicManifest};
pub use ui::{UiManifest, Widget, WidgetKind};
pub use visual::{
    ClusterRef, Decal, GpuHandle, InstancePass, LightKind, LightStub, MaterialRef, OneShotMesh,
    PaletteSlot, ParticleEmitter, PostFlags, ProbeGrid, Ribbon, SkinnedInstance, VisualManifest,
};

pub use klotho_core::{AabbMm, BlobId, Epoch, IVec3, PoseMm, Sigil, Tick};
