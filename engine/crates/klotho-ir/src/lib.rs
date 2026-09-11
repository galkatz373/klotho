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

mod agency;
mod analog;
mod anchor;
mod decl;
mod doc;
mod error;
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

pub use agency::{Agency, AssistLevel, Channel};
pub use analog::Analog;
pub use anchor::AnchorId;
pub use decl::{
    Affordance, Beat, BindSrc, CanonDiff, Cost, Law, LawBody, RiteGraph, RiteNode, RiteOp, Status,
};
pub use doc::IntentDoc;
pub use error::IrError;
pub use infer::{FactId, InferIntent, ModelId};
pub use klotho_core::{LocusKind, PlayerId, PoseMm, Sigil, SimLod, Tick};
pub use klotho_prove::ProvenanceId;
pub use mind::{MindIntent, MindSpec};
pub use name::Name;
pub use parse::{from_ron, to_ron};
pub use player::PlayerIntent;
pub use pred::{Cmp, Pred, SourceKind};
pub use project::{
    AnchorKind, Flattened, IntentModule, IntentModuleRef, IntentProject, LockEntry, ModuleImport,
    ModuleLock, NameAlias, ObjectAnchor, ParameterDecl, ParameterType, ParameterValue,
    ProjectBundle, SourceSpan, SpanKind, Tombstone, migrate_doc, module_content_hash,
};
pub use rel::Rel;
pub use seed::SeedFact;
pub use style::StyleIntent;
pub use target::{IntentTarget, Slot};
pub use validate::validate_doc;
pub use verb::Verb;
