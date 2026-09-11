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

mod error;
mod parse;
mod pin;
mod preview;

pub use error::AuthorError;
pub use parse::{load_file, parse_kdown, parse_ron};
pub use pin::{Pin, apply_pin};
pub use preview::{cook_summary, cook_validated, preview_summary};

pub use klotho_compile::{Cooked, cook_doc};
pub use klotho_ir::{IntentDoc, from_ron, to_ron, validate_doc};
