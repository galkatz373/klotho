//! Vendored integer overlap (HLD A5). AABB + swept capsule. No SIMD, no f32.

use klotho_core::{AabbMm, IVec3};

/// Closed AABB overlap, including shared faces.
#[must_use]
pub const fn aabb_overlaps(a: AabbMm, b: AabbMm) -> bool {
    a.intersects(b)
}

/// Conservative swept volume: union of start and end AABBs.
#[must_use]
pub const fn swept_aabb(from: AabbMm, to: AabbMm) -> AabbMm {
    from.swept_union(to)
}

/// Vertical capsule: XZ circle + Y slab. Millimetres.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct CapsuleMm {
    /// Centre of the Y-slab (x, y, z).
    pub centre: IVec3,
    /// XZ radius, millimetres.
    pub radius: i32,
    /// Half-height of the Y slab, millimetres.
    pub half_h: i32,
}

impl CapsuleMm {
    /// Capsule that covers `world` AABB (XZ circumradius, Y slab).
    #[must_use]
    pub const fn covering_aabb(world: AabbMm) -> Self {
        let cx = mid_i32(world.min.x, world.max.x);
        let cy = mid_i32(world.min.y, world.max.y);
        let cz = mid_i32(world.min.z, world.max.z);
        let hx = half_extent(world.min.x, world.max.x);
        let hz = half_extent(world.min.z, world.max.z);
        let hy = half_extent(world.min.y, world.max.y);
        let radius = if hx > hz { hx } else { hz };
        Self {
            centre: IVec3 {
                x: cx,
                y: cy,
                z: cz,
            },
            radius,
            half_h: hy,
        }
    }
}

const fn mid_i32(a: i32, b: i32) -> i32 {
    (a as i64 + b as i64).div_euclid(2) as i32
}

const fn half_extent(min: i32, max: i32) -> i32 {
    let d = (max as i64).saturating_sub(min as i64);
    (d.div_euclid(2)) as i32
}

/// Capsule vs AABB. Y slabs must overlap; XZ is circle vs rectangle.
#[must_use]
pub const fn capsule_overlaps_aabb(c: CapsuleMm, b: AabbMm) -> bool {
    if b.is_empty() || c.radius < 0 || c.half_h < 0 {
        return false;
    }
    let y0 = c.centre.y.wrapping_sub(c.half_h);
    let y1 = c.centre.y.wrapping_add(c.half_h);
    if y1 < b.min.y || b.max.y < y0 {
        return false;
    }
    let nx = clamp_i32(c.centre.x, b.min.x, b.max.x);
    let nz = clamp_i32(c.centre.z, b.min.z, b.max.z);
    let dx = (c.centre.x as i64).saturating_sub(nx as i64);
    let dz = (c.centre.z as i64).saturating_sub(nz as i64);
    let r = c.radius as i64;
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz)) <= r.saturating_mul(r)
}

const fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_touch_is_overlap() {
        let a = AabbMm::new(
            IVec3 { x: 0, y: 0, z: 0 },
            IVec3 {
                x: 10,
                y: 10,
                z: 10,
            },
        );
        let b = AabbMm::new(
            IVec3 { x: 10, y: 0, z: 0 },
            IVec3 {
                x: 20,
                y: 10,
                z: 10,
            },
        );
        assert!(aabb_overlaps(a, b));
    }

    #[test]
    fn capsule_covers_its_aabb() {
        let b = AabbMm::new(
            IVec3 {
                x: -200,
                y: 0,
                z: -200,
            },
            IVec3 {
                x: 200,
                y: 1800,
                z: 200,
            },
        );
        let c = CapsuleMm::covering_aabb(b);
        assert!(capsule_overlaps_aabb(c, b));
    }

    #[test]
    fn capsule_misses_distant_box() {
        let c = CapsuleMm {
            centre: IVec3 { x: 0, y: 900, z: 0 },
            radius: 200,
            half_h: 900,
        };
        let b = AabbMm::new(
            IVec3 {
                x: 5000,
                y: 0,
                z: 5000,
            },
            IVec3 {
                x: 5100,
                y: 100,
                z: 5100,
            },
        );
        assert!(!capsule_overlaps_aabb(c, b));
    }
}
