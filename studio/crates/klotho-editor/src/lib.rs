//! Distaff viewport: Manifest presenter, outliner, Pin, cook dashboard, play-in-editor.
//!
//! Pin is the authoring act. The viewport is a view of Manifest. Gizmo poses
//! that are not Pinned are overlay only and do not survive recook.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod assistant;
mod dashboard;
mod error;
mod inspector;
mod kernel;
mod outliner;
mod session;
mod transaction;

pub use assistant::{
    AcceptanceEditor, Assumption, CaptureComparison, CostView, DistaffReview, PlanStep,
    ProvenanceView, RequestDraft, ReviewGroup, ReviewState, conservative_risk_inputs,
};
pub use dashboard::CookDashboard;
pub use error::EditorError;
pub use inspector::InspectorView;
pub use outliner::{LocusEntry, Outliner, PlaceGroup};
pub use session::EditorSession;
pub use transaction::EditorTransaction;

pub use klotho_author::{
    AuthorError, Cooked, IntentDoc, Pin, apply_pin, cook_validated, load_file,
};
pub use klotho_core::{Hash, LocusKind, Mm, PoseMm, Tick, YawMd};
pub use klotho_ir::{Name, Rel, SeedFact};
pub use klotho_render::{NullPresenter, Presenter, VisualManifest};
pub use klotho_ui::Pause;

#[cfg(test)]
mod kai08_tests;
#[cfg(test)]
mod kai09_tests;
#[cfg(test)]
mod tests;
