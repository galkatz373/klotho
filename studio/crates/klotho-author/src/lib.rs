//! Distaff: constraint cockpit. RON canonical + kdown sugar, same AST.
//!
//! Pin is cook-time: freeze a preview fact into Canon or seed Trace.
//! Nothing is real until Pin.
//!
//! Author-facing nouns: Locus, Canon, Intent, Trace, Rite, Law, Pin.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// HLD crate graph: Distaff → klotho-commit. Pin is cook-time and does not construct a kernel.
use klotho_commit as _;

mod apply;
mod error;
mod parse;
mod pin;
mod preview;
mod project;

pub use apply::{
    AnchoredSeedFact, ApplyOutcome, SemanticEdit, apply_edit, apply_edit_unlocked, apply_edits,
    bundle_content_hash, dependents_of, lookup_module, lookup_object, refresh_locks,
};
pub use error::AuthorError;
pub use parse::{load_doc_file, parse_kdown, parse_ron};
pub use pin::{Pin, apply_pin};
pub use preview::{cook_summary, cook_validated, preview_summary};
pub use project::{Loaded, flatten_bundle, load_any, load_file, migrate_to_dir, write_bundle};

pub use klotho_compile::{Cooked, cook_doc, cook_project};
pub use klotho_ir::{IntentDoc, IntentProject, from_ron, to_ron, validate_doc};
