//! Distaff viewport: Manifest presenter, outliner, Pin, cook dashboard, play-in-editor.
//!
//! Pin is the authoring act. The viewport is a view of Manifest. Gizmo poses
//! that are not Pinned are overlay only and do not survive recook.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod a11y;
mod assistant;
mod dashboard;
mod error;
mod feel;
mod inspector;
mod kernel;
mod narrative;
mod outliner;
mod present;
mod quality;
mod session;
mod transaction;
mod world;

pub use a11y::{A11yReview, review_a11y, review_first_title};
pub use assistant::{
    AcceptanceEditor, Assumption, CaptureComparison, CostView, DistaffReview, PlanStep,
    ProvenanceView, RequestDraft, ReviewGroup, ReviewState, conservative_risk_inputs,
};
pub use dashboard::CookDashboard;
pub use error::EditorError;
pub use feel::{FeelAbSession, FeelCandidate, FeelSweep, evidence_copy};
pub use inspector::InspectorView;
pub use narrative::{LineRow, QuestRow, WriterRoomView, review_narrative};
pub use outliner::{LocusEntry, Outliner, PlaceGroup};
pub use present::{PresentReview, review_presentation};
pub use quality::{QualityReview, review_quality};
pub use session::EditorSession;
pub use transaction::EditorTransaction;
pub use world::{GraphEdge, PlaceNode, WorldGraphView, review_world, world_graph};

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
mod kai10_tests;
#[cfg(test)]
mod kai11_tests;
#[cfg(test)]
mod kai14_tests;
#[cfg(test)]
mod kai15_tests;
#[cfg(test)]
mod kai16_tests;
#[cfg(test)]
mod kai17_tests;
#[cfg(test)]
mod kai18_tests;
#[cfg(test)]
mod kai20_tests;
#[cfg(test)]
mod kai21_tests;
#[cfg(test)]
mod kai22_tests;
#[cfg(test)]
mod tests;
