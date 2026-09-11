//! Visual presentation buffer. GPU handles are presenter-owned (PR 12).

use klotho_core::{AabbMm, BlobId, Epoch, IVec3, PoseMm, Sigil, Tick};

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
    /// Directional sun. `dir` is millimetre-scale, presenter-normalized.
    Sun {
        /// Direction vector (not unit; presenter normalizes).
        dir: IVec3,
        /// Milli-intensity.
        intensity_milli: u16,
    },
}

/// Which instance list a mesh extract lands in.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub enum InstancePass {
    /// Opaque statics. Hearth unlit path.
    #[default]
    Opaque,
    /// Alpha-tested / masked.
    Masked,
    /// Skinned palettes.
    Skinned,
}

/// One skinned instance. Palette index is into [`VisualManifest::palettes`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct SkinnedInstance {
    /// Skinned-mesh CAS blob.
    pub blob: BlobId,
    /// Presenter GPU slot. Extract leaves [`GpuHandle::NONE`].
    pub gpu: GpuHandle,
    /// Admitted root pose.
    pub pose: PoseMm,
    /// Index into [`VisualManifest::palettes`].
    pub palette: u16,
    /// Closed material.
    pub material: MaterialRef,
}

/// GPU skinning palette slot. CPU joints travel with the Manifest.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct PaletteSlot {
    /// Presenter GPU slot. Extract leaves [`GpuHandle::NONE`].
    pub gpu: GpuHandle,
    /// Bone count the clip/mesh declared. 0 = identity / T-pose.
    pub bones: u16,
    /// Local joint poses (`len == bones`). Empty when `bones == 0`.
    pub joints: Vec<PoseMm>,
}

impl PaletteSlot {
    /// Identity / T-pose slot.
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            gpu: GpuHandle::NONE,
            bones: 0,
            joints: Vec::new(),
        }
    }
}

/// Trace-driven decal. Presentation TTL only; no identity field.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Decal {
    /// CAS recipe (texture / mesh).
    pub blob: BlobId,
    /// Integer millimetre placement.
    pub pose: PoseMm,
    /// Closed material.
    pub material: MaterialRef,
    /// Tick the cue was committed.
    pub born: Tick,
    /// Live while `now < born + ttl_ticks`.
    pub ttl_ticks: u16,
}

/// One-shot debris mesh. Manifest TTL only; no identity field.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct OneShotMesh {
    /// CAS recipe (mesh).
    pub blob: BlobId,
    /// Integer millimetre placement.
    pub pose: PoseMm,
    /// Closed material.
    pub material: MaterialRef,
    /// Tick the cue was committed.
    pub born: Tick,
    /// Live while `now < born + ttl_ticks`.
    pub ttl_ticks: u16,
}

/// Cook-baked irradiance probe volume. SSGI is presenter-only.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ProbeGrid {
    /// Probe grid CAS blob.
    pub blob: BlobId,
    /// Grid origin, millimetres.
    pub origin: IVec3,
    /// Cell size, millimetres.
    pub spacing_mm: i32,
    /// Cell counts along X, Y, Z.
    pub dim: (u8, u8, u8),
}

/// Post-process permutation. Unlit Hearth goldens use [`PostFlags::UNLIT`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct PostFlags {
    /// Temporal AA. Off for competitive permutation.
    pub taa: bool,
    /// Bloom.
    pub bloom: bool,
    /// Irradiance probes + SSGI. Off for unlit and competitive.
    pub gi: bool,
    /// Shooter competitive: no TAA/GI, at most one cascade.
    pub competitive: bool,
    /// Color-grade LUT blob. `None` is identity.
    pub lut: Option<BlobId>,
}

impl PostFlags {
    /// Hearth unlit+lambert. Pixel goldens stay on this path.
    pub const UNLIT: Self = Self {
        taa: false,
        bloom: false,
        gi: false,
        competitive: false,
        lut: None,
    };

    /// Adventure presentation (probes + SSGI). Unused by Hearth extract.
    pub const ADVENTURE: Self = Self {
        taa: true,
        bloom: true,
        gi: true,
        competitive: false,
        lut: None,
    };

    /// Shooter competitive permutation.
    pub const COMPETITIVE: Self = Self {
        taa: false,
        bloom: false,
        gi: false,
        competitive: true,
        lut: None,
    };
}

impl Default for PostFlags {
    fn default() -> Self {
        Self::UNLIT
    }
}

/// Dumb visual buffer. The presenter borrows this; it does not own World.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct VisualManifest {
    /// Cook / hull epoch of the snapshot this was extracted from.
    pub epoch: Epoch,
    /// Snapshot tick.
    pub tick: Tick,
    /// Opaque clustered mesh instances (Hearth unlit path).
    pub clusters: Vec<ClusterRef>,
    /// Parallel material for each opaque cluster (`materials.len() == clusters.len()`).
    pub materials: Vec<MaterialRef>,
    /// Masked / alpha-tested instances.
    pub masked: Vec<ClusterRef>,
    /// Parallel material for [`Self::masked`].
    pub masked_materials: Vec<MaterialRef>,
    /// Skinned instances.
    pub skinned: Vec<SkinnedInstance>,
    /// Skinning palettes indexed by [`SkinnedInstance::palette`].
    pub palettes: Vec<PaletteSlot>,
    /// Lights. Empty is valid (unlit).
    pub lights: Vec<LightStub>,
    /// Cook-baked irradiance probes. Empty on Hearth.
    pub probes: Vec<ProbeGrid>,
    /// Post permutation. Hearth extract writes [`PostFlags::UNLIT`].
    pub post: PostFlags,
    /// Debug overlays only. Not the hot identity path.
    pub debug_sigils: Vec<(Sigil, AabbMm)>,
    /// Trace-driven decals. Empty on Hearth unlit extract.
    pub decals: Vec<Decal>,
    /// One-shot debris meshes. Empty on Hearth unlit extract.
    pub one_shots: Vec<OneShotMesh>,
}

impl VisualManifest {
    /// Empty buffer at `epoch`.
    #[must_use]
    pub const fn empty(epoch: Epoch) -> Self {
        Self {
            epoch,
            tick: Tick::ZERO,
            clusters: Vec::new(),
            materials: Vec::new(),
            masked: Vec::new(),
            masked_materials: Vec::new(),
            skinned: Vec::new(),
            palettes: Vec::new(),
            lights: Vec::new(),
            probes: Vec::new(),
            post: PostFlags::UNLIT,
            debug_sigils: Vec::new(),
            decals: Vec::new(),
            one_shots: Vec::new(),
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
        Self::from_v2(
            epoch,
            Tick::ZERO,
            items,
            [],
            [],
            [],
            lights,
            [],
            PostFlags::UNLIT,
            debug,
        )
    }

    /// Extract v2 lists through the crate-private SoA.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn from_v2(
        epoch: Epoch,
        tick: Tick,
        opaque: impl IntoIterator<Item = (BlobId, PoseMm, MaterialRef)>,
        masked: impl IntoIterator<Item = (BlobId, PoseMm, MaterialRef)>,
        skinned: impl IntoIterator<Item = SkinnedInstance>,
        palettes: impl IntoIterator<Item = PaletteSlot>,
        lights: impl IntoIterator<Item = LightStub>,
        probes: impl IntoIterator<Item = ProbeGrid>,
        post: PostFlags,
        debug: impl IntoIterator<Item = (Sigil, AabbMm)>,
    ) -> Self {
        let mut t = crate::tables::VisualTables::new();
        t.set_tick(tick);
        t.set_post(post);
        for (blob, pose, material) in opaque {
            t.push(blob, pose, material);
        }
        for (blob, pose, material) in masked {
            t.push_masked(blob, pose, material);
        }
        for slot in palettes {
            t.push_palette(slot);
        }
        for inst in skinned {
            t.push_skinned(inst);
        }
        for light in lights {
            t.push_light(light);
        }
        for probe in probes {
            t.push_probe(probe);
        }
        for (s, hull) in debug {
            t.push_debug(s, hull);
        }
        t.extract(epoch)
    }

    /// Visual buffer that holds only VFX lists. Other columns stay empty.
    #[must_use]
    pub fn from_vfx(
        epoch: Epoch,
        tick: Tick,
        decals: impl IntoIterator<Item = Decal>,
        one_shots: impl IntoIterator<Item = OneShotMesh>,
    ) -> Self {
        let mut t = crate::tables::VisualTables::new();
        t.set_tick(tick);
        for d in decals {
            t.push_decal(d);
        }
        for o in one_shots {
            t.push_oneshot(o);
        }
        t.extract(epoch)
    }

    /// Appends `decals` and `one_shots` onto `self`. Other columns are unchanged.
    #[must_use]
    pub fn with_vfx(
        mut self,
        decals: impl IntoIterator<Item = Decal>,
        one_shots: impl IntoIterator<Item = OneShotMesh>,
    ) -> Self {
        let add = Self::from_vfx(self.epoch, self.tick, decals, one_shots);
        self.decals.extend(add.decals);
        self.one_shots.extend(add.one_shots);
        self
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{BlobId, Epoch, Hash, IVec3, LocusKind, Mm, PoseMm, Sigil, Tick, YawMd};

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
        assert!(vis.masked.is_empty());
        assert!(vis.skinned.is_empty());
        assert!(vis.palettes.is_empty());
        assert!(vis.probes.is_empty());
        assert_eq!(vis.post, PostFlags::UNLIT);
        assert_eq!(vis.tick, Tick::ZERO);
        assert!(!vis.clusters[0].gpu.is_uploaded());
        assert!(vis.decals.is_empty());
        assert!(vis.one_shots.is_empty());
        let _eye: Observer = Observer::origin();
    }

    #[test]
    fn extract_v2_lists_stay_crate_private_soa() {
        let mut t = VisualTables::new();
        t.set_tick(Tick(9));
        t.push_masked(
            BlobId(*Hash::ZERO.as_bytes()),
            PoseMm::new(Mm(1), Mm(0), Mm(0), YawMd::ZERO),
            MaterialRef {
                tag: MaterialTag::Metal,
                palette: 1,
            },
        );
        t.push_palette(PaletteSlot {
            gpu: GpuHandle::NONE,
            bones: 32,
            joints: vec![PoseMm::default(); 32],
        });
        t.push_skinned(SkinnedInstance {
            blob: BlobId(*Hash::ZERO.as_bytes()),
            gpu: GpuHandle::NONE,
            pose: PoseMm::new(Mm(2), Mm(0), Mm(0), YawMd::ZERO),
            palette: 0,
            material: MaterialRef {
                tag: MaterialTag::Organic,
                palette: 0,
            },
        });
        t.push_probe(ProbeGrid {
            blob: BlobId(*Hash::ZERO.as_bytes()),
            origin: IVec3 { x: 0, y: 0, z: 0 },
            spacing_mm: 2000,
            dim: (4, 2, 4),
        });
        t.set_post(PostFlags::ADVENTURE);
        t.push_light(LightStub {
            pos: IVec3 {
                x: 0,
                y: 1000,
                z: 0,
            },
            kind: LightKind::Sun {
                dir: IVec3 { x: 1, y: 2, z: 1 },
                intensity_milli: 1000,
            },
        });
        let vis = t.extract(Epoch(1));
        assert_eq!(vis.tick, Tick(9));
        assert_eq!(vis.masked.len(), 1);
        assert_eq!(vis.masked_materials.len(), 1);
        assert_eq!(vis.skinned.len(), 1);
        assert_eq!(vis.palettes[0].bones, 32);
        assert_eq!(vis.probes.len(), 1);
        assert!(vis.post.gi);
        assert!(matches!(vis.lights[0].kind, LightKind::Sun { .. }));
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
        assert!(vis.decals.is_empty());
        assert!(vis.one_shots.is_empty());
    }

    #[test]
    fn empty_and_from_v2_default_vfx_empty() {
        let empty = VisualManifest::empty(Epoch::ZERO);
        assert!(empty.decals.is_empty());
        assert!(empty.one_shots.is_empty());
        assert_eq!(empty.post, PostFlags::UNLIT);

        let vis = VisualManifest::from_instances(Epoch::ZERO, [], [], []);
        assert!(vis.decals.is_empty());
        assert!(vis.one_shots.is_empty());
        assert_eq!(vis.post, PostFlags::UNLIT);

        let vis = VisualManifest::from_v2(
            Epoch::ZERO,
            Tick::ZERO,
            [],
            [],
            [],
            [],
            [],
            [],
            PostFlags::UNLIT,
            [],
        );
        assert!(vis.decals.is_empty());
        assert!(vis.one_shots.is_empty());
    }

    #[test]
    fn with_vfx_merges_without_sigils_or_particles() {
        let blob = BlobId(*Hash::ZERO.as_bytes());
        let pose = PoseMm::new(Mm(1), Mm(0), Mm(2), YawMd::ZERO);
        let material = MaterialRef {
            tag: MaterialTag::Stone,
            palette: 0,
        };
        let cluster_pose = PoseMm::new(Mm(9), Mm(0), Mm(9), YawMd::ZERO);
        let cluster_mat = MaterialRef {
            tag: MaterialTag::Metal,
            palette: 1,
        };
        let light = LightStub {
            pos: IVec3 {
                x: 0,
                y: 1000,
                z: 0,
            },
            kind: LightKind::Emissive,
        };
        let decal = Decal {
            blob,
            pose,
            material,
            born: Tick(1),
            ttl_ticks: 4,
        };
        let decal2 = Decal {
            blob,
            pose: PoseMm::new(Mm(3), Mm(0), Mm(4), YawMd::ZERO),
            material,
            born: Tick(2),
            ttl_ticks: 4,
        };
        let one = OneShotMesh {
            blob,
            pose,
            material,
            born: Tick(1),
            ttl_ticks: 4,
        };
        let vis = VisualManifest::from_v2(
            Epoch(2),
            Tick(7),
            [(blob, cluster_pose, cluster_mat)],
            [],
            [],
            [],
            [light],
            [],
            PostFlags::ADVENTURE,
            [],
        );
        assert_eq!(vis.clusters.len(), 1);
        assert!(vis.decals.is_empty());
        let vis = vis.with_vfx([decal], [one]).with_vfx([decal2], []);
        assert_eq!(vis.epoch, Epoch(2));
        assert_eq!(vis.tick, Tick(7));
        assert_eq!(vis.post, PostFlags::ADVENTURE);
        assert_eq!(vis.clusters.len(), 1);
        assert_eq!(vis.clusters[0].blob, blob);
        assert_eq!(vis.clusters[0].pose, cluster_pose);
        assert_eq!(vis.materials, vec![cluster_mat]);
        assert_eq!(vis.lights, vec![light]);
        assert_eq!(vis.decals, vec![decal, decal2]);
        assert_eq!(vis.one_shots, vec![one]);

        let Decal {
            blob: _,
            pose: _,
            material: _,
            born: _,
            ttl_ticks: _,
        } = vis.decals[0];
        let OneShotMesh {
            blob: _,
            pose: _,
            material: _,
            born: _,
            ttl_ticks: _,
        } = vis.one_shots[0];
        let VisualManifest {
            epoch: _,
            tick: _,
            clusters: _,
            materials: _,
            masked: _,
            masked_materials: _,
            skinned: _,
            palettes: _,
            lights: _,
            probes: _,
            post: _,
            debug_sigils: _,
            decals: _,
            one_shots: _,
        } = vis;
    }
}
