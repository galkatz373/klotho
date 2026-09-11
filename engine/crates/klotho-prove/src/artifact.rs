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
    /// Skinned mesh: `i16` verts + 4-bone joints/weights + `u32` indices.
    SkinnedMesh = 8,
    /// Sealed evaluation evidence bundle (KAI-06).
    Evidence = 9,
}

impl ArtifactKind {
    /// Decode a packed kind byte. `None` for unknown future values.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::ClusteredMesh),
            1 => Some(Self::Hull),
            2 => Some(Self::Texture),
            3 => Some(Self::Grain),
            4 => Some(Self::ClipSet),
            5 => Some(Self::RiteChunk),
            6 => Some(Self::AffordanceGraph),
            7 => Some(Self::Embedding),
            8 => Some(Self::SkinnedMesh),
            9 => Some(Self::Evidence),
            _ => None,
        }
    }

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
        assert_eq!(ArtifactKind::SkinnedMesh as u8, 8);
        assert_eq!(ArtifactKind::Evidence as u8, 9);
        assert_eq!(ArtifactKind::from_u8(3), Some(ArtifactKind::Grain));
        assert_eq!(ArtifactKind::from_u8(8), Some(ArtifactKind::SkinnedMesh));
        assert_eq!(ArtifactKind::from_u8(9), Some(ArtifactKind::Evidence));
        assert_eq!(ArtifactKind::from_u8(10), None);
    }
}
