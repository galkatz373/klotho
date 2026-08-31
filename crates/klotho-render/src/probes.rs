//! Probe-grid readiness. Missing blobs skip; they never panic the presenter.

use klotho_manifest::ProbeGrid;

/// Default Manifest spacing (millimetres).
pub const DEFAULT_SPACING_MM: i32 = 2000;
/// Era 2 cell-count cap (`dim` product).
pub const MAX_PROBE_CELLS: u32 = 8 * 4 * 8;

/// Spacing and dim are inside the Era 2 cap.
#[must_use]
pub fn probe_grid_usable(grid: &ProbeGrid) -> bool {
    if grid.spacing_mm <= 0 {
        return false;
    }
    let cells = u32::from(grid.dim.0)
        .saturating_mul(u32::from(grid.dim.1))
        .saturating_mul(u32::from(grid.dim.2));
    cells > 0 && cells <= MAX_PROBE_CELLS
}

/// `true` when the grid transform is usable and the CAS blob exists.
#[must_use]
pub fn probe_grid_ready(grid: &ProbeGrid, blob: Option<&[u8]>) -> bool {
    blob.is_some() && probe_grid_usable(grid)
}

/// Constant-sky sample, or `None` when the grid must be skipped.
#[must_use]
pub fn probe_sample(
    grid: &ProbeGrid,
    _world_mm: [i32; 3],
    blob: Option<&[u8]>,
) -> Option<[f32; 3]> {
    if !probe_grid_ready(grid, blob) {
        return None;
    }
    Some([0.22, 0.28, 0.38])
}

#[cfg(test)]
mod tests {
    use klotho_core::{BlobId, IVec3};

    use super::*;

    fn grid() -> ProbeGrid {
        ProbeGrid {
            blob: BlobId::ZERO,
            origin: IVec3 { x: 0, y: 0, z: 0 },
            spacing_mm: DEFAULT_SPACING_MM,
            dim: (8, 4, 8),
        }
    }

    #[test]
    fn missing_blob_is_not_ready() {
        let g = grid();
        assert!(!probe_grid_ready(&g, None));
        assert_eq!(probe_sample(&g, [0, 0, 0], None), None);
    }

    #[test]
    fn present_blob_is_ready() {
        let g = grid();
        let bytes: &[u8] = &[0];
        assert!(probe_grid_ready(&g, Some(bytes)));
        assert_eq!(
            probe_sample(&g, [0, 0, 0], Some(bytes)),
            Some([0.22, 0.28, 0.38])
        );
    }

    #[test]
    fn oversize_dim_is_skipped() {
        let mut g = grid();
        g.dim = (16, 16, 16);
        assert!(!probe_grid_ready(&g, Some(&[0])));
        assert_eq!(probe_sample(&g, [0, 0, 0], Some(&[0])), None);
    }
}
