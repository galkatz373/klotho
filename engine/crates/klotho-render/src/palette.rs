//! Closed material → albedo and metalness/roughness. Unlit uses albedo only.

use klotho_manifest::{MaterialRef, MaterialTag};

/// Linear RGB in 0..=1. Emissive is the unlit permutation in the lambert shader.
#[must_use]
pub fn albedo(mat: MaterialRef) -> [f32; 4] {
    let rgb = match mat.tag {
        MaterialTag::Organic => [0.55, 0.38, 0.22],
        MaterialTag::Metal => [0.62, 0.64, 0.68],
        MaterialTag::Stone => [0.45, 0.44, 0.42],
        MaterialTag::Cloth => [0.42, 0.22, 0.22],
        MaterialTag::Emissive => [1.0, 0.55, 0.15],
        MaterialTag::Water => [0.18, 0.35, 0.55],
    };
    // palette 0 = style default; others nudge saturation slightly
    let n = (mat.palette as f32) * 0.02;
    [
        (rgb[0] + n).clamp(0.0, 1.0),
        (rgb[1] + n).clamp(0.0, 1.0),
        (rgb[2] + n).clamp(0.0, 1.0),
        1.0,
    ]
}

/// Metalness / roughness constants. Emissive skips lighting in the PBR path.
#[must_use]
pub fn metalness_roughness(tag: MaterialTag) -> (f32, f32) {
    match tag {
        MaterialTag::Organic => (0.0, 0.8),
        MaterialTag::Metal => (1.0, 0.35),
        MaterialTag::Stone => (0.0, 0.7),
        MaterialTag::Cloth => (0.0, 0.9),
        MaterialTag::Emissive => (0.0, 1.0),
        MaterialTag::Water => (0.0, 0.05),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_set_has_distinct_albedos() {
        let a = albedo(MaterialRef {
            tag: MaterialTag::Organic,
            palette: 0,
        });
        let b = albedo(MaterialRef {
            tag: MaterialTag::Metal,
            palette: 0,
        });
        assert_ne!(a, b);
        assert_eq!(a[3], 1.0);
    }

    #[test]
    fn metal_and_stone_pbr_params_differ() {
        let metal = metalness_roughness(MaterialTag::Metal);
        let stone = metalness_roughness(MaterialTag::Stone);
        assert_ne!(metal, stone);
        assert_eq!(metal.0, 1.0);
        assert_eq!(stone.0, 0.0);
        assert!(metal.1 < stone.1);
    }
}
