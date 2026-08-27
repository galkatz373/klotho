//! Disposable presentation: [`VisualManifest`], [`SonicManifest`], [`UiManifest`].
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

mod material;
mod observer;
mod sonic;
mod ui;
mod visual;

pub use material::MaterialTag;
pub use observer::{EYE_HEIGHT_MM, GpuBudget, Observer};
pub use sonic::{BedRef, GrainVoice, SonicManifest};
pub use ui::{UiManifest, Widget, WidgetKind};
pub use visual::{
    ClusterRef, GpuHandle, InstancePass, LightKind, LightStub, MaterialRef, PaletteSlot, PostFlags,
    ProbeGrid, SkinnedInstance, VisualManifest,
};

pub use klotho_core::{AabbMm, BlobId, Epoch, IVec3, PoseMm, Sigil, Tick};
