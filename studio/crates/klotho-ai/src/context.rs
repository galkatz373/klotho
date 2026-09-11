//! Small, hash-bound context compilation with explicit omissions.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::AnchorId;
use klotho_schema::{
    BudgetSchema, DiagnosticSchema, OperationSchema, PatternSchema, SchemaCatalog,
};

use crate::index::{SemanticEntry, SemanticProjectIndex};
use crate::memory::{ApprovedMemory, ProjectMemory};

/// Bounded structural context request.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRequest {
    /// Included modules or objects. Empty means project-wide.
    pub anchors: BTreeSet<AnchorId>,
    /// Maximum structural rows.
    pub max_entries: u32,
    /// Include approved project memory.
    pub include_memory: bool,
}

/// A context item omitted because of scope or cap.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOmission {
    /// Omitted row identity.
    pub anchor: AnchorId,
    /// Stable reason.
    pub reason: String,
}

/// Exact context passed to a model backend.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledContext {
    /// Exact project hash.
    pub project_hash: Hash,
    /// Generated schema version.
    pub schema_version: u32,
    /// Generated catalog hash.
    pub catalog_hash: Hash,
    /// Bounded generated action surface needed for planning.
    pub schema: SchemaContext,
    /// Selected authoritative structural rows.
    pub entries: Vec<SemanticEntry>,
    /// Approved source-linked memory only.
    pub memory: Vec<ApprovedMemory>,
    /// Explicitly omitted rows.
    pub omissions: Vec<ContextOmission>,
}

/// Generated schema subset embedded in model context. Canon validation remains authoritative.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaContext {
    /// Semantic operation forms.
    pub operations: Vec<OperationSchema>,
    /// Registered patterns.
    pub patterns: Vec<PatternSchema>,
    /// Structured diagnostic catalog.
    pub diagnostics: Vec<DiagnosticSchema>,
    /// Named budget profiles.
    pub budgets: Vec<BudgetSchema>,
}

/// Context compiler.
pub struct ContextBuilder;

impl ContextBuilder {
    /// Compile the smallest requested slice in stable order.
    #[must_use]
    pub fn compile(
        index: &SemanticProjectIndex,
        catalog: &SchemaCatalog,
        memory: &ProjectMemory,
        request: &ContextRequest,
    ) -> CompiledContext {
        let limit = usize::try_from(request.max_entries.max(1)).unwrap_or(usize::MAX);
        let in_scope = |row: &SemanticEntry| {
            request.anchors.is_empty()
                || request.anchors.contains(&row.anchor)
                || row
                    .parent
                    .is_some_and(|parent| request.anchors.contains(&parent))
        };
        let mut entries = Vec::new();
        let mut omissions = Vec::new();
        for row in &index.entries {
            if !in_scope(row) {
                omissions.push(ContextOmission {
                    anchor: row.anchor,
                    reason: "outside_scope".into(),
                });
            } else if entries.len() == limit {
                omissions.push(ContextOmission {
                    anchor: row.anchor,
                    reason: "entry_cap".into(),
                });
            } else {
                entries.push(row.clone());
            }
        }
        let approved = if request.include_memory {
            memory
                .rows()
                .iter()
                .filter(|row| request.anchors.is_empty() || request.anchors.contains(&row.anchor))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        CompiledContext {
            project_hash: index.project_hash,
            schema_version: index.schema_version,
            catalog_hash: catalog.toolchain_hash,
            schema: SchemaContext {
                operations: catalog.operations.clone(),
                patterns: catalog.patterns.clone(),
                diagnostics: catalog.diagnostics.clone(),
                budgets: catalog.budgets.clone(),
            },
            entries,
            memory: approved,
            omissions,
        }
    }
}
