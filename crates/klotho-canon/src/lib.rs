//! Cooked Canon: pred compiler, eval, Law/Affordance/Beat tables, rite CFG.
//!
//! Authoring AST is [`klotho_ir`]. Cook emits [`PredProgram`] bytecode; [`eval_pred`]
//! interprets it under op caps. The rite VM is `klotho-commit`.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod ast;
mod cfg;
mod compile;
mod contradict;
mod cook;
mod error;
mod eval;
mod tables;

pub use ast::{
    Atom, CookedSlot, HEAT, IGNITE, OPAQUE, PRED_OPS_PER_EVAL, PRED_OPS_PER_TICK, PredChunk,
    PredId, PredOp, PredProgram, RELATED_SCAN_CAP, RITE_STEPS_PER_RITE_TICK, RITE_STEPS_PER_TICK,
    RelatedScan, RiteChunk, RiteId, RiteInstr,
};
pub use cfg::check_rite_cfg;
pub use compile::compile_pred;
pub use cook::{cook, cook_diffs};
pub use error::CookError;
pub use eval::{EvalCtx, MemStore, PredStore, eval_pred};
pub use klotho_ir::{
    Affordance, Beat, CanonDiff, IntentDoc, Law, LawBody, Pred, RiteGraph, RiteNode, RiteOp,
};
pub use tables::{
    Canon, CookedAffordance, CookedBeat, CookedCost, CookedLaw, CookedLawBody, CookedRite,
};
