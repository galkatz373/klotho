//! [`Presenter`]: pure function of VisualManifest + Observer + GpuBudget.

use klotho_manifest::{GpuBudget, Observer, VisualManifest};

use crate::math::dist2_xz;

/// wgpu / null / future neural presenters share this trait (HLD §5).
pub trait Presenter: Send {
    /// Draw `vis`. GPU handles are presenter-owned. Sim thread must not wait.
    fn present(&mut self, vis: &VisualManifest, observer: Observer, budget: GpuBudget);
}

/// Records presents. Used on the render thread in tests (no GPU).
#[derive(Clone, Debug, Default)]
pub struct NullPresenter {
    /// How many times [`Presenter::present`] ran.
    pub frames: u32,
    /// Clusters drawn after budget clamp on the last frame.
    pub last_drawn: u16,
    /// Last observer yaw millidegrees.
    pub last_yaw: i32,
}

impl Presenter for NullPresenter {
    fn present(&mut self, vis: &VisualManifest, observer: Observer, budget: GpuBudget) {
        self.frames = self.frames.saturating_add(1);
        self.last_yaw = observer.eye.yaw.0;
        self.last_drawn = draw_list(vis, observer, budget).len() as u16;
    }
}

/// Indices into `vis.clusters` after `max_clusters` drop-farthest.
#[must_use]
pub fn draw_list(vis: &VisualManifest, observer: Observer, budget: GpuBudget) -> Vec<usize> {
    let cap = budget.max_clusters as usize;
    if vis.clusters.len() <= cap {
        return (0..vis.clusters.len()).collect();
    }
    let mut order: Vec<usize> = (0..vis.clusters.len()).collect();
    order.sort_by_key(|&i| dist2_xz(vis.clusters[i].pose, observer));
    order.truncate(cap);
    order
}

#[cfg(test)]
mod tests {
    use klotho_core::{BlobId, Epoch, Mm, PoseMm, YawMd};
    use klotho_manifest::{MaterialRef, MaterialTag, Observer};

    use super::*;

    fn blob(n: u8) -> BlobId {
        let mut b = [0u8; 32];
        b[0] = n;
        BlobId::from_bytes(b)
    }

    #[test]
    fn budget_drops_farthest() {
        let vis = VisualManifest::from_instances(
            Epoch::ZERO,
            [
                (
                    blob(1),
                    PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
                    MaterialRef {
                        tag: MaterialTag::Stone,
                        palette: 0,
                    },
                ),
                (
                    blob(2),
                    PoseMm::new(Mm(10_000), Mm(0), Mm(0), YawMd::ZERO),
                    MaterialRef {
                        tag: MaterialTag::Stone,
                        palette: 0,
                    },
                ),
            ],
            [],
            [],
        );
        let mut budget = GpuBudget::HEARTH;
        budget.max_clusters = 1;
        let drawn = draw_list(&vis, Observer::origin(), budget);
        assert_eq!(drawn, vec![0]);
    }
}
