//! Attention IR from snapshot + observer mind + Canon.
//!
//! Extract is a pure function of [`WorldSnapshot::view()`], the observer
//! [`Sigil`], and cooked [`Canon`]. Fact names reach a widget only through
//! `Knows`. Pause stops `step` locally and does not enqueue [`PlayerIntent`].
//! Pause-menu save copies the published snapshot with an empty suffix.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod extract;
mod pause;

pub use extract::extract_ui;
pub use pause::{LoadError, Pause, SaveQuad, Session, check_load, load, save_from_snapshot};

pub use klotho_canon::Canon;
pub use klotho_core::Sigil;
pub use klotho_ir::PlayerIntent;
pub use klotho_manifest::{UiManifest, Widget, WidgetKind};
pub use klotho_world::WorldSnapshot;
