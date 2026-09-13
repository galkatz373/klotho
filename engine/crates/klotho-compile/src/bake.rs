//! Deterministic probe / light bake (KAI-17).
//!
//! Integer milli-irradiance. No host floats in hashed bytes. Presentation
//! only: the baker never writes Projection or Trace.

use klotho_core::IVec3;
use klotho_manifest::{LightKind, LightStub, ProbeGrid};
use klotho_prove::blob_id_of;

use crate::error::CompileError;
use crate::header::{MAX_PROBE_CELLS, encode_probe_grid};

/// Default spacing matches [`klotho_render::DEFAULT_SPACING_MM`].
pub const BAKE_SPACING_MM: i32 = 2_000;

/// Bake a probe volume from punctual lights. Missing/empty lights yield sky.
pub fn bake_probes(
    origin: IVec3,
    spacing_mm: i32,
    dim: (u8, u8, u8),
    lights: &[LightStub],
) -> Result<(ProbeGrid, Vec<u8>), CompileError> {
    let cells = u32::from(dim.0)
        .saturating_mul(u32::from(dim.1))
        .saturating_mul(u32::from(dim.2));
    if spacing_mm <= 0 || cells == 0 || cells > MAX_PROBE_CELLS {
        return Err(CompileError::Header("probe bake dim".into()));
    }
    let mut samples = Vec::with_capacity(cells as usize * 3);
    for z in 0..dim.2 {
        for y in 0..dim.1 {
            for x in 0..dim.0 {
                let pos = IVec3 {
                    x: origin
                        .x
                        .saturating_add(i32::from(x).saturating_mul(spacing_mm)),
                    y: origin
                        .y
                        .saturating_add(i32::from(y).saturating_mul(spacing_mm)),
                    z: origin
                        .z
                        .saturating_add(i32::from(z).saturating_mul(spacing_mm)),
                };
                let rgb = sample_cell(pos, lights);
                samples.extend_from_slice(&rgb);
            }
        }
    }
    let bytes = encode_probe_grid(origin, spacing_mm, dim, &samples)?;
    let grid = ProbeGrid {
        blob: blob_id_of(&bytes),
        origin,
        spacing_mm,
        dim,
    };
    Ok((grid, bytes))
}

fn sample_cell(pos: IVec3, lights: &[LightStub]) -> [u16; 3] {
    // Constant sky so an empty bake is still a valid volume.
    let mut acc = [220u32, 280, 380];
    for light in lights {
        let LightKind::Point { intensity_milli } = light.kind else {
            continue;
        };
        let dx = i64::from(pos.x) - i64::from(light.pos.x);
        let dy = i64::from(pos.y) - i64::from(light.pos.y);
        let dz = i64::from(pos.z) - i64::from(light.pos.z);
        let dist2 = dx.saturating_mul(dx) + dy.saturating_mul(dy) + dz.saturating_mul(dz);
        // 1 / (1 + dist_m^2) in milli. dist is mm; divide by 1e6 for metres^2.
        let dist2_m_milli = (dist2 / 1_000_000).clamp(0, i64::from(i32::MAX)) as u32;
        let denom = 1_000u32.saturating_add(dist2_m_milli);
        let add = (u32::from(intensity_milli).saturating_mul(1_000)) / denom.max(1);
        acc[0] = acc[0].saturating_add(add);
        acc[1] = acc[1].saturating_add(add.saturating_mul(95) / 100);
        acc[2] = acc[2].saturating_add(add.saturating_mul(85) / 100);
    }
    [
        acc[0].min(1_000) as u16,
        acc[1].min(1_000) as u16,
        acc[2].min(1_000) as u16,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{decode_probe_grid, peek_kind, validate_blob};
    use klotho_prove::ArtifactKind;

    #[test]
    fn empty_lights_are_sky_and_stable() {
        let origin = IVec3 { x: 0, y: 0, z: 0 };
        let (a, bytes_a) = bake_probes(origin, BAKE_SPACING_MM, (2, 1, 2), &[]).unwrap();
        let (b, bytes_b) = bake_probes(origin, BAKE_SPACING_MM, (2, 1, 2), &[]).unwrap();
        assert_eq!(bytes_a, bytes_b);
        assert_eq!(a.blob, b.blob);
        assert_eq!(peek_kind(&bytes_a).unwrap(), ArtifactKind::ProbeGrid);
        validate_blob(&bytes_a).unwrap();
        let decoded = decode_probe_grid(&bytes_a).unwrap();
        assert_eq!(decoded.samples_milli.len(), 12);
        assert!(decoded.samples_milli.iter().all(|&s| s <= 1_000));
    }

    #[test]
    fn a_point_light_brightens_the_nearest_cell() {
        let origin = IVec3 { x: 0, y: 0, z: 0 };
        let light = LightStub {
            pos: IVec3 { x: 0, y: 0, z: 0 },
            kind: LightKind::Point {
                intensity_milli: 1_000,
            },
        };
        let (_, empty) = bake_probes(origin, BAKE_SPACING_MM, (2, 1, 1), &[]).unwrap();
        let (_, lit) = bake_probes(origin, BAKE_SPACING_MM, (2, 1, 1), &[light]).unwrap();
        let e = decode_probe_grid(&empty).unwrap();
        let l = decode_probe_grid(&lit).unwrap();
        assert!(l.samples_milli[0] >= e.samples_milli[0]);
        assert_ne!(empty, lit);
    }

    #[test]
    fn oversize_dim_fails_closed() {
        let origin = IVec3 { x: 0, y: 0, z: 0 };
        assert!(bake_probes(origin, BAKE_SPACING_MM, (16, 16, 16), &[]).is_err());
        assert!(bake_probes(origin, 0, (2, 2, 2), &[]).is_err());
    }
}
