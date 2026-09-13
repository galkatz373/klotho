//! Attention IR, production HUD skin, and accessible declarative UI (KAI-16).
//!
//! Extract is a pure function of [`WorldSnapshot::view()`], the observer
//! [`Sigil`], and cooked [`Canon`]. Fact names reach a widget only through
//! `Knows`. Pause stops `step` locally and does not enqueue [`PlayerIntent`].
//! Pause-menu save copies the published snapshot with an empty suffix.
//!
//! Menus are declarative constraint layouts with focus semantics, remapping,
//! captions, and a locale × aspect × input × accessibility capture matrix.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod a11y;
mod capture;
mod error;
mod extract;
mod focus;
mod layout;
mod menu;
mod pause;
mod skin;

pub use a11y::{apply_profile, caption_band, check_compliance, compliance_evidence, prove_menu};
pub use capture::{
    CaptureCell, CaptureReport, capture_profiles, matrix_gate, run_capture_matrix,
    seeded_focus_fault, seeded_overflow_fault,
};
pub use error::UiError;
pub use extract::extract_ui;
pub use focus::{FocusCycle, FocusNav, nav_from_button, screen_reader_tree};
pub use layout::{
    CAPTURE_ASPECTS, CAPTURE_LOCALES, LaidOut, LayoutFrame, UiKind, UiNode, aspect_viewport, layout,
};
pub use menu::{pause_menu, remap_menu, settings_menu};
pub use pause::{LoadError, Pause, SaveQuad, Session, check_load, load, save_from_snapshot};
pub use skin::{
    Color, HudElement, HudFrame, HudPalette, HudSkin, HudSlot, HudViewport, Rect, SafeArea,
    skin_hud,
};

pub use klotho_canon::Canon;
pub use klotho_core::Sigil;
pub use klotho_ir::PlayerIntent;
pub use klotho_manifest::{
    A11yEvidence, CaptionBand, FocusRole, ScreenReaderNode, UiManifest, Widget, WidgetKind,
};
pub use klotho_world::WorldSnapshot;
