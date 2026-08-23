//! Compiled artifact kinds stored in the CAS (HLD §4).

use crate::encode::CanonBuf;

/// What a CAS blob claims to be. Kind is provenance metadata; the `BlobId`
/// is still the blake3 of the raw bytes (same bytes ⇒ same id).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
#[repr(u8)]
pub enum ArtifactKind {
    /// v1 geometry. Not mesh-shader meshlets.
    ClusteredMesh = 0,
    /// Integer AABB / capsule hull.
    Hull = 1,
    /// Texture bytes (header-validated before GPU upload).
    Texture = 2,
    /// Audio grain.
    Grain = 3,
    /// v1 verb→clip set. v2 MotionDb.
    ClipSet = 4,
    /// 12-op rite bytecode.
    RiteChunk = 5,
    /// Cooked affordance graph.
    AffordanceGraph = 6,
    /// Optional embedding (v2 Weaver). Stored, not interpreted, in v1.
    Embedding = 7,
}

impl ArtifactKind {
    pub(crate) fn encode(self, buf: &mut CanonBuf) {
        buf.u8(self as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(ArtifactKind::ClusteredMesh as u8, 0);
        assert_eq!(ArtifactKind::Embedding as u8, 7);
    }
}
