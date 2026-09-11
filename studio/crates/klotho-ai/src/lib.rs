//! Isolated authoring transactions, semantic merge, leases, and audit.
//!
//! Model routing, agents, embeddings, Infer, and journeys are out of scope.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod audit;
mod cells;
mod conflict;
mod diff;
mod error;
mod exec;
mod ids;
mod lease;
mod merge;
mod ops;
mod store;
mod workspace;

pub use audit::{AuditKind, AuditRecord, ReviewQueueEntry};
pub use cells::{OpDecl, Precondition, declare};
pub use conflict::{
    ConflictReason, ConflictWitness, MatrixRule, MergeClass, classify_pair, matrix_rule,
};
pub use diff::{ImpactEdge, SemanticDiff};
pub use error::AiError;
pub use exec::{PreparedOp, apply_ops, check_prepared};
pub use ids::{
    AssetRequestId, Cell, ChangeId, FieldId, LeaseId, ReferenceId, TxId, derive_op_anchor,
};
pub use lease::Lease;
pub use merge::{diff_ops, merge_ops, merge_snapshots};
pub use ops::{
    AcceptanceContract, AuthorChangeSet, AuthorOp, AuthoringProvenance, ChangeScope, JourneySpec,
    OpKind, PatternArg, PatternInstance, TxBudget,
};
pub use store::{AuthoringTransaction, KlothoAi, TransactionStore, TxStatus};
pub use workspace::{AuthoringSnapshot, ContentWorkspace};

pub use klotho_author::{AnchoredSeedFact, SemanticEdit, apply_edit, bundle_content_hash};

#[cfg(test)]
mod tests;
