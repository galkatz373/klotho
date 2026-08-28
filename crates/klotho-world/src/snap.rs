//! Place column blob carried on a residency proposal. Not a source.

use klotho_core::{
    AabbMm, BlobId, Hash, LocusKind, PhysRequest, PoseMm, ResourceId, Sigil, SimLod, Vel3,
};
use klotho_ir::Rel;

/// Process per-Place row bomb. Apply rejects a larger snap; it is not truncated.
pub const MAX_PLACE_ROWS: usize = 100_000;

/// Compact Place payload. Kernel applies it in one transaction (K21).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceSnap {
    /// Place this blob belongs to.
    pub place: Sigil,
    /// Cook digest the rows were captured under.
    pub canon_hash: Hash,
    /// Trace prefix the rows were captured under.
    pub prefix: Hash,
    rows: Vec<PlaceRow>,
}

/// One packed locus as it should be restored.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceRow {
    /// Identity.
    pub sigil: Sigil,
    /// Packed kind.
    pub kind: LocusKind,
    /// Pose, if any.
    pub pose: Option<PoseMm>,
    /// Linear velocity.
    pub vel: Vel3,
    /// Yaw rate, millideg / tick.
    pub yaw_rate: i32,
    /// Local hull AABB.
    pub hull: Option<AabbMm>,
    /// Canonical hull blob.
    pub hull_id: BlobId,
    /// Affordance bits 0..63.
    pub afford: u64,
    /// Non-zero quantity rows.
    pub qty: Vec<(ResourceId, i32)>,
    /// Outgoing relations.
    pub rels: Vec<(Rel, Sigil)>,
    /// Contact-group id.
    pub island: u16,
    /// Sleep ticks.
    pub sleep: u16,
    /// Simulation LOD.
    pub sim_lod: SimLod,
    /// Pending `PHYS_REQ`, if any.
    pub phys_req: Option<PhysRequest>,
    /// Known fact ids.
    pub knows: Vec<u16>,
}

impl PlaceRow {
    /// Empty columns for `sigil`.
    #[must_use]
    pub fn new(sigil: Sigil, kind: LocusKind) -> Self {
        Self {
            sigil,
            kind,
            pose: None,
            vel: Vel3::ZERO,
            yaw_rate: 0,
            hull: None,
            hull_id: BlobId::ZERO,
            afford: 0,
            qty: Vec::new(),
            rels: Vec::new(),
            island: 0,
            sleep: 0,
            sim_lod: SimLod::Full,
            phys_req: None,
            knows: Vec::new(),
        }
    }
}

impl PlaceSnap {
    /// Construct without validating row count. Apply fails closed on oversize.
    #[must_use]
    pub fn new(place: Sigil, canon_hash: Hash, prefix: Hash, rows: Vec<PlaceRow>) -> Self {
        Self {
            place,
            canon_hash,
            prefix,
            rows,
        }
    }

    /// Packed rows in capture / author order.
    #[must_use]
    pub fn rows(&self) -> &[PlaceRow] {
        &self.rows
    }

    /// Row count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// No rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::LocusKind;

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    #[test]
    fn new_does_not_truncate_oversize() {
        let n = MAX_PLACE_ROWS + 1;
        let rows = vec![PlaceRow::new(relic(1), LocusKind::Relic); n];
        let snap = PlaceSnap::new(place(1), Hash::ZERO, Hash::ZERO, rows);
        assert_eq!(snap.len(), n);
    }
}
