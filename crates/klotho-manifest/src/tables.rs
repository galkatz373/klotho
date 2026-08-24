//! Manifest SoA. **`pub(crate)`** — the compile firewall for K2.
//!
//! `klotho-render`, `klotho-audio`, and `klotho-compile` build public
//! Manifests from these columns. Gameplay crates must not name this module;
//! `scripts/ci/forbidden-imports.sh` greps for `klotho_manifest::tables`.

use klotho_core::{AabbMm, BlobId, Epoch, PoseMm, Sigil};

use crate::sonic::{BedRef, GrainVoice, SonicManifest};
use crate::ui::{UiManifest, Widget};
use crate::visual::{ClusterRef, GpuHandle, LightStub, MaterialRef, VisualManifest};

/// Visual SoA. Extract copies into [`VisualManifest`] (AoS presenter buffer).
#[derive(Clone, Debug, Default)]
pub(crate) struct VisualTables {
    blobs: Vec<BlobId>,
    poses: Vec<PoseMm>,
    materials: Vec<MaterialRef>,
    lights: Vec<LightStub>,
    debug: Vec<(Sigil, AabbMm)>,
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
        VisualManifest {
            epoch,
            clusters,
            materials: self.materials.clone(),
            lights: self.lights.clone(),
            debug_sigils: self.debug.clone(),
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
        assert!(SonicTables::new().bed.is_none());
        assert!(UiTables::new().widgets.is_empty());
    }
}
