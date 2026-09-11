//! CPU clustered-forward tile lists. Uniform-sized so Metal downlevel works.

use klotho_manifest::{LightKind, VisualManifest};

/// Punctual lights packed into the PBR uniform.
pub const MAX_POINT_LIGHTS: usize = 32;
/// Nominal tile size in pixels before the grid cap.
pub const TILE_SIZE_PX: u32 = 16;
/// Tile columns stored in the mask uniform.
pub const MAX_TILES_X: u32 = 32;
/// Tile rows stored in the mask uniform.
pub const MAX_TILES_Y: u32 = 18;

/// GPU point light (metres, linear).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PointLight {
    /// World position, metres.
    pub pos: [f32; 3],
    /// Influence radius, metres.
    pub radius: f32,
    /// Linear RGB.
    pub color: [f32; 3],
    /// Intensity (1.0 = `intensity_milli` 1000).
    pub intensity: f32,
}

/// Per-tile light bitmasks (`bit i` = light `i` overlaps that tile).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileAssign {
    /// Columns (1..=[`MAX_TILES_X`]).
    pub tiles_x: u32,
    /// Rows (1..=[`MAX_TILES_Y`]).
    pub tiles_y: u32,
    /// `tiles_x * tiles_y` masks; extras up to the uniform cap stay 0.
    pub masks: Vec<u32>,
}

impl TileAssign {
    /// Mask at tile `(x, y)`, or 0 if out of range.
    #[must_use]
    pub fn mask(&self, x: u32, y: u32) -> u32 {
        if x >= self.tiles_x || y >= self.tiles_y {
            return 0;
        }
        self.masks[(y * self.tiles_x + x) as usize]
    }
}

/// First [`MAX_POINT_LIGHTS`] `Point` stubs. Overflow is dropped.
#[must_use]
pub fn collect_point_lights(vis: &VisualManifest) -> Vec<PointLight> {
    let mut out = Vec::new();
    for l in &vis.lights {
        if out.len() >= MAX_POINT_LIGHTS {
            break;
        }
        let LightKind::Point { intensity_milli } = l.kind else {
            continue;
        };
        let intensity = f32::from(intensity_milli) / 1000.0;
        let radius = (2.0 + 6.0 * intensity).clamp(1.0, 16.0);
        out.push(PointLight {
            pos: [
                l.pos.x as f32 / 1000.0,
                l.pos.y as f32 / 1000.0,
                l.pos.z as f32 / 1000.0,
            ],
            radius,
            color: [1.0, 0.95, 0.85],
            intensity,
        });
    }
    out
}

/// Assign lights to screen tiles. Lights past [`MAX_POINT_LIGHTS`] are ignored.
#[must_use]
pub fn assign_tiles(
    lights: &[PointLight],
    view_proj: &[f32; 16],
    width: u32,
    height: u32,
) -> TileAssign {
    let width = width.max(1);
    let height = height.max(1);
    let tiles_x = width.div_ceil(TILE_SIZE_PX).clamp(1, MAX_TILES_X);
    let tiles_y = height.div_ceil(TILE_SIZE_PX).clamp(1, MAX_TILES_Y);
    let n = (tiles_x * tiles_y) as usize;
    let mut masks = vec![0u32; n];
    let tile_w = width as f32 / tiles_x as f32;
    let tile_h = height as f32 / tiles_y as f32;
    for (i, light) in lights.iter().take(MAX_POINT_LIGHTS).enumerate() {
        let Some((sx, sy, r_px)) =
            project_sphere_screen(view_proj, light.pos, light.radius, width, height)
        else {
            continue;
        };
        let bit = 1u32 << i;
        for ty in 0..tiles_y {
            let y0 = ty as f32 * tile_h;
            let y1 = y0 + tile_h;
            for tx in 0..tiles_x {
                let x0 = tx as f32 * tile_w;
                let x1 = x0 + tile_w;
                if circle_intersects_rect(sx, sy, r_px, x0, y0, x1, y1) {
                    masks[(ty * tiles_x + tx) as usize] |= bit;
                }
            }
        }
    }
    TileAssign {
        tiles_x,
        tiles_y,
        masks,
    }
}

fn project(vp: &[f32; 16], p: [f32; 3]) -> Option<(f32, f32, f32)> {
    let x = vp[0] * p[0] + vp[4] * p[1] + vp[8] * p[2] + vp[12];
    let y = vp[1] * p[0] + vp[5] * p[1] + vp[9] * p[2] + vp[13];
    let z = vp[2] * p[0] + vp[6] * p[1] + vp[10] * p[2] + vp[14];
    let w = vp[3] * p[0] + vp[7] * p[1] + vp[11] * p[2] + vp[15];
    let _ = z;
    if w.abs() < 1e-6 {
        return None;
    }
    Some((x / w, y / w, w))
}

fn circle_intersects_rect(cx: f32, cy: f32, r: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    let nx = cx.clamp(x0, x1);
    let ny = cy.clamp(y0, y1);
    let dx = cx - nx;
    let dy = cy - ny;
    dx * dx + dy * dy <= r * r
}

/// Project a world-space sphere onto the framebuffer (pixels, y-down).
#[must_use]
pub fn project_sphere_screen(
    vp: &[f32; 16],
    pos: [f32; 3],
    radius: f32,
    width: u32,
    height: u32,
) -> Option<(f32, f32, f32)> {
    let (ndc_x, ndc_y, w) = project(vp, pos)?;
    if w <= 0.0 {
        return None;
    }
    let sx = (ndc_x * 0.5 + 0.5) * width as f32;
    let sy = (0.5 - ndc_y * 0.5) * height as f32;
    let r_px = (radius / w) * 0.5 * width as f32;
    Some((sx, sy, r_px.max(0.5)))
}

#[cfg(test)]
mod tests {
    use klotho_core::{Epoch, IVec3};
    use klotho_manifest::{LightKind, LightStub, VisualManifest};

    use super::*;

    fn identity() -> [f32; 16] {
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m
    }

    fn light_at(pos: [f32; 3], radius: f32) -> PointLight {
        PointLight {
            pos,
            radius,
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
        }
    }

    #[test]
    fn inside_tile_is_set() {
        let vp = identity();
        let lights = [light_at([0.0, 0.0, 0.0], 0.2)];
        let tiles = assign_tiles(&lights, &vp, 64, 64);
        assert_eq!(tiles.tiles_x, 4);
        assert_eq!(tiles.tiles_y, 4);
        // origin → NDC 0,0 → pixel (32, 32) → tile (2, 2)
        assert_ne!(tiles.mask(2, 2) & 1, 0);
    }

    #[test]
    fn outside_tile_is_clear() {
        let vp = identity();
        let lights = [light_at([0.0, 0.0, 0.0], 0.05)];
        let tiles = assign_tiles(&lights, &vp, 64, 64);
        assert_eq!(tiles.mask(0, 0) & 1, 0);
        assert_eq!(tiles.mask(3, 0) & 1, 0);
    }

    #[test]
    fn overflow_drops_extras() {
        let vp = identity();
        let lights: Vec<_> = (0..33).map(|_| light_at([0.0, 0.0, 0.0], 1.0)).collect();
        let tiles = assign_tiles(&lights, &vp, 64, 64);
        assert_eq!(tiles.mask(2, 2), u32::MAX);
        assert_eq!(lights.len(), 33);
    }

    #[test]
    fn collect_caps_at_max() {
        let lights: Vec<_> = (0..40)
            .map(|i| LightStub {
                pos: IVec3 {
                    x: i * 100,
                    y: 1000,
                    z: 0,
                },
                kind: LightKind::Point {
                    intensity_milli: 1000,
                },
            })
            .collect();
        let vis = VisualManifest::from_instances(Epoch::ZERO, [], lights, []);
        assert_eq!(collect_point_lights(&vis).len(), MAX_POINT_LIGHTS);
    }
}
