//! Versioned structural and disposable embedding indexes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::{AnchorId, AnchorKind};
use klotho_prove::hash_bytes;

use crate::error::AiError;
use crate::workspace::AuthoringSnapshot;

/// Structural row family.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticEntryKind {
    /// Intent module.
    Module,
    /// Named object within a module.
    Object,
    /// Pattern instance.
    Pattern,
    /// Approved asset candidate edge.
    Asset,
    /// Approved reference edge.
    Reference,
}

/// Exact, hash-linked structural index row.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticEntry {
    /// Stable semantic identity.
    pub anchor: AnchorId,
    /// Row family.
    pub kind: SemanticEntryKind,
    /// Current display label.
    pub label: String,
    /// Owning module for non-module rows.
    pub parent: Option<AnchorId>,
    /// Hash of the exact indexed source row.
    pub source_hash: Hash,
    /// Object kind when applicable.
    pub anchor_kind: Option<AnchorKind>,
}

/// Incremental refresh result.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct IndexDelta {
    /// Added identities.
    pub added: Vec<AnchorId>,
    /// Changed identities.
    pub changed: Vec<AnchorId>,
    /// Removed identities.
    pub removed: Vec<AnchorId>,
}

/// Distance function is part of an embedding cache identity.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceMetric {
    /// Cosine distance.
    Cosine,
    /// Dot-product ordering.
    Dot,
    /// Euclidean distance.
    Euclidean,
}

/// Complete cache key. Mixed-key queries are forbidden.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingKey {
    /// Project source hash.
    pub project_hash: Hash,
    /// Generated schema version.
    pub schema_version: u32,
    /// Exact chunker implementation.
    pub chunker_hash: Hash,
    /// Adapter implementation.
    pub backend_hash: Hash,
    /// Model weights/version.
    pub model_hash: Hash,
    /// Distance metric.
    pub distance: DistanceMetric,
}

/// Quantized disposable vector tied to one exact source row.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingRow {
    /// Indexed anchor.
    pub anchor: AnchorId,
    /// Source hash at embedding time.
    pub source_hash: Hash,
    /// Quantized vector; retrieval is advisory only.
    pub vector: Vec<i16>,
}

/// Disposable retrieval index.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingIndex {
    /// Complete key.
    pub key: EmbeddingKey,
    /// Stable anchor order.
    pub rows: Vec<EmbeddingRow>,
}

/// Authoritative structural index plus optional disposable retrieval cache.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SemanticProjectIndex {
    /// Exact indexed project hash.
    pub project_hash: Hash,
    /// Schema format used for context.
    pub schema_version: u32,
    /// Stable rows.
    pub entries: Vec<SemanticEntry>,
    /// Optional exact-key embedding cache.
    pub embeddings: Option<EmbeddingIndex>,
}

impl SemanticProjectIndex {
    /// Build from locked authoring data.
    pub fn build(snapshot: &AuthoringSnapshot, schema_version: u32) -> Result<Self, AiError> {
        let project_hash = snapshot.project_hash()?;
        let mut entries = Vec::new();
        for module in &snapshot.modules {
            entries.push(SemanticEntry {
                anchor: module.anchor,
                kind: SemanticEntryKind::Module,
                label: module.id.as_str().to_owned(),
                parent: None,
                source_hash: module.content_hash()?,
                anchor_kind: Some(AnchorKind::Module),
            });
            for object in &module.object_anchors {
                let bytes =
                    klotho_ir::to_ron(object).map_err(|error| AiError::Ser(error.to_string()))?;
                entries.push(SemanticEntry {
                    anchor: object.anchor,
                    kind: SemanticEntryKind::Object,
                    label: object.name.as_str().to_owned(),
                    parent: Some(module.anchor),
                    source_hash: hash_bytes(bytes.as_bytes()),
                    anchor_kind: Some(object.kind),
                });
            }
            for instance in &module.patterns {
                let bytes =
                    klotho_ir::to_ron(instance).map_err(|error| AiError::Ser(error.to_string()))?;
                entries.push(SemanticEntry {
                    anchor: instance.anchor,
                    kind: SemanticEntryKind::Pattern,
                    label: instance.instance.as_str().to_owned(),
                    parent: Some(module.anchor),
                    source_hash: hash_bytes(bytes.as_bytes()),
                    anchor_kind: Some(AnchorKind::Pattern),
                });
            }
        }
        for (anchor, requests) in &snapshot.assets {
            for request in requests {
                let label = request.to_string();
                entries.push(SemanticEntry {
                    anchor: *anchor,
                    kind: SemanticEntryKind::Asset,
                    label: label.clone(),
                    parent: snapshot.owning_module(*anchor),
                    source_hash: hash_bytes(label.as_bytes()),
                    anchor_kind: None,
                });
            }
        }
        for (anchor, references) in &snapshot.references {
            for reference in references {
                let label = reference.to_string();
                entries.push(SemanticEntry {
                    anchor: *anchor,
                    kind: SemanticEntryKind::Reference,
                    label: label.clone(),
                    parent: snapshot.owning_module(*anchor),
                    source_hash: hash_bytes(label.as_bytes()),
                    anchor_kind: None,
                });
            }
        }
        entries.sort();
        Ok(Self {
            project_hash,
            schema_version,
            entries,
            embeddings: None,
        })
    }

    /// Refresh structural rows, invalidating retrieval on any project change.
    pub fn refresh(&mut self, snapshot: &AuthoringSnapshot) -> Result<IndexDelta, AiError> {
        let next = Self::build(snapshot, self.schema_version)?;
        let old: BTreeMap<_, _> = self
            .entries
            .iter()
            .map(|row| ((row.anchor, row.kind, row.label.as_str()), row))
            .collect();
        let new: BTreeMap<_, _> = next
            .entries
            .iter()
            .map(|row| ((row.anchor, row.kind, row.label.as_str()), row))
            .collect();
        let mut delta = IndexDelta::default();
        for ((anchor, _, _), row) in &new {
            match old.get(&(*anchor, row.kind, row.label.as_str())) {
                None => delta.added.push(*anchor),
                Some(previous) if **previous != **row => delta.changed.push(*anchor),
                Some(_) => {}
            }
        }
        for (anchor, kind, label) in old.keys() {
            if !new.contains_key(&(*anchor, *kind, *label)) {
                delta.removed.push(*anchor);
            }
        }
        delta.added.sort();
        delta.added.dedup();
        delta.changed.sort();
        delta.changed.dedup();
        delta.removed.sort();
        delta.removed.dedup();
        let changed = self.project_hash != next.project_hash;
        self.project_hash = next.project_hash;
        self.entries = next.entries;
        if changed {
            self.embeddings = None;
        }
        Ok(delta)
    }

    /// Install a complete retrieval cache. Stale rows and mixed keys fail closed.
    pub fn install_embeddings(&mut self, mut index: EmbeddingIndex) -> Result<(), AiError> {
        if index.key.project_hash != self.project_hash
            || index.key.schema_version != self.schema_version
        {
            return Err(AiError::EmbeddingKeyMismatch);
        }
        index.rows.sort_by_key(|row| row.anchor);
        for row in &index.rows {
            if !self
                .entries
                .iter()
                .any(|entry| entry.anchor == row.anchor && entry.source_hash == row.source_hash)
            {
                return Err(AiError::EmbeddingKeyMismatch);
            }
        }
        self.embeddings = Some(index);
        Ok(())
    }

    /// Query only when the caller supplies the exact cache key.
    pub fn embedding_rows(&self, key: &EmbeddingKey) -> Result<&[EmbeddingRow], AiError> {
        let index = self
            .embeddings
            .as_ref()
            .ok_or(AiError::EmbeddingKeyMismatch)?;
        if &index.key != key {
            return Err(AiError::EmbeddingKeyMismatch);
        }
        Ok(&index.rows)
    }
}
