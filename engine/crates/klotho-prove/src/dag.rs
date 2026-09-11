//! Provenance DAG: Entity / Activity / Agent, hashed ids, Unknown wash.

use std::collections::BTreeMap;
use std::fmt;

use klotho_core::{BlobId, Hash};
use serde::{Deserialize, Serialize};

use crate::artifact::ArtifactKind;
use crate::cas::Cas;
use crate::digest::hash_bytes;
use crate::encode::CanonBuf;
use crate::error::ProveError;
use crate::license::LicenseSpan;

/// Encoding version for a provenance node. Bump ⇒ every `ProvenanceId` changes.
const NODE_VERSION: u8 = 1;

/// Stable id of a provenance node: blake3 of its canonical encoding.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct ProvenanceId(pub Hash);

impl ProvenanceId {
    /// Borrow the 32 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

impl fmt::Display for ProvenanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "prov:{}", self.0)
    }
}

/// What a node records (PROV-DM Entity / Activity / Agent, frozen for v1).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum ProvenanceKind {
    /// Content-addressed artifact sitting in the CAS.
    Artifact {
        /// Blob id (blake3 of the bytes).
        blob: BlobId,
        /// Declared kind. Does not affect `blob`.
        kind: ArtifactKind,
    },
    /// Authoring Intent document, identified by its IR hash.
    Intent {
        /// Hash of the canonical IntentDoc bytes (computed by `klotho-ir`).
        doc_hash: Hash,
    },
    /// Cook / Pin / Commit activity.
    Activity {
        /// Which activity.
        activity: Activity,
    },
    /// Human, compiler, kitbash source, or model.
    Agent {
        /// Which agent.
        agent: Agent,
    },
    /// Committed Trace prefix. The hash is the prefix hash, not a CAS blob.
    TracePrefix {
        /// Prefix hash as defined by `klotho-trace`.
        prefix_hash: Hash,
    },
}

/// Cook-time or kernel activity that produced child entities.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum Activity {
    /// IntentDoc + kitbash → CAS blobs / Canon.
    Cook = 0,
    /// Distaff Pin of a preview fact into Canon or seed Trace.
    Pin = 1,
    /// `CommitKernel` append to Trace.
    Commit = 2,
}

/// Who participated. `Model` may appear on a proposal's provenance; it cannot
/// be the sole agent of a committed Trace event that impersonates `PlayerIntent`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Agent {
    /// Human author.
    Author,
    /// Compiler / Weaver version that cooked the blob.
    Compiler {
        /// Monotonic cook toolchain version mixed into artifact hashes.
        version: u32,
    },
    /// Reviewed kitbash source.
    Kitbash,
    /// Inference host. Recorded for audit; not a Player.
    Model,
}

/// One DAG node. `id` is determined by `kind`, effective `license`, and parents.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ProvenanceNode {
    /// Content id of this node.
    pub id: ProvenanceId,
    /// Entity / activity / agent / trace prefix.
    pub kind: ProvenanceKind,
    /// Effective license after Unknown wash.
    pub license: LicenseSpan,
    /// `wasDerivedFrom` parents, sorted.
    pub parents: Vec<ProvenanceId>,
}

/// Provenance DAG. Iteration is ordered (K25).
#[derive(Clone, Debug, Default)]
pub struct ProvenanceDag {
    nodes: BTreeMap<ProvenanceId, ProvenanceNode>,
}

impl ProvenanceDag {
    /// Empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
        }
    }

    /// Insert a node. Parents must already exist. Parent order does not
    /// affect the id (they are sorted). If any parent (or `license`) is
    /// Unknown, the stored license is Unknown.
    pub fn insert(
        &mut self,
        kind: ProvenanceKind,
        license: LicenseSpan,
        parents: &[ProvenanceId],
    ) -> Result<ProvenanceId, ProveError> {
        let mut sorted = parents.to_vec();
        sorted.sort();
        sorted.dedup();

        let mut effective = license;
        for p in &sorted {
            let parent = self.nodes.get(p).ok_or(ProveError::MissingParent(*p))?;
            effective = effective.wash(&parent.license);
        }

        let id = hash_node(&kind, &effective, &sorted);
        let node = ProvenanceNode {
            id,
            kind,
            license: effective,
            parents: sorted,
        };
        if let Some(existing) = self.nodes.get(&id) {
            debug_assert_eq!(existing, &node);
            return Ok(id);
        }
        self.nodes.insert(id, node);
        Ok(id)
    }

    /// Borrow a node.
    #[must_use]
    pub fn node(&self, id: ProvenanceId) -> Option<&ProvenanceNode> {
        self.nodes.get(&id)
    }

    /// Number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` if the DAG is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Ordered nodes.
    pub fn iter(&self) -> impl Iterator<Item = &ProvenanceNode> {
        self.nodes.values()
    }

    /// `Ok(())` iff every node is exportable (no `Unknown` license).
    pub fn exportable(&self) -> Result<(), ProveError> {
        for n in self.nodes.values() {
            if !n.license.is_exportable() {
                return Err(ProveError::UnknownLicense);
            }
        }
        Ok(())
    }

    /// Every `Artifact` node must have its blob in `cas`.
    pub fn blobs_present(&self, cas: &Cas) -> Result<(), ProveError> {
        for n in self.nodes.values() {
            if let ProvenanceKind::Artifact { blob, .. } = n.kind {
                if !cas.contains(blob) {
                    return Err(ProveError::MissingBlob(blob));
                }
            }
        }
        Ok(())
    }
}

fn hash_node(
    kind: &ProvenanceKind,
    license: &LicenseSpan,
    parents: &[ProvenanceId],
) -> ProvenanceId {
    let mut buf = CanonBuf::new();
    buf.u8(NODE_VERSION);
    encode_kind(kind, &mut buf);
    license.encode(&mut buf);
    buf.u32_le(parents.len() as u32);
    for p in parents {
        buf.arr32(p.as_bytes());
    }
    ProvenanceId(hash_bytes(buf.as_slice()))
}

fn encode_kind(kind: &ProvenanceKind, buf: &mut CanonBuf) {
    match kind {
        ProvenanceKind::Artifact { blob, kind } => {
            buf.u8(0);
            buf.arr32(blob.as_bytes());
            kind.encode(buf);
        }
        ProvenanceKind::Intent { doc_hash } => {
            buf.u8(1);
            buf.arr32(doc_hash.as_bytes());
        }
        ProvenanceKind::Activity { activity } => {
            buf.u8(2);
            buf.u8(*activity as u8);
        }
        ProvenanceKind::Agent { agent } => {
            buf.u8(3);
            match agent {
                Agent::Author => buf.u8(0),
                Agent::Compiler { version } => {
                    buf.u8(1);
                    buf.u32_le(*version);
                }
                Agent::Kitbash => buf.u8(2),
                Agent::Model => buf.u8(3),
            }
        }
        ProvenanceKind::TracePrefix { prefix_hash } => {
            buf.u8(4);
            buf.arr32(prefix_hash.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cas;

    fn mit() -> LicenseSpan {
        LicenseSpan::spdx("MIT", "Copyright 2026").unwrap()
    }

    fn author(dag: &mut ProvenanceDag) -> ProvenanceId {
        dag.insert(
            ProvenanceKind::Agent {
                agent: Agent::Author,
            },
            mit(),
            &[],
        )
        .unwrap()
    }

    #[test]
    fn missing_parent_is_an_error() {
        let mut dag = ProvenanceDag::new();
        let ghost = ProvenanceId(Hash::ZERO);
        let err = dag
            .insert(
                ProvenanceKind::Activity {
                    activity: Activity::Cook,
                },
                mit(),
                &[ghost],
            )
            .unwrap_err();
        assert_eq!(err, ProveError::MissingParent(ghost));
    }

    #[test]
    fn parent_order_does_not_affect_id() {
        let mut dag = ProvenanceDag::new();
        let a = author(&mut dag);
        let k = dag
            .insert(
                ProvenanceKind::Agent {
                    agent: Agent::Kitbash,
                },
                mit(),
                &[],
            )
            .unwrap();
        let cook = ProvenanceKind::Activity {
            activity: Activity::Cook,
        };
        let id_ab = dag.insert(cook, mit(), &[a, k]).unwrap();
        let id_ba = dag.insert(cook, mit(), &[k, a]).unwrap();
        assert_eq!(id_ab, id_ba);
        assert_eq!(dag.len(), 3);
        assert_eq!(dag.node(id_ab).unwrap().parents, vec![a.min(k), a.max(k)]);
    }

    #[test]
    fn unknown_license_fails_export() {
        let mut dag = ProvenanceDag::new();
        dag.insert(
            ProvenanceKind::Agent {
                agent: Agent::Author,
            },
            LicenseSpan::Unknown,
            &[],
        )
        .unwrap();
        assert_eq!(dag.exportable(), Err(ProveError::UnknownLicense));
    }

    #[test]
    fn unknown_parent_washes_child() {
        let mut dag = ProvenanceDag::new();
        let src = dag
            .insert(
                ProvenanceKind::Agent {
                    agent: Agent::Kitbash,
                },
                LicenseSpan::Unknown,
                &[],
            )
            .unwrap();
        let child = dag
            .insert(
                ProvenanceKind::Activity {
                    activity: Activity::Cook,
                },
                mit(),
                &[src],
            )
            .unwrap();
        assert_eq!(dag.node(child).unwrap().license, LicenseSpan::Unknown);
        assert_eq!(dag.exportable(), Err(ProveError::UnknownLicense));
    }

    #[test]
    fn licensed_dag_is_exportable() {
        let mut dag = ProvenanceDag::new();
        author(&mut dag);
        dag.exportable().unwrap();
    }

    #[test]
    fn artifact_must_be_in_cas() {
        let mut dag = ProvenanceDag::new();
        let mut cas = Cas::new();
        let blob = cas.put(b"hull").unwrap();
        dag.insert(
            ProvenanceKind::Artifact {
                blob,
                kind: ArtifactKind::Hull,
            },
            mit(),
            &[],
        )
        .unwrap();
        dag.blobs_present(&cas).unwrap();

        let mut empty = Cas::new();
        let err = dag.blobs_present(&empty).unwrap_err();
        assert_eq!(err, ProveError::MissingBlob(blob));
        // Putting other bytes does not satisfy the named blob.
        empty.put(b"other").unwrap();
        assert_eq!(
            dag.blobs_present(&empty),
            Err(ProveError::MissingBlob(blob))
        );
    }

    #[test]
    fn node_id_is_stable_for_same_encoding() {
        let mut a = ProvenanceDag::new();
        let mut b = ProvenanceDag::new();
        let ka = ProvenanceKind::Agent {
            agent: Agent::Compiler { version: 1 },
        };
        let id_a = a.insert(ka, mit(), &[]).unwrap();
        let id_b = b.insert(ka, mit(), &[]).unwrap();
        assert_eq!(id_a, id_b);
    }

    #[test]
    fn golden_author_node_id() {
        // Pin node encoding (version, kind tags, license tags, parent list).
        let mut dag = ProvenanceDag::new();
        let id = author(&mut dag);
        assert_eq!(
            id.to_string(),
            "prov:446a036cc368f0d46b3069703ee2c24f1c5056cb1a9ecc9c77d15279e03f1503"
        );
    }
}
