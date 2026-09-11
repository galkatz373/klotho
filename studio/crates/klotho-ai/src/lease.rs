//! Renewable anchor-tree leases. Expiry does not authorize overwrite.

use serde::{Deserialize, Serialize};

use klotho_ir::AnchorId;

use crate::ids::{LeaseId, TxId};
use crate::workspace::AuthoringSnapshot;

/// Lease on a subtree rooted at `root`.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    /// Lease identity.
    pub id: LeaseId,
    /// Holding transaction.
    pub tx: TxId,
    /// Subtree root.
    pub root: AnchorId,
    /// Exclusive expiry on the store sequence clock.
    pub expires_at: u64,
}

impl Lease {
    /// True when `now` is past expiry. Expired leases do not grant writes.
    #[must_use]
    pub fn expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }
}

/// Anchors covered by a lease on `root`.
#[must_use]
pub fn subtree(snap: &AuthoringSnapshot, root: AnchorId) -> Vec<AnchorId> {
    let mut out = vec![root];
    if let Some(module) = snap.modules.iter().find(|m| m.anchor == root) {
        for object in &module.object_anchors {
            out.push(object.anchor);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// True when two subtrees share an anchor.
#[must_use]
pub fn overlaps(a: &[AnchorId], b: &[AnchorId]) -> bool {
    a.iter().any(|id| b.contains(id))
}
