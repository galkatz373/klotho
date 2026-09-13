//! Clustered geometry and texture residency (KAI-17).
//!
//! The pager never builds a `Proposal`. LOD choice is a function of distance
//! and the pinned quality tier.

use klotho_core::{BlobId, IVec3};
use klotho_ir::QualityTier;

/// One clustered-mesh LOD chain in a catalog volume.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct ClusterLod {
    /// LOD0 (highest).
    pub lod0: BlobId,
    /// Optional cheaper LODs, coarsest last.
    pub lods: Vec<BlobId>,
    /// World millimetre position used for distance.
    pub at: IVec3,
    /// Resident texture bytes at LOD0.
    pub texture_bytes: u32,
}

/// One texture stream request. Mip 0 is full resolution.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct TextureResident {
    /// Texture CAS id.
    pub blob: BlobId,
    /// Selected mip (`0` = full).
    pub mip: u8,
    /// Bytes after mip selection.
    pub bytes: u32,
}

/// Geometry + texture residency for one extract.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct PresentResidency {
    /// Selected clustered-mesh blobs.
    pub meshes: Vec<BlobId>,
    /// Selected texture mips.
    pub textures: Vec<TextureResident>,
    /// Sum of selected texture bytes.
    pub texture_bytes: u64,
}

/// Pick mesh LOD and texture mip from observer distance and quality.
#[must_use]
pub fn plan_residency(
    clusters: &[ClusterLod],
    textures: &[(BlobId, u32)],
    eye: IVec3,
    tier: QualityTier,
) -> PresentResidency {
    let mut out = PresentResidency::default();
    for c in clusters {
        out.meshes.push(select_lod(c, dist_mm(c.at, eye), tier));
        out.texture_bytes = out
            .texture_bytes
            .saturating_add(u64::from(c.texture_bytes) / mip_div(tier));
    }
    for &(blob, bytes) in textures {
        let mip = texture_mip(tier);
        let resident = bytes / (1u32 << (2 * u32::from(mip))).max(1);
        out.textures.push(TextureResident {
            blob,
            mip,
            bytes: resident,
        });
        out.texture_bytes = out.texture_bytes.saturating_add(u64::from(resident));
    }
    out
}

fn dist_mm(a: IVec3, b: IVec3) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    let dz = i64::from(a.z) - i64::from(b.z);
    dx.saturating_mul(dx)
        .saturating_add(dy.saturating_mul(dy))
        .saturating_add(dz.saturating_mul(dz))
}

fn select_lod(cluster: &ClusterLod, dist2: i64, tier: QualityTier) -> BlobId {
    if cluster.lods.is_empty() {
        return cluster.lod0;
    }
    let step = match tier {
        QualityTier::High => 8_000_000i64, //  ~2.8 m
        QualityTier::Medium => 2_000_000,  //  ~1.4 m
        QualityTier::Low => 500_000,       //  ~0.7 m
    };
    let idx = (dist2 / step).clamp(0, cluster.lods.len() as i64) as usize;
    if idx == 0 {
        cluster.lod0
    } else {
        cluster.lods[idx - 1]
    }
}

fn texture_mip(tier: QualityTier) -> u8 {
    match tier {
        QualityTier::High => 0,
        QualityTier::Medium => 1,
        QualityTier::Low => 2,
    }
}

fn mip_div(tier: QualityTier) -> u64 {
    match tier {
        QualityTier::High => 1,
        QualityTier::Medium => 4,
        QualityTier::Low => 16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(n: u8) -> BlobId {
        BlobId::from_bytes({
            let mut b = [0u8; 32];
            b[0] = n;
            b
        })
    }

    #[test]
    fn high_keeps_lod0_and_mip0() {
        let c = ClusterLod {
            lod0: blob(1),
            lods: vec![blob(2), blob(3)],
            at: IVec3 { x: 0, y: 0, z: 0 },
            texture_bytes: 4_096,
        };
        let plan = plan_residency(
            &[c],
            &[(blob(9), 4_096)],
            IVec3 { x: 0, y: 0, z: 0 },
            QualityTier::High,
        );
        assert_eq!(plan.meshes, vec![blob(1)]);
        assert_eq!(plan.textures[0].mip, 0);
        assert_eq!(plan.textures[0].bytes, 4_096);
    }

    #[test]
    fn low_drops_to_cheaper_lod_and_mip() {
        let c = ClusterLod {
            lod0: blob(1),
            lods: vec![blob(2), blob(3)],
            at: IVec3 {
                x: 2_000,
                y: 0,
                z: 0,
            },
            texture_bytes: 4_096,
        };
        let eye = IVec3 { x: 0, y: 0, z: 0 };
        let high = plan_residency(std::slice::from_ref(&c), &[], eye, QualityTier::High);
        let low = plan_residency(&[c], &[(blob(9), 4_096)], eye, QualityTier::Low);
        assert_ne!(high.meshes[0], low.meshes[0]);
        assert_eq!(low.textures[0].mip, 2);
        assert!(low.texture_bytes < 4_096);
    }
}
