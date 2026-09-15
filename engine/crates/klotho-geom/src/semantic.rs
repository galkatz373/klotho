//! Bounded inward sampling of semantic swept capsules. Broad-phase casts never
//! establish a gameplay hit. An integer sphere inside the swept capsule does.
use crate::{GeomError, Shape, contact};
use klotho_core::{IVec3, PoseMm};

/// Maximum narrow-phase samples per channel/target interval.
pub const MAX_SWEEP_SAMPLES: u32 = 65_536;
/// Maximum spacing along time and the capsule segment, millimetres per axis.
pub const SWEEP_SPACING_MM: i64 = 5;
pub use klotho_core::SweepSample;
fn distance(a: IVec3, b: IVec3) -> i64 {
    [
        i64::from(a.x) - i64::from(b.x),
        i64::from(a.y) - i64::from(b.y),
        i64::from(a.z) - i64::from(b.z),
    ]
    .into_iter()
    .map(i64::abs)
    .max()
    .unwrap()
}
fn lerp(a: IVec3, b: IVec3, n: u32, d: u32) -> IVec3 {
    let f = |a: i32, b: i32| {
        (i64::from(a) + (i64::from(b) - i64::from(a)) * i64::from(n) / i64::from(d)) as i32
    };
    IVec3 {
        x: f(a.x, b.x),
        y: f(a.y, b.y),
        z: f(a.z, b.z),
    }
}
fn counts(ends: [[IVec3; 2]; 2]) -> Result<(u32, u32), GeomError> {
    let steps = |d: i64| ((d + SWEEP_SPACING_MM - 1) / SWEEP_SPACING_MM).max(1) as u32;
    let time = steps(distance(ends[0][0], ends[1][0]).max(distance(ends[0][1], ends[1][1])));
    let segment = steps(distance(ends[0][0], ends[0][1]).max(distance(ends[1][0], ends[1][1])));
    if time > u16::MAX as u32
        || segment > u16::MAX as u32
        || (time + 1)
            .checked_mul(segment + 1)
            .is_none_or(|n| n > MAX_SWEEP_SAMPLES)
    {
        return Err(GeomError::Malformed);
    }
    Ok((time, segment))
}
/// Maximum number of narrow-phase samples charged for an interval.
pub fn semantic_sample_count(ends: [[IVec3; 2]; 2]) -> Result<u32, GeomError> {
    let (time, segment) = counts(ends)?;
    Ok((time + 1) * (segment + 1))
}
fn sphere(
    ends: [[IVec3; 2]; 2],
    radius: i32,
    sample: SweepSample,
    time: u32,
    segment: u32,
) -> Result<Option<Shape>, GeomError> {
    // Two integer lerps can shift the centre by less than 4 mm Euclidean.
    // Shrinking by 4 mm keeps the tested sphere inside the exact swept volume.
    if radius <= 0 || u32::from(sample.time) > time || u32::from(sample.segment) > segment {
        return Err(GeomError::Malformed);
    }
    let a = lerp(ends[0][0], ends[1][0], sample.time.into(), time);
    let b = lerp(ends[0][1], ends[1][1], sample.time.into(), time);
    let exact = |a: IVec3, b: IVec3, n: u32, d: u32| {
        [
            i64::from(b.x) - i64::from(a.x),
            i64::from(b.y) - i64::from(a.y),
            i64::from(b.z) - i64::from(a.z),
        ]
        .into_iter()
        .all(|v| v * i64::from(n) % i64::from(d) == 0)
    };
    let shrink = if exact(ends[0][0], ends[1][0], sample.time.into(), time)
        && exact(ends[0][1], ends[1][1], sample.time.into(), time)
        && exact(a, b, sample.segment.into(), segment)
    {
        0
    } else {
        4
    };
    if radius <= shrink {
        return Ok(None);
    }
    Ok(Some(Shape::sphere(
        lerp(a, b, sample.segment.into(), segment),
        radius - shrink,
    )?))
}
/// Find the first actual narrow-phase intersection, in canonical sample order.
/// Small grazing contacts can miss within the declared inward envelope; query
/// overflow refuses the entire interval rather than trusting an AABB.
pub fn semantic_sweep(
    ends: [[IVec3; 2]; 2],
    radius: i32,
    target: Shape,
    pose: PoseMm,
) -> Result<Option<SweepSample>, GeomError> {
    let (time, segment) = counts(ends)?;
    for t in 0..=time {
        for s in 0..=segment {
            let sample = SweepSample {
                time: t as u16,
                segment: s as u16,
            };
            if let Some(ball) = sphere(ends, radius, sample, time, segment)? {
                if intersects(ball, target, pose)? {
                    return Ok(Some(sample));
                }
            }
        }
    }
    Ok(None)
}
/// Independently reproduce the claimed narrow-phase sample.
pub fn verify_semantic_sweep(
    ends: [[IVec3; 2]; 2],
    radius: i32,
    target: Shape,
    pose: PoseMm,
    sample: SweepSample,
) -> Result<bool, GeomError> {
    let (time, segment) = counts(ends)?;
    let Some(ball) = sphere(ends, radius, sample, time, segment)? else {
        return Ok(false);
    };
    intersects(ball, target, pose)
}

// Exact integer ball/convex query. Face projections and segment distances use
// rational inequalities; no rounded nearest point or AABB can establish a hit.
fn ball_convex(p: IVec3, radius: i32, vertices: &[IVec3]) -> bool {
    type V = [i128; 3];
    let sub = |a: IVec3, b: IVec3| -> V {
        [
            i128::from(a.x) - i128::from(b.x),
            i128::from(a.y) - i128::from(b.y),
            i128::from(a.z) - i128::from(b.z),
        ]
    };
    let dot = |a: V, b: V| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: V, b: V| -> V {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let r2 = i128::from(radius).pow(2);
    let mut inside = true;
    let mut faces = 0;
    for i in 0..vertices.len() {
        for j in i + 1..vertices.len() {
            for k in j + 1..vertices.len() {
                let a = vertices[i];
                let b = vertices[j];
                let c = vertices[k];
                let mut n = cross(sub(b, a), sub(c, a));
                let n2 = dot(n, n);
                if n2 == 0 {
                    continue;
                }
                let mut positive = false;
                let mut negative = false;
                for &v in vertices {
                    let d = dot(sub(v, a), n);
                    positive |= d > 0;
                    negative |= d < 0;
                }
                if positive && negative || !positive && !negative {
                    continue;
                }
                if positive {
                    n = n.map(|v| -v)
                }
                faces += 1;
                let plane = dot(sub(p, a), n);
                inside &= plane <= 0;
                // Projection onto this face lies within the triangle exactly when all
                // three oriented edge predicates have the same sign.
                let signs = [
                    dot(cross(sub(b, a), sub(p, a)), n),
                    dot(cross(sub(c, b), sub(p, b)), n),
                    dot(cross(sub(a, c), sub(p, c)), n),
                ];
                let projected = signs.iter().all(|&v| v >= 0) || signs.iter().all(|&v| v <= 0);
                if projected && plane * plane <= r2 * n2 {
                    return true;
                }
                for (a, b) in [(a, b), (b, c), (c, a)] {
                    let edge = sub(b, a);
                    let delta = sub(p, a);
                    let den = dot(edge, edge);
                    let t = dot(delta, edge);
                    if den == 0 || t <= 0 {
                        if dot(delta, delta) <= r2 {
                            return true;
                        }
                    } else if t >= den {
                        let d = sub(p, b);
                        if dot(d, d) <= r2 {
                            return true;
                        }
                    } else if dot(delta, delta) * den - t * t <= r2 * den {
                        return true;
                    }
                }
            }
        }
    }
    faces > 0 && inside
}
fn intersects(ball: Shape, target: Shape, pose: PoseMm) -> Result<bool, GeomError> {
    use crate::query::posed_point;
    let Shape::Sphere {
        local_center,
        radius_mm,
    } = ball
    else {
        return Err(GeomError::Malformed);
    };
    // Broad-phase pruning is only an optimization; a successful result always
    // comes from the actual primitive or convex geometry below.
    if !crate::bounds(ball, PoseMm::default())?.intersects(crate::bounds(target, pose)?) {
        return Ok(false);
    }
    match target {
        Shape::Convex { vertices, len } => {
            if usize::from(len) > vertices.len() || len < 4 {
                return Err(GeomError::Malformed);
            }
            let points = vertices.map(|v| posed_point(v, pose));
            if points[..usize::from(len)]
                .iter()
                .any(|&v| distance(v, local_center) > 200_000)
            {
                return Err(GeomError::Malformed);
            }
            Ok(ball_convex(
                local_center,
                radius_mm,
                &points[..usize::from(len)],
            ))
        }
        Shape::Compound { parts, len } => {
            if len == 0 || usize::from(len) > parts.len() {
                return Err(GeomError::Malformed);
            }
            for part in &parts[..usize::from(len)] {
                let p = posed_point(part.local_pose.translation(), pose);
                let child = PoseMm {
                    x: klotho_core::Mm(p.x),
                    y: klotho_core::Mm(p.y),
                    z: klotho_core::Mm(p.z),
                    yaw: klotho_core::YawMd(pose.yaw.0.wrapping_add(part.local_pose.yaw.0)),
                    pitch: klotho_core::YawMd(pose.pitch.0.wrapping_add(part.local_pose.pitch.0)),
                    roll: klotho_core::YawMd(pose.roll.0.wrapping_add(part.local_pose.roll.0)),
                };
                if intersects(ball, part.shape.as_shape(), child)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(contact(ball, PoseMm::default(), target, pose)?.is_some()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{AabbMm, IVec3, YawMd};
    fn p(x: i32, y: i32, z: i32) -> IVec3 {
        IVec3 { x, y, z }
    }
    #[test]
    fn a_rotated_box_aabb_is_not_hit_evidence() {
        let shape = Shape::oriented_box(AabbMm::new(p(-1000, 0, -50), p(1000, 100, 50))).unwrap();
        let pose = PoseMm {
            yaw: YawMd(45000),
            ..PoseMm::default()
        };
        let point = p(450, 50, 450);
        assert!(crate::bounds(shape, pose).unwrap().contains_point(point));
        assert_eq!(
            semantic_sweep([[point; 2]; 2], 10, shape, pose).unwrap(),
            None
        );
    }
    #[test]
    fn thin_crossing_and_small_radius_have_actual_narrow_evidence() {
        let target = Shape::oriented_box(AabbMm::new(p(-2, 0, -100), p(2, 100, 100))).unwrap();
        for radius in [1, 60] {
            let ends = [[p(-300, 50, 0); 2], [p(300, 50, 0); 2]];
            let sample = semantic_sweep(ends, radius, target, PoseMm::default())
                .unwrap()
                .unwrap();
            assert!(
                verify_semantic_sweep(ends, radius, target, PoseMm::default(), sample).unwrap()
            );
            assert!(
                !verify_semantic_sweep(
                    ends,
                    radius,
                    target,
                    PoseMm::default(),
                    SweepSample {
                        time: u16::MAX,
                        segment: 0
                    }
                )
                .unwrap_or(false)
            );
        }
    }
    #[test]
    fn convex_void_cannot_become_a_contact_from_its_bounds() {
        let shape = Shape::convex(&[p(0, 0, 0), p(100, 0, 0), p(0, 100, 0), p(0, 0, 100)]).unwrap();
        let outside = p(70, 70, 70);
        let inside = p(10, 10, 10);
        assert!(
            crate::bounds(shape, PoseMm::default())
                .unwrap()
                .contains_point(outside)
        );
        assert_eq!(
            semantic_sweep([[outside; 2]; 2], 10, shape, PoseMm::default()).unwrap(),
            None
        );
        assert!(
            semantic_sweep([[inside; 2]; 2], 1, shape, PoseMm::default())
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn excess_query_work_fails_closed() {
        let ends = [
            [p(-100000, 0, 0), p(-100000, 100000, 0)],
            [p(100000, 0, 0), p(100000, 100000, 0)],
        ];
        assert_eq!(semantic_sample_count(ends), Err(GeomError::Malformed));
    }
}
