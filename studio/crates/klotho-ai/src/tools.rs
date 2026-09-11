//! Closed typed tool registry. No string command or arbitrary path exists.

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_eval::EvidenceBundle;
use klotho_ir::{Diagnostic, Name};
use klotho_pattern::PatternCatalogRow;
use klotho_schema::SchemaCatalog;

use crate::context::{CompiledContext, ContextBuilder, ContextRequest};
use crate::diff::SemanticDiff;
use crate::error::AiError;
use crate::evaluation::EvaluationBroker;
use crate::ids::{ChangeId, TxId};
use crate::index::SemanticProjectIndex;
use crate::memory::ProjectMemory;
use crate::ops::{AuthorOp, ChangeScope, TxBudget};
use crate::policy::{Capability, ToolProfile};
use crate::store::TransactionStore;

/// Every agent-callable action. Deserialization rejects unknown fields and
/// variants, so hostile output cannot smuggle a shell/path/release operation.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolCall {
    /// Compile bounded semantic context.
    ProjectDescribe(ContextRequest),
    /// Return the generated catalog.
    SchemaQuery,
    /// Create an isolated transaction.
    ChangeCreate {
        /// Exact base project hash.
        base: Hash,
        /// Declared scope.
        scope: ChangeScope,
        /// Operation cap.
        budget: TxBudget,
    },
    /// Apply semantic operations.
    ChangeApply {
        /// Transaction.
        transaction: TxId,
        /// Typed operations.
        operations: Vec<AuthorOp>,
    },
    /// Semantic diff.
    ChangeDiff {
        /// Transaction.
        transaction: TxId,
    },
    /// Search by exact capability/name token.
    PatternSearch {
        /// Case-sensitive token matched against id/requires/grants.
        capability: String,
    },
    /// Validate the isolated project and return structured diagnostics.
    ValidateRun {
        /// Transaction.
        transaction: TxId,
    },
    /// Read sealed trusted evidence.
    EvidenceRead {
        /// Evidence content hash.
        hash: Hash,
    },
    /// Enter the human review queue. This does not Pin or merge.
    ChangeSubmit {
        /// Transaction.
        transaction: TxId,
    },
}

impl ToolCall {
    /// Required capability.
    #[must_use]
    pub const fn capability(&self) -> Capability {
        match self {
            Self::ProjectDescribe(_) => Capability::ProjectDescribe,
            Self::SchemaQuery => Capability::SchemaQuery,
            Self::ChangeCreate { .. } => Capability::ChangeCreate,
            Self::ChangeApply { .. } => Capability::ChangeApply,
            Self::ChangeDiff { .. } => Capability::ChangeDiff,
            Self::PatternSearch { .. } => Capability::PatternSearch,
            Self::ValidateRun { .. } => Capability::ValidateRun,
            Self::EvidenceRead { .. } => Capability::EvidenceRead,
            Self::ChangeSubmit { .. } => Capability::ChangeSubmit,
        }
    }
}

/// Typed tool result.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ToolResult {
    /// Semantic context.
    Project(CompiledContext),
    /// Generated schema.
    Schema(SchemaCatalog),
    /// New transaction/change.
    Created {
        /// Isolated transaction.
        transaction: TxId,
        /// Stable change identity.
        change: ChangeId,
    },
    /// New isolated snapshot hash.
    Applied(Hash),
    /// Semantic diff.
    Diff(SemanticDiff),
    /// Compatible patterns.
    Patterns(Vec<PatternCatalogRow>),
    /// Structured diagnostics; empty means valid.
    Diagnostics(Vec<Diagnostic>),
    /// Trusted sealed evidence.
    Evidence(EvidenceBundle),
    /// Candidate entered the human review queue.
    Submitted(ChangeId),
}

/// Stateless registry for the closed protocol.
pub struct ToolRegistry;

/// Borrowed service surfaces used while executing one call.
pub struct ToolEnvironment<'a> {
    /// Transactions.
    pub transactions: &'a mut TransactionStore,
    /// Structural index.
    pub project: &'a SemanticProjectIndex,
    /// Generated catalog.
    pub catalog: &'a SchemaCatalog,
    /// Approved memory.
    pub memory: &'a ProjectMemory,
    /// Trusted evidence broker.
    pub evaluation: &'a EvaluationBroker,
}

impl ToolRegistry {
    /// Decode a model-produced call with a byte cap and deny-unknown schema.
    pub fn decode(bytes: &[u8], max_bytes: u64) -> Result<ToolCall, AiError> {
        if bytes.len() as u64 > max_bytes {
            return Err(AiError::BackendPayload);
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|error| AiError::BackendProtocol(error.to_string()))?;
        klotho_ir::from_ron(text).map_err(|error| AiError::BackendProtocol(error.to_string()))
    }

    /// Authorize and execute one typed call.
    pub fn execute(
        profile: &ToolProfile,
        call: ToolCall,
        env: ToolEnvironment<'_>,
    ) -> Result<ToolResult, AiError> {
        profile.require(call.capability())?;
        match call {
            ToolCall::ProjectDescribe(request) => Ok(ToolResult::Project(ContextBuilder::compile(
                env.project,
                env.catalog,
                env.memory,
                &request,
            ))),
            ToolCall::SchemaQuery => Ok(ToolResult::Schema(env.catalog.clone())),
            ToolCall::ChangeCreate {
                base,
                scope,
                budget,
            } => {
                if base != env.project.project_hash {
                    return Err(AiError::BaseHashMismatch);
                }
                let transaction = env.transactions.create(scope, budget)?;
                let change = env.transactions.transaction(transaction)?.change;
                Ok(ToolResult::Created {
                    transaction,
                    change,
                })
            }
            ToolCall::ChangeApply {
                transaction,
                operations,
            } => env
                .transactions
                .apply(transaction, operations)
                .map(ToolResult::Applied),
            ToolCall::ChangeDiff { transaction } => {
                env.transactions.diff(transaction).map(ToolResult::Diff)
            }
            ToolCall::PatternSearch { capability } => {
                let mut rows: Vec<_> = klotho_pattern::catalog_rows()
                    .into_iter()
                    .filter(|row| {
                        row.id.contains(&capability)
                            || row.requires.iter().any(|value| value.contains(&capability))
                            || row.grants.iter().any(|value| value.contains(&capability))
                    })
                    .collect();
                rows.sort_by(|a, b| a.id.cmp(&b.id).then(a.version.cmp(&b.version)));
                Ok(ToolResult::Patterns(rows))
            }
            ToolCall::ValidateRun { transaction } => {
                let snapshot = env.transactions.snapshot(transaction)?;
                let diagnostics = match klotho_author::flatten_bundle(&snapshot.bundle()) {
                    Ok(_) => Vec::new(),
                    Err(error) => vec![author_diagnostic(&error)],
                };
                Ok(ToolResult::Diagnostics(diagnostics))
            }
            ToolCall::EvidenceRead { hash } => env
                .evaluation
                .get(hash)
                .map(|record| ToolResult::Evidence(record.bundle)),
            ToolCall::ChangeSubmit { transaction } => env
                .transactions
                .submit(transaction)
                .map(|entry| ToolResult::Submitted(entry.change)),
        }
    }

    /// Tool names for context/schema discovery.
    #[must_use]
    pub fn names() -> Vec<Name> {
        [
            "project.describe",
            "schema.query",
            "change.create",
            "change.apply",
            "change.diff",
            "pattern.search",
            "validate.run",
            "evidence.read",
            "change.submit",
        ]
        .into_iter()
        .map(Name::from)
        .collect()
    }
}

fn author_diagnostic(error: &klotho_author::AuthorError) -> Diagnostic {
    match error {
        klotho_author::AuthorError::Ir(error) => Diagnostic::from(error),
        klotho_author::AuthorError::Pattern(error) => {
            error.to_diagnostic(klotho_ir::AnchorId::ZERO)
        }
        other => klotho_ir::diagnose_named(
            "IR.Parse",
            klotho_ir::FailureClass::Schema,
            "authoring",
            "project",
            other.to_string(),
        ),
    }
}
