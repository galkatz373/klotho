//! Intent IR: the author-facing AST and the runtime intent packets.
//!
//! v1 canonical syntax is **RON**. kdown sugar (same AST) is owned by Distaff
//! (`klotho-author`). There is no natural-language compiler in v1 (Q3).
//!
//! [`IntentProject`] is the modular authoring form. Flattening is a pure
//! function of locked module bytes and yields a current [`IntentDoc`].
//!
//! [`from_ron`] is the canonical parser. Distaff desugars `*.kdown` into this AST.
//!
//! This crate does **not** execute Laws or Rites. Cook/eval lives in
//! `klotho-canon`. The types here are what Appendix A / B will parse as.
//!
//! `PlayerIntent` is the only packet with [`Agency`]. Mind and Infer cannot
//! impersonate a player at the type level (K10).
//!
//! Depends on `klotho-prove` for [`ProvenanceId`] (PR 03 depends on PR 02).
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod a11y;
mod agency;
mod analog;
mod anchor;
mod decl;
mod diag;
mod doc;
mod error;
mod feel;
mod infer;
mod mind;
mod name;
mod parse;
mod player;
mod pred;
mod project;
mod rel;
mod seed;
mod style;
mod target;
mod validate;
mod verb;

pub use a11y::{A11yProfile, CaptionMode, ContrastMode};
pub use agency::{Agency, AssistLevel, Channel};
pub use analog::Analog;
pub use anchor::AnchorId;
pub use decl::{
    Affordance, Beat, BindSrc, CanonDiff, Cost, Law, LawBody, RiteGraph, RiteNode, RiteOp, Status,
};
pub use diag::{
    Counterexample, Diagnostic, DiagnosticCatalogEntry, DiagnosticCode, EstimatedCost,
    FailureClass, RepairShape, Severity, as_data, blame_anchor, diagnose_agency, diagnose_budget,
    diagnose_cap, diagnose_cfg, diagnose_contradiction, diagnose_hash_drift, diagnose_journey,
    diagnose_named, diagnose_package, diagnose_provenance, diagnostic_catalog, prove_to_diagnostic,
};
pub use doc::IntentDoc;
pub use error::IrError;
pub use feel::{
    AimAssistContract, CameraResponse, CurveKnot, FeelAccessibility, FeelContract,
    ImpactPresentation, QuantizedCurve, TickWindow,
};
pub use infer::{FactId, InferIntent, ModelId};
pub use klotho_core::{LocusKind, PlayerId, PoseMm, Sigil, SimLod, Tick};
pub use klotho_prove::ProvenanceId;
pub use mind::{
    FarRule, MAX_FAR_INPUTS, MAX_MIND_FACTS, MAX_MIND_GOALS, MAX_MIND_OPERATORS, MindFact,
    MindGoal, MindIntent, MindOperator, MindProgram, MindQuery, MindRef, MindSpec, MindTarget,
};
pub use name::Name;
pub use parse::{from_ron, to_ron};
pub use player::PlayerIntent;
pub use pred::{Cmp, Pred, SourceKind};
pub use project::{
    AnchorKind, Flattened, IntentModule, IntentModuleRef, IntentProject, LockEntry, ModuleImport,
    ModuleLock, NameAlias, ObjectAnchor, ParameterDecl, ParameterType, ParameterValue, PatternArg,
    PatternInstance, ProjectBundle, SourceSpan, SpanKind, Tombstone, migrate_doc,
    module_content_hash,
};
pub use rel::Rel;
pub use seed::SeedFact;
pub use style::StyleIntent;
pub use target::{IntentTarget, Slot};
pub use validate::validate_doc;
pub use verb::Verb;
