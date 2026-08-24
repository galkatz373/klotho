//! Visual presentation buffer. GPU handles are presenter-owned (PR 12).

use klotho_core::{AabbMm, BlobId, Epoch, IVec3, PoseMm, Sigil};

use crate::material::MaterialTag;

/// Slot for a GPU resource the presenter fills after upload. `0` = not uploaded.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct GpuHandle(pub u32);

impl GpuHandle {
    /// Not yet uploaded.
    pub const NONE: Self = Self(0);

    /// `true` if a presenter has filled this slot.
    #[must_use]
    pub const fn is_uploaded(self) -> bool {
        self.0 != 0
    }
}

/// One clustered static mesh instance (not a mesh-shader meshlet).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ClusterRef {
    /// CAS blob id of the clustered-mesh bytes.
    pub blob: BlobId,
    /// Presenter GPU slot. Manifest extract leaves this [`GpuHandle::NONE`].
    pub gpu: GpuHandle,
    /// Integer millimetre placement.
    pub pose: PoseMm,
}

/// Material binding. Palette index is into style Intent palettes.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct MaterialRef {
    /// Closed tag.
    pub tag: MaterialTag,
    /// Index into the cooked style palette list (`stone` / `metal` / `organic`).
    pub palette: u8,
}

/// v1 light. No clustered deferred; stubs for the unlit+lambert family.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct LightStub {
    /// Position, millimetres.
    pub pos: IVec3,
    /// Kind.
    pub kind: LightKind,
}

/// Closed v1 light kinds.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum LightKind {
    /// Omni. Intensity is milli-units (1000 = 1.0).
    Point {
        /// Milli-intensity.
        intensity_milli: u16,
    },
    /// Emissive surface (forge, fire). No separate radius in v1.
    Emissive,
}

/// Dumb visual buffer. The presenter borrows this; it does not own World.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct VisualManifest {
    /// Cook / hull epoch of the snapshot this was extracted from.
    pub epoch: Epoch,
    /// Clustered mesh instances.
    pub clusters: Vec<ClusterRef>,
    /// Parallel material for each cluster (`materials.len() == clusters.len()`).
    pub materials: Vec<MaterialRef>,
    /// Lights. Empty is valid (unlit).
    pub lights: Vec<LightStub>,
    /// Debug overlays only. Not the hot identity path.
    pub debug_sigils: Vec<(Sigil, AabbMm)>,
}

impl VisualManifest {
    /// Empty buffer at `epoch`.
    #[must_use]
    pub const fn empty(epoch: Epoch) -> Self {
        Self {
            epoch,
            clusters: Vec::new(),
            materials: Vec::new(),
            lights: Vec::new(),
            debug_sigils: Vec::new(),
        }
    }

    /// Cluster count after GPU-budget clamp is the presenter's job (PR 12).
    #[must_use]
    pub fn cluster_count(&self) -> usize {
        self.clusters.len()
    }

    /// Build from instances via the crate-private SoA (K2: callers never name `tables`).
    #[must_use]
    pub fn from_instances(
        epoch: Epoch,
        items: impl IntoIterator<Item = (BlobId, PoseMm, MaterialRef)>,
        lights: impl IntoIterator<Item = LightStub>,
        debug: impl IntoIterator<Item = (Sigil, AabbMm)>,
    ) -> Self {
        let mut t = crate::tables::VisualTables::new();
        for (blob, pose, material) in items {
            t.push(blob, pose, material);
        }
        for light in lights {
            t.push_light(light);
        }
        for (s, hull) in debug {
            t.push_debug(s, hull);
        }
        t.extract(epoch)
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{Epoch, Hash, LocusKind, Mm, PoseMm, Sigil, YawMd};

    use super::*;
    use crate::tables::VisualTables;
    use crate::{MaterialTag, Observer};

    #[test]
    fn extract_has_no_hot_path_sigils() {
        let mut t = VisualTables::new();
        t.push(
            BlobId(*Hash::ZERO.as_bytes()),
            PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
            MaterialRef {
                tag: MaterialTag::Organic,
                palette: 0,
            },
        );
        let vis = t.extract(Epoch::ZERO);
        assert_eq!(vis.clusters.len(), 1);
        assert_eq!(vis.materials.len(), 1);
        assert!(vis.debug_sigils.is_empty());
        assert!(!vis.clusters[0].gpu.is_uploaded());
        let _eye: Observer = Observer::origin();
    }

    #[test]
    fn debug_sigils_are_the_only_identity_channel() {
        let mut t = VisualTables::new();
        let s = Sigil::pack(LocusKind::Relic, 0, 1).unwrap();
        t.push_debug(s, AabbMm::from_point(IVec3 { x: 0, y: 0, z: 0 }));
        let vis = t.extract(Epoch(3));
        assert_eq!(vis.epoch, Epoch(3));
        assert_eq!(
            vis.debug_sigils,
            vec![(s, AabbMm::from_point(IVec3 { x: 0, y: 0, z: 0 }))]
        );
        assert!(vis.clusters.is_empty());
    }
}
