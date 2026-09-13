//! Built-in Klotho authoring intelligence (KAI-07).
//!
//! Model output is untrusted. It can reach a project only through the closed
//! [`ToolCall`] protocol and isolated [`TransactionStore`]. Evidence remains a
//! sealed, read-only result produced by `klotho-eval` trusted tools.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod agent;
mod assets;
mod audit;
mod cells;
mod conflict;
mod context;
mod diff;
mod error;
mod evaluation;
mod exec;
mod ids;
mod index;
mod lease;
mod memory;
mod merge;
mod model;
mod ops;
mod policy;
mod review;
mod service;
mod store;
mod tools;
mod workspace;

pub use agent::{
    AgentRole, AgentScheduler, AiProgress, CreativeRequest, RequestBudget, RequestId,
    RequestRecord, RequestState, Usage,
};
pub use assets::AssetRequestStore;
pub use audit::{AuditKind, AuditRecord, ReviewQueueEntry};
pub use cells::{OpDecl, Precondition, declare};
pub use conflict::{
    ConflictReason, ConflictWitness, MatrixRule, MergeClass, classify_pair, matrix_rule,
};
pub use context::{
    CompiledContext, ContextBuilder, ContextOmission, ContextRequest, SchemaContext,
};
pub use diff::{ImpactEdge, SemanticDiff};
pub use error::AiError;
pub use evaluation::{EvaluationBroker, EvidenceRecord};
pub use exec::{PreparedOp, apply_ops, check_prepared};
pub use ids::{
    AssetRequestId, Cell, ChangeId, FieldId, LeaseId, ReferenceId, TxId, derive_op_anchor,
};
pub use index::{
    DistanceMetric, EmbeddingIndex, EmbeddingKey, EmbeddingRow, IndexDelta, SemanticEntry,
    SemanticEntryKind, SemanticProjectIndex,
};
pub use lease::Lease;
pub use memory::{ApprovedMemory, MemoryApproval, ProjectMemory};
pub use merge::{diff_ops, merge_ops, merge_snapshots};
pub use model::{
    BackendId, BackendKind, BackendRecord, BackendRequest, BackendResponse, BackendSpec,
    BackendStatus, BackendTransport, DeterministicFakeBackend, ModelBackend, ModelCapability,
    ModelRouter, ProtocolAdapter,
};
pub use ops::{
    AcceptanceContract, AuthorChangeSet, AuthorOp, AuthoringProvenance, ChangeScope, JourneySpec,
    OpKind, PatternArg, PatternInstance, TxBudget,
};
pub use policy::{
    Capability, ContextClass, DisclosurePolicy, ExecutionPolicy, SecretStore, ToolProfile,
};
pub use review::{
    ArtifactClass, BatchItem, FrozenBatch, OwnerBudget, OwnerQueues, R0Rule, RiskInput, RiskLevel,
    RiskPolicy, SampleDisposition, SampleRecord, SamplingPolicy,
};
pub use service::{KlothoAi, RepairOutcome, ReviewPackage};
pub use store::{AuthoringTransaction, TransactionStore, TxStatus};
pub use tools::{ToolCall, ToolRegistry, ToolResult};
pub use workspace::{AuthoringSnapshot, ContentWorkspace};

pub use klotho_author::{AnchoredSeedFact, SemanticEdit, apply_edit, bundle_content_hash};

#[cfg(test)]
mod kai07_tests;
#[cfg(test)]
mod kai08_tests;
#[cfg(test)]
mod tests;
