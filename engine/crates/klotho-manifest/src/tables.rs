//! Manifest SoA. **`pub(crate)`** — the compile firewall for K2.
//!
//! `klotho-render`, `klotho-audio`, `klotho-compile`, `klotho-vfx`, and
//! `klotho-cinematic` build public
//! Manifests from these columns. Gameplay crates must not name this module;
//! `scripts/ci/forbidden-imports.sh` greps for `klotho_manifest::tables`.

use klotho_core::{AabbMm, BlobId, Epoch, PoseMm, Sigil, Tick};

use crate::sonic::{BedRef, GrainVoice, SonicManifest};
use crate::ui::{UiManifest, Widget};
use crate::visual::{
    ClusterRef, Decal, GpuHandle, LightStub, MaterialRef, OneShotMesh, PaletteSlot, PostFlags,
    ProbeGrid, SkinnedInstance, VisualManifest,
};

/// Visual SoA. Extract copies into [`VisualManifest`] (AoS presenter buffer).
#[derive(Clone, Debug, Default)]
pub(crate) struct VisualTables {
    blobs: Vec<BlobId>,
    poses: Vec<PoseMm>,
    materials: Vec<MaterialRef>,
    masked: Vec<(BlobId, PoseMm, MaterialRef)>,
    skinned: Vec<SkinnedInstance>,
    palettes: Vec<PaletteSlot>,
    lights: Vec<LightStub>,
    probes: Vec<ProbeGrid>,
    post: PostFlags,
    debug: Vec<(Sigil, AabbMm)>,
    tick: Tick,
    decals: Vec<Decal>,
    one_shots: Vec<OneShotMesh>,
}

impl VisualTables {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, blob: BlobId, pose: PoseMm, material: MaterialRef) {
        self.blobs.push(blob);
        self.poses.push(pose);
        self.materials.push(material);
    }

    pub(crate) fn push_light(&mut self, light: LightStub) {
        self.lights.push(light);
    }

    pub(crate) fn push_debug(&mut self, s: Sigil, hull: AabbMm) {
        self.debug.push((s, hull));
    }

    pub(crate) fn push_masked(&mut self, blob: BlobId, pose: PoseMm, material: MaterialRef) {
        self.masked.push((blob, pose, material));
    }

    pub(crate) fn push_skinned(&mut self, inst: SkinnedInstance) {
        self.skinned.push(inst);
    }

    pub(crate) fn push_palette(&mut self, slot: PaletteSlot) {
        self.palettes.push(slot);
    }

    pub(crate) fn push_probe(&mut self, probe: ProbeGrid) {
        self.probes.push(probe);
    }

    pub(crate) fn set_post(&mut self, post: PostFlags) {
        self.post = post;
    }

    pub(crate) fn set_tick(&mut self, tick: Tick) {
        self.tick = tick;
    }

    pub(crate) fn push_decal(&mut self, decal: Decal) {
        self.decals.push(decal);
    }

    pub(crate) fn push_oneshot(&mut self, mesh: OneShotMesh) {
        self.one_shots.push(mesh);
    }

    pub(crate) fn extract(&self, epoch: Epoch) -> VisualManifest {
        let clusters = self
            .blobs
            .iter()
            .zip(self.poses.iter())
            .map(|(&blob, &pose)| ClusterRef {
                blob,
                gpu: GpuHandle::NONE,
                pose,
            })
            .collect();
        let masked = self
            .masked
            .iter()
            .map(|&(blob, pose, _)| ClusterRef {
                blob,
                gpu: GpuHandle::NONE,
                pose,
            })
            .collect();
        let masked_materials = self.masked.iter().map(|&(_, _, m)| m).collect();
        VisualManifest {
            epoch,
            tick: self.tick,
            clusters,
            materials: self.materials.clone(),
            masked,
            masked_materials,
            skinned: self.skinned.clone(),
            palettes: self.palettes.clone(),
            lights: self.lights.clone(),
            probes: self.probes.clone(),
            post: self.post,
            debug_sigils: self.debug.clone(),
            decals: self.decals.clone(),
            one_shots: self.one_shots.clone(),
        }
    }
}

/// Sonic SoA. One bed slot (v1).
#[derive(Clone, Debug, Default)]
pub(crate) struct SonicTables {
    grains: Vec<GrainVoice>,
    bed: Option<BedRef>,
}

impl SonicTables {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push_grain(&mut self, g: GrainVoice) {
        self.grains.push(g);
    }

    pub(crate) fn set_bed(&mut self, bed: BedRef) {
        self.bed = Some(bed);
    }

    pub(crate) fn extract(&self, epoch: Epoch) -> SonicManifest {
        SonicManifest {
            epoch,
            grains: self.grains.clone(),
            bed: self.bed,
        }
    }
}

/// UI SoA (draw-order column).
#[derive(Clone, Debug, Default)]
pub(crate) struct UiTables {
    widgets: Vec<Widget>,
}

impl UiTables {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, w: Widget) {
        self.widgets.push(w);
    }

    pub(crate) fn extract(&self, epoch: Epoch) -> UiManifest {
        UiManifest {
            epoch,
            widgets: self.widgets.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soa_starts_empty() {
        assert!(VisualTables::new().blobs.is_empty());
        assert!(VisualTables::new().decals.is_empty());
        assert!(VisualTables::new().one_shots.is_empty());
        assert!(SonicTables::new().bed.is_none());
        assert!(UiTables::new().widgets.is_empty());
    }
}
