//! Cooked Canon types and rite CFG checks.
//!
//! Authoring AST is [`klotho_ir`]. This crate does **not** evaluate predicates
//! (PR 04b). It does reject ill-formed rite CFGs (PR 04a).
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod ast;
mod cfg;
mod error;

pub use ast::{
    PRED_OPS_PER_EVAL, PRED_OPS_PER_TICK, PredChunk, PredOp, RELATED_SCAN_CAP,
    RITE_STEPS_PER_RITE_TICK, RITE_STEPS_PER_TICK, RiteChunk, RiteInstr,
};
pub use cfg::check_rite_cfg;
pub use error::CookError;
pub use klotho_ir::{
    Affordance, Beat, CanonDiff, IntentDoc, Law, LawBody, Pred, RiteGraph, RiteNode, RiteOp,
};
