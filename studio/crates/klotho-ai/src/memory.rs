//! Persistent project memory made only from approved, source-linked decisions.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::{AnchorId, Name, from_ron, to_ron};

use crate::error::AiError;

/// Named human approval for a memory row.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryApproval {
    /// Human or accountable role.
    pub by: Name,
    /// Approval record hash.
    pub record_hash: Hash,
}

/// Approved decision or summary tied to exact source bytes.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedMemory {
    /// Stable semantic subject.
    pub anchor: AnchorId,
    /// Human-readable approved summary.
    pub summary: String,
    /// Exact sources supporting the summary.
    pub source_hashes: Vec<Hash>,
    /// Human approval.
    pub approval: MemoryApproval,
}

/// Content-addressed approved-memory ledger.
pub struct ProjectMemory {
    path: PathBuf,
    rows: Vec<ApprovedMemory>,
}

impl ProjectMemory {
    /// Open or create a ledger beneath the isolated authoring workspace.
    pub fn open(path: &Path) -> Result<Self, AiError> {
        let rows = if path.exists() {
            let text = fs::read_to_string(path).map_err(|e| AiError::Io(e.to_string()))?;
            from_ron(&text).map_err(|e| AiError::Ser(e.to_string()))?
        } else {
            Vec::new()
        };
        Ok(Self {
            path: path.to_path_buf(),
            rows,
        })
    }

    /// Append an approved row. Empty summaries/sources fail closed.
    pub fn approve(&mut self, mut row: ApprovedMemory) -> Result<(), AiError> {
        if row.summary.trim().is_empty() || row.source_hashes.is_empty() {
            return Err(AiError::UnapprovedMemory);
        }
        row.source_hashes.sort();
        row.source_hashes.dedup();
        self.rows.push(row);
        self.rows.sort_by(|a, b| {
            a.anchor
                .cmp(&b.anchor)
                .then(a.approval.record_hash.cmp(&b.approval.record_hash))
        });
        self.persist()
    }

    /// Read-only approved rows.
    #[must_use]
    pub fn rows(&self) -> &[ApprovedMemory] {
        &self.rows
    }

    fn persist(&self) -> Result<(), AiError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| AiError::Io(e.to_string()))?;
        }
        let text = to_ron(&self.rows).map_err(|e| AiError::Ser(e.to_string()))?;
        let temp = self.path.with_extension("tmp");
        fs::write(&temp, text).map_err(|e| AiError::Io(e.to_string()))?;
        fs::rename(&temp, &self.path).map_err(|e| AiError::Io(e.to_string()))
    }
}
