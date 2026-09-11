//! Semantic diff and impact graph for a transaction.

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::AnchorId;

use crate::ids::{Cell, ChangeId};
use crate::ops::AuthorOp;

/// Semantic diff of a transaction against its base.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticDiff {
    /// Transaction change id.
    pub change: ChangeId,
    /// Base project hash.
    pub base_hash: Hash,
    /// Current snapshot hash.
    pub current_hash: Hash,
    /// Applied ops.
    pub ops: Vec<AuthorOp>,
    /// Written cells.
    pub writes: Vec<Cell>,
    /// Read cells.
    pub reads: Vec<Cell>,
    /// Impact graph: object → dependents.
    pub impact: Vec<ImpactEdge>,
}

/// One impact edge.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactEdge {
    /// Edited object.
    pub target: AnchorId,
    /// Dependent that reads it.
    pub dependent: AnchorId,
}
