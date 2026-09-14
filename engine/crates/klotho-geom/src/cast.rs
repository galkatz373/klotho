//! Ray and conservative shape casts.

use klotho_core::{AabbMm, IVec3, PoseMm, frac_cmp};

use crate::query::{bounds, contact, posed_point};
use crate::shape::{GeomError, Shape};
use crate::witness::CONTACT_SLOP_MM;

/// Result of a start→end shape cast against one obstacle.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct SweptHit {
    /// Conservative enter fraction numerator.
    pub enter_n: i64,
    /// Conservative enter fraction denominator (`> 0`).
    pub enter_d: i64,
    /// Penetration at the start pose. Zero if separated or touching.
    pub start_depth_mm: i32,
    /// Penetration at the end pose.
    pub end_depth_mm: i32,
    /// Start was free and the conservative Minkowski cast entered the obstacle.
    pub crossing: bool,
}

/// First hit of the closed segment `origin → origin+dir` against `shape` at `pose`.
pub fn raycast(
    shape: Shape,
    pose: PoseMm,
    origin: IVec3,
    dir: IVec3,
) -> Result<Option<(i64, i64)>, GeomError> {
    match shape {
        Shape::OrientedBox { local } => Ok(ray_obb(local, pose, origin, dir)),
        Shape::Sphere {
            local_center,
            radius_mm,
        } => Ok(ray_sphere(
            posed_point(local_center, pose),
            radius_mm,
            origin,
            dir,
        )),
        Shape::Capsule { .. } => {
            let b = bounds(shape, pose)?;
            Ok(b.segment_hit(origin, dir))
        }
    }
}

/// Conservative translation cast of `mover` from `start` to `end` against one obstacle.
pub fn swept_against(
    mover: Shape,
    start: PoseMm,
    end: PoseMm,
    obstacle: Shape,
    obstacle_pose: PoseMm,
) -> Result<SweptHit, GeomError> {
    let start_depth = contact(mover, start, obstacle, obstacle_pose)?.map_or(0, |c| c.depth_mm);
    let end_depth = contact(mover, end, obstacle, obstacle_pose)?.map_or(0, |c| c.depth_mm);
    let a0 = bounds(mover, start)?;
    let a1 = bounds(mover, end)?;
    let swept = a0.swept_union(a1);
    let occ = bounds(obstacle, obstacle_pose)?;
    if !swept.intersects(occ) {
        return Ok(SweptHit {
            enter_n: 0,
            enter_d: 1,
            start_depth_mm: start_depth,
            end_depth_mm: end_depth,
            crossing: false,
        });
    }
    let expanded = minkowski_sum(occ, a0);
    let origin = aabb_center(a0);
    let dir = aabb_center(a1).wrapping_sub(origin);
    let (enter_n, enter_d, exit_n, exit_d, origin_inside) =
        match slab_interval(expanded, origin, dir) {
            None => {
                return Ok(SweptHit {
                    enter_n: 0,
                    enter_d: 1,
                    start_depth_mm: start_depth,
                    end_depth_mm: end_depth,
                    crossing: false,
                });
            }
            Some(v) => v,
        };
    let start_free = start_depth == 0 && !a0.intersects(occ);
    let went_through = origin_inside
        || (frac_cmp(enter_n, enter_d, 0, 1) != core::cmp::Ordering::Less
            && frac_cmp(enter_n, enter_d, 1, 1) != core::cmp::Ordering::Greater);
    let exited_before_end = frac_cmp(exit_n, exit_d, 1, 1) == core::cmp::Ordering::Less;
    let end_free = end_depth == 0 && !a1.intersects(occ);
    let crossing = start_free
        && went_through
        && (end_depth > CONTACT_SLOP_MM
            || (exited_before_end && end_free)
            || end_depth > start_depth);
    Ok(SweptHit {
        enter_n,
        enter_d,
        start_depth_mm: start_depth,
        end_depth_mm: end_depth,
        crossing,
    })
}

fn ray_obb(local: AabbMm, pose: PoseMm, origin: IVec3, dir: IVec3) -> Option<(i64, i64)> {
    if pose.yaw.0 == 0 && pose.pitch.0 == 0 && pose.roll.0 == 0 {
        let world = AabbMm::new(
            local.min.wrapping_add(pose.translation()),
            local.max.wrapping_add(pose.translation()),
        );
        return world.segment_hit(origin, dir);
    }
    bounds_hit(local, pose, origin, dir)
}

fn bounds_hit(local: AabbMm, pose: PoseMm, origin: IVec3, dir: IVec3) -> Option<(i64, i64)> {
    crate::query::bounds(Shape::OrientedBox { local }, pose)
        .ok()
        .and_then(|b| b.segment_hit(origin, dir))
}

fn ray_sphere(center: IVec3, radius: i32, origin: IVec3, dir: IVec3) -> Option<(i64, i64)> {
    if radius < 0 {
        return None;
    }
    if dir == IVec3::ZERO {
        let d = origin.wrapping_sub(center);
        let dist2 = i64::from(d.x) * i64::from(d.x)
            + i64::from(d.y) * i64::from(d.y)
            + i64::from(d.z) * i64::from(d.z);
        return (dist2 <= i64::from(radius) * i64::from(radius)).then_some((0, 1));
    }
    // Conservative: ray vs AABB of the sphere.
    AabbMm::new(
        IVec3 {
            x: center.x.saturating_sub(radius),
            y: center.y.saturating_sub(radius),
            z: center.z.saturating_sub(radius),
        },
        IVec3 {
            x: center.x.saturating_add(radius),
            y: center.y.saturating_add(radius),
            z: center.z.saturating_add(radius),
        },
    )
    .segment_hit(origin, dir)
}

fn aabb_center(a: AabbMm) -> IVec3 {
    IVec3 {
        x: ((i64::from(a.min.x) + i64::from(a.max.x)) / 2) as i32,
        y: ((i64::from(a.min.y) + i64::from(a.max.y)) / 2) as i32,
        z: ((i64::from(a.min.z) + i64::from(a.max.z)) / 2) as i32,
    }
}

fn minkowski_sum(obstacle: AabbMm, mover: AabbMm) -> AabbMm {
    let hx = half_len(mover.min.x, mover.max.x);
    let hy = half_len(mover.min.y, mover.max.y);
    let hz = half_len(mover.min.z, mover.max.z);
    AabbMm::new(
        IVec3 {
            x: obstacle.min.x.saturating_sub(hx),
            y: obstacle.min.y.saturating_sub(hy),
            z: obstacle.min.z.saturating_sub(hz),
        },
        IVec3 {
            x: obstacle.max.x.saturating_add(hx),
            y: obstacle.max.y.saturating_add(hy),
            z: obstacle.max.z.saturating_add(hz),
        },
    )
}

fn half_len(min: i32, max: i32) -> i32 {
    ((i64::from(max) - i64::from(min)).div_euclid(2)).unsigned_abs() as i32
}

/// Slab enter/exit of `origin → origin+dir` against a closed AABB.
/// Returns `(tmin_n, tmin_d, tmax_n, tmax_d, origin_inside)`.
fn slab_interval(aabb: AabbMm, origin: IVec3, dir: IVec3) -> Option<(i64, i64, i64, i64, bool)> {
    if aabb.is_empty() {
        return None;
    }
    let inside = aabb.contains_point(origin);
    if dir == IVec3::ZERO {
        return inside.then_some((0, 1, 1, 1, true));
    }
    let mut tmin_n = 0i64;
    let mut tmin_d = 1i64;
    let mut tmax_n = 1i64;
    let mut tmax_d = 1i64;
    if !clip(
        origin.x,
        dir.x,
        aabb.min.x,
        aabb.max.x,
        &mut tmin_n,
        &mut tmin_d,
        &mut tmax_n,
        &mut tmax_d,
    ) || !clip(
        origin.y,
        dir.y,
        aabb.min.y,
        aabb.max.y,
        &mut tmin_n,
        &mut tmin_d,
        &mut tmax_n,
        &mut tmax_d,
    ) || !clip(
        origin.z,
        dir.z,
        aabb.min.z,
        aabb.max.z,
        &mut tmin_n,
        &mut tmin_d,
        &mut tmax_n,
        &mut tmax_d,
    ) {
        return None;
    }
    if frac_cmp(tmin_n, tmin_d, tmax_n, tmax_d) == core::cmp::Ordering::Greater {
        return None;
    }
    if frac_cmp(tmax_n, tmax_d, 0, 1) == core::cmp::Ordering::Less {
        return None;
    }
    if frac_cmp(tmin_n, tmin_d, 1, 1) == core::cmp::Ordering::Greater {
        return None;
    }
    if frac_cmp(tmin_n, tmin_d, 0, 1) == core::cmp::Ordering::Less {
        tmin_n = 0;
        tmin_d = 1;
    }
    Some((tmin_n, tmin_d, tmax_n, tmax_d, inside))
}

#[allow(clippy::too_many_arguments)]
fn clip(
    origin: i32,
    dir: i32,
    min: i32,
    max: i32,
    tmin_n: &mut i64,
    tmin_d: &mut i64,
    tmax_n: &mut i64,
    tmax_d: &mut i64,
) -> bool {
    if dir == 0 {
        return origin >= min && origin <= max;
    }
    let (enter_b, exit_b) = if dir > 0 { (min, max) } else { (max, min) };
    let (en, ed) = pos_den(i64::from(enter_b) - i64::from(origin), i64::from(dir));
    let (xn, xd) = pos_den(i64::from(exit_b) - i64::from(origin), i64::from(dir));
    if frac_cmp(en, ed, *tmin_n, *tmin_d) == core::cmp::Ordering::Greater {
        *tmin_n = en;
        *tmin_d = ed;
    }
    if frac_cmp(xn, xd, *tmax_n, *tmax_d) == core::cmp::Ordering::Less {
        *tmax_n = xn;
        *tmax_d = xd;
    }
    true
}

fn pos_den(n: i64, d: i64) -> (i64, i64) {
    if d < 0 { (-n, -d) } else { (n, d) }
}
