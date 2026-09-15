//! Bounds, distance, and contact for PHYS-A03 primitives.

use klotho_core::{AabbMm, IVec3, Mm, PoseMm, QuantizedContact, YawMd, rotate, rotation_axes};

use crate::shape::{GeomError, Shape};
use crate::terrain::{heightfield_bounds, mesh_bounds, terrain_contact};

const FX: i64 = 65_536;

/// Bounded, deterministically ordered contact patch.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct ContactManifold {
    /// Fixed point storage; only the first `len` entries participate.
    pub points: [QuantizedContact; 4],
    /// Number of live points.
    pub len: u8,
}

impl ContactManifold {
    /// Live contact points in canonical point order.
    #[must_use]
    pub fn as_slice(&self) -> &[QuantizedContact] {
        &self.points[..usize::from(self.len)]
    }
}

/// Conservative world AABB of `shape` at `pose`.
pub fn bounds(shape: Shape, pose: PoseMm) -> Result<AabbMm, GeomError> {
    match shape {
        Shape::OrientedBox { local } => Ok(obb_bounds(local, pose)),
        Shape::Sphere {
            local_center,
            radius_mm,
        } => {
            let c = posed_point(local_center, pose);
            Ok(AabbMm::new(
                IVec3 {
                    x: c.x.saturating_sub(radius_mm),
                    y: c.y.saturating_sub(radius_mm),
                    z: c.z.saturating_sub(radius_mm),
                },
                IVec3 {
                    x: c.x.saturating_add(radius_mm),
                    y: c.y.saturating_add(radius_mm),
                    z: c.z.saturating_add(radius_mm),
                },
            ))
        }
        Shape::Capsule {
            local_center,
            radius_mm,
            half_height_mm,
        } => {
            let (p0, p1) = capsule_ends(local_center, half_height_mm, pose);
            let a = AabbMm::from_point(p0).union(AabbMm::from_point(p1));
            let pad = IVec3 {
                x: radius_mm,
                y: radius_mm,
                z: radius_mm,
            };
            Ok(AabbMm::new(
                a.min.wrapping_sub(pad),
                a.max.wrapping_add(pad),
            ))
        }
        Shape::Convex { vertices, len } => {
            let points = convex_points(&vertices, len, pose)?;
            Ok(points_bounds(&points))
        }
        Shape::Compound { parts, len } => {
            let mut iter = parts.iter().take(usize::from(len));
            let Some(first) = iter.next() else {
                return Err(GeomError::Malformed);
            };
            let mut out = bounds(first.shape.as_shape(), compose_pose(pose, first.local_pose))?;
            for part in iter {
                let child_pose = compose_pose(pose, part.local_pose);
                out = out.union(bounds(part.shape.as_shape(), child_pose)?);
            }
            if out.is_empty() {
                Err(GeomError::Malformed)
            } else {
                Ok(out)
            }
        }
        Shape::TriangleMesh { tris, len } => {
            let b = mesh_bounds(&tris, len, pose);
            if b.is_empty() {
                Err(GeomError::Malformed)
            } else {
                Ok(b)
            }
        }
        Shape::Heightfield {
            origin,
            cell_x_mm,
            cell_z_mm,
            nx,
            nz,
            samples,
        } => {
            let b = heightfield_bounds(origin, cell_x_mm, cell_z_mm, nx, nz, &samples, pose);
            if b.is_empty() {
                Err(GeomError::Malformed)
            } else {
                Ok(b)
            }
        }
    }
}

/// Penetration in millimetres. `None` if separated. Zero is touching.
pub fn penetration_mm(
    a: Shape,
    pose_a: PoseMm,
    b: Shape,
    pose_b: PoseMm,
) -> Result<Option<i32>, GeomError> {
    Ok(contact(a, pose_a, b, pose_b)?.map(|c| c.depth_mm))
}

/// Kernel-reproducible contact. `None` if separated (including open gaps).
pub fn contact(
    a: Shape,
    pose_a: PoseMm,
    b: Shape,
    pose_b: PoseMm,
) -> Result<Option<QuantizedContact>, GeomError> {
    match (a, b) {
        (Shape::Compound { parts, len }, other) => {
            compound_contact(parts, len, pose_a, other, pose_b, false)
        }
        (other, Shape::Compound { parts, len }) => {
            compound_contact(parts, len, pose_b, other, pose_a, true)
        }
        (Shape::Convex { .. }, Shape::Convex { .. })
        | (Shape::Convex { .. }, Shape::OrientedBox { .. })
        | (Shape::OrientedBox { .. }, Shape::Convex { .. }) => {
            convex_poly_contact(a, pose_a, b, pose_b)
        }
        (Shape::OrientedBox { local: la }, Shape::OrientedBox { local: lb }) => {
            Ok(obb_obb(la, pose_a, lb, pose_b))
        }
        (Shape::Sphere { .. }, Shape::Sphere { .. }) => Ok(sphere_sphere(a, pose_a, b, pose_b)),
        (Shape::Capsule { .. }, Shape::Capsule { .. }) => Ok(capsule_capsule(a, pose_a, b, pose_b)),
        (Shape::Sphere { .. }, Shape::OrientedBox { local }) => {
            Ok(sphere_obb(a, pose_a, local, pose_b, false))
        }
        (Shape::OrientedBox { local }, Shape::Sphere { .. }) => {
            Ok(sphere_obb(b, pose_b, local, pose_a, true))
        }
        (Shape::Capsule { .. }, Shape::OrientedBox { local }) => {
            Ok(capsule_obb(a, pose_a, local, pose_b, false))
        }
        (Shape::OrientedBox { local }, Shape::Capsule { .. }) => {
            Ok(capsule_obb(b, pose_b, local, pose_a, true))
        }
        (Shape::Sphere { .. }, Shape::Capsule { .. }) => {
            Ok(sphere_capsule(a, pose_a, b, pose_b, false))
        }
        (Shape::Capsule { .. }, Shape::Sphere { .. }) => {
            Ok(sphere_capsule(b, pose_b, a, pose_a, true))
        }
        (Shape::TriangleMesh { .. } | Shape::Heightfield { .. }, other) => {
            terrain_contact(a, pose_a, other, pose_b, true)
        }
        (other, Shape::TriangleMesh { .. } | Shape::Heightfield { .. }) => {
            terrain_contact(b, pose_b, other, pose_a, false)
        }
        _ => Err(GeomError::Unsupported),
    }
}

/// Stable contact patch with at most four quantized points.
pub fn manifold(
    a: Shape,
    pose_a: PoseMm,
    b: Shape,
    pose_b: PoseMm,
) -> Result<Option<ContactManifold>, GeomError> {
    let Some(primary) = contact(a, pose_a, b, pose_b)? else {
        return Ok(None);
    };
    let mut candidates = Vec::new();
    if let Ok(points) = poly_points(a, pose_a) {
        let bb = bounds(b, pose_b)?;
        candidates.extend(points.into_iter().filter(|&p| bb.contains_point(p)));
    }
    if let Ok(points) = poly_points(b, pose_b) {
        let ba = bounds(a, pose_a)?;
        candidates.extend(points.into_iter().filter(|&p| ba.contains_point(p)));
    }
    candidates.push(primary.point);
    candidates.sort_unstable_by_key(|p| (p.x, p.y, p.z));
    candidates.dedup();
    let mut out = ContactManifold::default();
    for (i, point) in candidates.into_iter().take(4).enumerate() {
        out.points[i] = QuantizedContact {
            point,
            feature: primary.feature.saturating_add(i as u16),
            ..primary
        };
        out.len += 1;
    }
    Ok(Some(out))
}

fn compound_contact(
    parts: [crate::shape::CompoundPart; crate::shape::MAX_COMPOUND_PARTS],
    len: u8,
    compound_pose: PoseMm,
    other: Shape,
    other_pose: PoseMm,
    flip: bool,
) -> Result<Option<QuantizedContact>, GeomError> {
    if len == 0 || usize::from(len) > parts.len() {
        return Err(GeomError::Malformed);
    }
    let mut best: Option<QuantizedContact> = None;
    for (i, part) in parts.iter().take(usize::from(len)).enumerate() {
        let child_pose = compose_pose(compound_pose, part.local_pose);
        let mut hit = if flip {
            contact(other, other_pose, part.shape.as_shape(), child_pose)?
        } else {
            contact(part.shape.as_shape(), child_pose, other, other_pose)?
        };
        if let Some(ref mut h) = hit {
            h.feature = ((i as u16) << 8) | (h.feature & 0xff);
        }
        if hit.is_some_and(|h| {
            best.is_none_or(|old| {
                (h.depth_mm, core::cmp::Reverse(h.feature))
                    > (old.depth_mm, core::cmp::Reverse(old.feature))
            })
        }) {
            best = hit;
        }
    }
    Ok(best)
}

fn compose_pose(parent: PoseMm, local: PoseMm) -> PoseMm {
    let t = posed_point(local.translation(), parent);
    PoseMm {
        x: Mm(t.x),
        y: Mm(t.y),
        z: Mm(t.z),
        yaw: YawMd(parent.yaw.0.wrapping_add(local.yaw.0)),
        pitch: YawMd(parent.pitch.0.wrapping_add(local.pitch.0)),
        roll: YawMd(parent.roll.0.wrapping_add(local.roll.0)),
    }
}

fn convex_points(
    vertices: &[IVec3; crate::shape::MAX_CONVEX_VERTICES],
    len: u8,
    pose: PoseMm,
) -> Result<Vec<IVec3>, GeomError> {
    if !(4..=crate::shape::MAX_CONVEX_VERTICES).contains(&usize::from(len)) {
        return Err(GeomError::Malformed);
    }
    Ok(vertices
        .iter()
        .take(usize::from(len))
        .map(|&v| posed_point(v, pose))
        .collect())
}

fn poly_points(shape: Shape, pose: PoseMm) -> Result<Vec<IVec3>, GeomError> {
    match shape {
        Shape::OrientedBox { local } => Ok(corners(local, pose).to_vec()),
        Shape::Convex { vertices, len } => convex_points(&vertices, len, pose),
        _ => Err(GeomError::Unsupported),
    }
}

fn points_bounds(points: &[IVec3]) -> AabbMm {
    let mut iter = points.iter().copied();
    let Some(first) = iter.next() else {
        return AabbMm::new(IVec3 { x: 1, y: 1, z: 1 }, IVec3::ZERO);
    };
    let mut out = AabbMm::from_point(first);
    for p in iter {
        out = out.union(AabbMm::from_point(p));
    }
    out
}

fn convex_poly_contact(
    a: Shape,
    pose_a: PoseMm,
    b: Shape,
    pose_b: PoseMm,
) -> Result<Option<QuantizedContact>, GeomError> {
    let pa = poly_points(a, pose_a)?;
    let pb = poly_points(b, pose_b)?;
    let ca = centroid_slice(&pa);
    let cb = centroid_slice(&pb);
    let mut axes = Vec::new();
    axes.extend(pa.iter().map(|&p| p.wrapping_sub(ca)));
    axes.extend(pb.iter().map(|&p| p.wrapping_sub(cb)));
    for aw in pa.windows(2) {
        let ea = aw[1].wrapping_sub(aw[0]);
        for bw in pb.windows(2) {
            axes.push(cross_plain(ea, bw[1].wrapping_sub(bw[0])));
        }
    }
    let mut best_depth = i32::MAX;
    let mut best_axis = IVec3::ZERO;
    let mut best_feature = 0u16;
    for (i, axis) in axes.into_iter().enumerate() {
        if l1(axis) < 2 {
            continue;
        }
        let Some(depth) = interval_overlap_slice(&pa, &pb, axis) else {
            return Ok(None);
        };
        if depth < best_depth {
            best_depth = depth;
            best_axis = axis;
            best_feature = u16::try_from(i).unwrap_or(u16::MAX);
        }
    }
    if best_axis == IVec3::ZERO {
        return Err(GeomError::Malformed);
    }
    Ok(Some(QuantizedContact {
        point: midpoint(support(&pa, neg(best_axis)), support(&pb, best_axis)),
        normal: pack_normal(
            best_axis,
            if dot_i(ca.wrapping_sub(cb), best_axis) >= 0 {
                1
            } else {
                -1
            },
        ),
        depth_mm: best_depth,
        feature: best_feature,
    }))
}

fn interval_overlap_slice(a: &[IVec3], b: &[IVec3], n: IVec3) -> Option<i32> {
    let project_one = |points: &[IVec3]| {
        points.iter().fold((i64::MAX, i64::MIN), |(lo, hi), &p| {
            let d = dot_i(p, n);
            (lo.min(d), hi.max(d))
        })
    };
    let (amin, amax) = project_one(a);
    let (bmin, bmax) = project_one(b);
    let overlap = amax.min(bmax) - amin.max(bmin);
    (overlap >= 0).then(|| {
        let mag = isqrt(dot_i(n, n)).max(1);
        (overlap / mag).clamp(0, i64::from(i32::MAX)) as i32
    })
}

fn centroid_slice(points: &[IVec3]) -> IVec3 {
    let n = points.len() as i64;
    let sum = points.iter().fold([0i64; 3], |mut s, p| {
        s[0] += i64::from(p.x);
        s[1] += i64::from(p.y);
        s[2] += i64::from(p.z);
        s
    });
    IVec3 {
        x: (sum[0] / n) as i32,
        y: (sum[1] / n) as i32,
        z: (sum[2] / n) as i32,
    }
}

fn cross_plain(a: IVec3, b: IVec3) -> IVec3 {
    IVec3 {
        x: (i64::from(a.y) * i64::from(b.z) - i64::from(a.z) * i64::from(b.y))
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        y: (i64::from(a.z) * i64::from(b.x) - i64::from(a.x) * i64::from(b.z))
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        z: (i64::from(a.x) * i64::from(b.y) - i64::from(a.y) * i64::from(b.x))
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    }
}

fn support(points: &[IVec3], direction: IVec3) -> IVec3 {
    points
        .iter()
        .copied()
        .max_by_key(|&p| (dot_i(p, direction), p.x, p.y, p.z))
        .unwrap_or(IVec3::ZERO)
}

pub(crate) fn posed_point(local: IVec3, pose: PoseMm) -> IVec3 {
    pose.translation()
        .wrapping_add(rotate(local, pose.yaw, pose.pitch, pose.roll))
}

fn obb_bounds(local: AabbMm, pose: PoseMm) -> AabbMm {
    let mut min = IVec3 {
        x: i32::MAX,
        y: i32::MAX,
        z: i32::MAX,
    };
    let mut max = IVec3 {
        x: i32::MIN,
        y: i32::MIN,
        z: i32::MIN,
    };
    for c in corners(local, pose) {
        min = min.min(c);
        max = max.max(c);
    }
    AabbMm::new(min, max)
}

fn corners(local: AabbMm, pose: PoseMm) -> [IVec3; 8] {
    let xs = [local.min.x, local.max.x];
    let ys = [local.min.y, local.max.y];
    let zs = [local.min.z, local.max.z];
    let mut out = [IVec3::ZERO; 8];
    let mut i = 0;
    for &x in &xs {
        for &y in &ys {
            for &z in &zs {
                out[i] = posed_point(IVec3 { x, y, z }, pose);
                i += 1;
            }
        }
    }
    out
}

fn capsule_ends(center: IVec3, half_height: i32, pose: PoseMm) -> (IVec3, IVec3) {
    let d = IVec3 {
        x: 0,
        y: half_height,
        z: 0,
    };
    (
        posed_point(
            IVec3 {
                x: center.x,
                y: center.y.wrapping_sub(d.y),
                z: center.z,
            },
            pose,
        ),
        posed_point(
            IVec3 {
                x: center.x,
                y: center.y.wrapping_add(d.y),
                z: center.z,
            },
            pose,
        ),
    )
}

fn obb_obb(la: AabbMm, pa: PoseMm, lb: AabbMm, pb: PoseMm) -> Option<QuantizedContact> {
    let ca = corners(la, pa);
    let cb = corners(lb, pb);
    let (ax, ay, az) = rotation_axes(pa.yaw, pa.pitch, pa.roll);
    let (bx, by, bz) = rotation_axes(pb.yaw, pb.pitch, pb.roll);
    let a_axes = [ax, ay, az];
    let b_axes = [bx, by, bz];
    let mut best_depth = i32::MAX;
    let mut best_axis = IVec3::ZERO;
    let mut best_feature = 0u16;
    let mut found = false;

    for (i, n) in a_axes.into_iter().enumerate() {
        {
            let depth = interval_overlap_mm(&ca, &cb, n)?;
            if depth < best_depth {
                best_depth = depth;
                best_axis = n;
                best_feature = i as u16;
                found = true;
            }
        }
    }
    for (i, n) in b_axes.into_iter().enumerate() {
        {
            let depth = interval_overlap_mm(&ca, &cb, n)?;
            if depth < best_depth {
                best_depth = depth;
                best_axis = n;
                best_feature = 3 + i as u16;
                found = true;
            }
        }
    }
    let mut f = 6u16;
    for a in a_axes {
        for b in b_axes {
            let n = cross_fx(a, b);
            if l1(n) < 256 {
                f = f.saturating_add(1);
                continue;
            }
            {
                let depth = interval_overlap_mm(&ca, &cb, n)?;
                if depth < best_depth {
                    best_depth = depth;
                    best_axis = n;
                    best_feature = f;
                    found = true;
                }
            }
            f = f.saturating_add(1);
        }
    }
    if !found {
        return None;
    }
    let na = pack_axis_from_a_to_b(&ca, &cb, best_axis);
    let point = midpoint(centroid(&ca), centroid(&cb));
    Some(QuantizedContact {
        point,
        normal: na,
        depth_mm: best_depth,
        feature: best_feature,
    })
}

fn interval_overlap_mm(ca: &[IVec3; 8], cb: &[IVec3; 8], n: IVec3) -> Option<i32> {
    if n == IVec3::ZERO {
        return Some(0);
    }
    let (amin, amax) = project(ca, n);
    let (bmin, bmax) = project(cb, n);
    let overlap = amax.min(bmax) - amin.max(bmin);
    if overlap < 0 {
        return None;
    }
    Some(overlap_to_mm(overlap, n))
}

fn project(cs: &[IVec3; 8], n: IVec3) -> (i64, i64) {
    let mut min = i64::MAX;
    let mut max = i64::MIN;
    for c in cs {
        let d = dot_fx(*c, n);
        min = min.min(d);
        max = max.max(d);
    }
    (min, max)
}

fn dot_fx(p: IVec3, n: IVec3) -> i64 {
    (i64::from(p.x) * i64::from(n.x)
        + i64::from(p.y) * i64::from(n.y)
        + i64::from(p.z) * i64::from(n.z))
        >> 16
}

fn overlap_to_mm(overlap_fx: i64, n: IVec3) -> i32 {
    let mag2 = i64::from(n.x) * i64::from(n.x)
        + i64::from(n.y) * i64::from(n.y)
        + i64::from(n.z) * i64::from(n.z);
    if mag2 <= 0 {
        return overlap_fx.clamp(0, i64::from(i32::MAX)) as i32;
    }
    // overlap_fx is millimetres when |n| is 16.16 unit. Scale otherwise.
    let mm = (overlap_fx * FX) / isqrt(mag2).max(1);
    mm.clamp(0, i64::from(i32::MAX)) as i32
}

fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

fn cross_fx(a: IVec3, b: IVec3) -> IVec3 {
    IVec3 {
        x: ((i64::from(a.y) * i64::from(b.z) - i64::from(a.z) * i64::from(b.y)) >> 16) as i32,
        y: ((i64::from(a.z) * i64::from(b.x) - i64::from(a.x) * i64::from(b.z)) >> 16) as i32,
        z: ((i64::from(a.x) * i64::from(b.y) - i64::from(a.y) * i64::from(b.x)) >> 16) as i32,
    }
}

fn l1(v: IVec3) -> i64 {
    i64::from(v.x.unsigned_abs()) + i64::from(v.y.unsigned_abs()) + i64::from(v.z.unsigned_abs())
}

fn pack_axis_from_a_to_b(ca: &[IVec3; 8], cb: &[IVec3; 8], n: IVec3) -> (i16, i16, i16) {
    let ac = centroid(ca);
    let bc = centroid(cb);
    let delta = ac.wrapping_sub(bc);
    let sign = if dot_i(delta, n) >= 0 { 1i64 } else { -1 };
    pack_normal(n, sign)
}

fn pack_normal(n: IVec3, sign: i64) -> (i16, i16, i16) {
    let mag = isqrt(
        i64::from(n.x) * i64::from(n.x)
            + i64::from(n.y) * i64::from(n.y)
            + i64::from(n.z) * i64::from(n.z),
    )
    .max(1);
    let s = 32_767i64 * sign;
    (
        ((i64::from(n.x) * s) / mag).clamp(-32_767, 32_767) as i16,
        ((i64::from(n.y) * s) / mag).clamp(-32_767, 32_767) as i16,
        ((i64::from(n.z) * s) / mag).clamp(-32_767, 32_767) as i16,
    )
}

fn centroid(cs: &[IVec3; 8]) -> IVec3 {
    let mut x = 0i64;
    let mut y = 0i64;
    let mut z = 0i64;
    for c in cs {
        x += i64::from(c.x);
        y += i64::from(c.y);
        z += i64::from(c.z);
    }
    IVec3 {
        x: (x / 8) as i32,
        y: (y / 8) as i32,
        z: (z / 8) as i32,
    }
}

fn midpoint(a: IVec3, b: IVec3) -> IVec3 {
    IVec3 {
        x: ((i64::from(a.x) + i64::from(b.x)) / 2) as i32,
        y: ((i64::from(a.y) + i64::from(b.y)) / 2) as i32,
        z: ((i64::from(a.z) + i64::from(b.z)) / 2) as i32,
    }
}

fn dot_i(a: IVec3, b: IVec3) -> i64 {
    i64::from(a.x) * i64::from(b.x)
        + i64::from(a.y) * i64::from(b.y)
        + i64::from(a.z) * i64::from(b.z)
}

fn sphere_sphere(a: Shape, pa: PoseMm, b: Shape, pb: PoseMm) -> Option<QuantizedContact> {
    let Shape::Sphere {
        local_center: ca,
        radius_mm: ra,
    } = a
    else {
        return None;
    };
    let Shape::Sphere {
        local_center: cb,
        radius_mm: rb,
    } = b
    else {
        return None;
    };
    let wa = posed_point(ca, pa);
    let wb = posed_point(cb, pb);
    sphere_points(wa, ra, wb, rb, 0)
}

fn sphere_points(wa: IVec3, ra: i32, wb: IVec3, rb: i32, feature: u16) -> Option<QuantizedContact> {
    let d = wa.wrapping_sub(wb);
    let dist2 = dot_i(d, d);
    let limit = i64::from(ra.saturating_add(rb));
    if dist2 > limit * limit {
        return None;
    }
    let dist = isqrt(dist2);
    let depth = (limit - dist).clamp(0, i64::from(i32::MAX)) as i32;
    let n = if dist == 0 {
        IVec3 {
            x: 0,
            y: FX as i32,
            z: 0,
        }
    } else {
        IVec3 {
            x: ((i64::from(d.x) * FX) / dist) as i32,
            y: ((i64::from(d.y) * FX) / dist) as i32,
            z: ((i64::from(d.z) * FX) / dist) as i32,
        }
    };
    Some(QuantizedContact {
        point: midpoint(wa, wb),
        normal: pack_normal(n, 1),
        depth_mm: depth,
        feature,
    })
}

fn sphere_obb(
    sphere: Shape,
    ps: PoseMm,
    local: AabbMm,
    pb: PoseMm,
    flip: bool,
) -> Option<QuantizedContact> {
    let Shape::Sphere {
        local_center,
        radius_mm,
    } = sphere
    else {
        return None;
    };
    let c = posed_point(local_center, ps);
    let (closest, inside, inward) = closest_on_obb(c, local, pb);
    let d = c.wrapping_sub(closest);
    let dist2 = dot_i(d, d);
    if !inside && dist2 > i64::from(radius_mm) * i64::from(radius_mm) {
        return None;
    }
    let (n_src, depth) = if inside {
        (inward, radius_mm.saturating_add(inward_depth(c, local, pb)))
    } else {
        let dist = isqrt(dist2);
        let n = if dist == 0 {
            IVec3 {
                x: 0,
                y: FX as i32,
                z: 0,
            }
        } else {
            IVec3 {
                x: ((i64::from(d.x) * FX) / dist) as i32,
                y: ((i64::from(d.y) * FX) / dist) as i32,
                z: ((i64::from(d.z) * FX) / dist) as i32,
            }
        };
        (
            n,
            (i64::from(radius_mm) - dist).clamp(0, i64::from(i32::MAX)) as i32,
        )
    };
    let sign = if flip { -1 } else { 1 };
    Some(QuantizedContact {
        point: closest,
        normal: pack_normal(n_src, sign),
        depth_mm: depth,
        feature: if flip { 1 } else { 0 },
    })
}

fn inward_depth(world: IVec3, local: AabbMm, pose: PoseMm) -> i32 {
    let (lx, ly, lz) = to_local(world, pose);
    let dx = (lx - local.min.x).abs().min((local.max.x - lx).abs());
    let dy = (ly - local.min.y).abs().min((local.max.y - ly).abs());
    let dz = (lz - local.min.z).abs().min((local.max.z - lz).abs());
    dx.min(dy).min(dz)
}

fn closest_on_obb(world: IVec3, local: AabbMm, pose: PoseMm) -> (IVec3, bool, IVec3) {
    let (lx, ly, lz) = to_local(world, pose);
    let cx = lx.clamp(local.min.x, local.max.x);
    let cy = ly.clamp(local.min.y, local.max.y);
    let cz = lz.clamp(local.min.z, local.max.z);
    let inside = cx == lx && cy == ly && cz == lz;
    let closest = posed_point(
        IVec3 {
            x: cx,
            y: cy,
            z: cz,
        },
        pose,
    );
    let (ax, ay, az) = rotation_axes(pose.yaw, pose.pitch, pose.roll);
    let inward = nearest_face_axis(lx, ly, lz, local, ax, ay, az);
    (closest, inside, inward)
}

fn nearest_face_axis(
    lx: i32,
    ly: i32,
    lz: i32,
    local: AabbMm,
    ax: IVec3,
    ay: IVec3,
    az: IVec3,
) -> IVec3 {
    let dx = (lx - local.min.x).abs().min((local.max.x - lx).abs());
    let dy = (ly - local.min.y).abs().min((local.max.y - ly).abs());
    let dz = (lz - local.min.z).abs().min((local.max.z - lz).abs());
    if dx <= dy && dx <= dz {
        if lx - local.min.x < local.max.x - lx {
            neg(ax)
        } else {
            ax
        }
    } else if dy <= dz {
        if ly - local.min.y < local.max.y - ly {
            neg(ay)
        } else {
            ay
        }
    } else if lz - local.min.z < local.max.z - lz {
        neg(az)
    } else {
        az
    }
}

fn neg(v: IVec3) -> IVec3 {
    IVec3 {
        x: v.x.wrapping_neg(),
        y: v.y.wrapping_neg(),
        z: v.z.wrapping_neg(),
    }
}

fn to_local(world: IVec3, pose: PoseMm) -> (i32, i32, i32) {
    let d = world.wrapping_sub(pose.translation());
    let (ax, ay, az) = rotation_axes(pose.yaw, pose.pitch, pose.roll);
    (dot_axis(d, ax), dot_axis(d, ay), dot_axis(d, az))
}

pub(crate) fn world_to_local_point(world: IVec3, pose: PoseMm) -> IVec3 {
    let (x, y, z) = to_local(world, pose);
    IVec3 { x, y, z }
}

fn dot_axis(v: IVec3, a: IVec3) -> i32 {
    ((i64::from(v.x) * i64::from(a.x)
        + i64::from(v.y) * i64::from(a.y)
        + i64::from(v.z) * i64::from(a.z))
        >> 16) as i32
}

fn capsule_obb(
    cap: Shape,
    pc: PoseMm,
    local: AabbMm,
    pb: PoseMm,
    flip: bool,
) -> Option<QuantizedContact> {
    let Shape::Capsule {
        local_center,
        radius_mm,
        half_height_mm,
    } = cap
    else {
        return None;
    };
    let (p0, p1) = capsule_ends(local_center, half_height_mm, pc);
    let q = closest_aabb_to_segment(p0, p1, local, pb);
    sphere_obb(
        Shape::Sphere {
            local_center: IVec3::ZERO,
            radius_mm,
        },
        PoseMm {
            x: Mm(q.x),
            y: Mm(q.y),
            z: Mm(q.z),
            yaw: YawMd::ZERO,
            pitch: YawMd::ZERO,
            roll: YawMd::ZERO,
        },
        local,
        pb,
        flip,
    )
}

fn closest_aabb_to_segment(p0: IVec3, p1: IVec3, local: AabbMm, pose: PoseMm) -> IVec3 {
    let mut best = p0;
    let mut best_d = i64::MAX;
    for i in 0..=8 {
        let t = i64::from(i);
        let p = IVec3 {
            x: (i64::from(p0.x) + (i64::from(p1.x) - i64::from(p0.x)) * t / 8) as i32,
            y: (i64::from(p0.y) + (i64::from(p1.y) - i64::from(p0.y)) * t / 8) as i32,
            z: (i64::from(p0.z) + (i64::from(p1.z) - i64::from(p0.z)) * t / 8) as i32,
        };
        let (c, _, _) = closest_on_obb(p, local, pose);
        let d = dot_i(p.wrapping_sub(c), p.wrapping_sub(c));
        if d < best_d {
            best_d = d;
            best = p;
        }
    }
    best
}

fn sphere_capsule(
    sphere: Shape,
    ps: PoseMm,
    cap: Shape,
    pc: PoseMm,
    flip: bool,
) -> Option<QuantizedContact> {
    let Shape::Sphere {
        local_center,
        radius_mm: rs,
    } = sphere
    else {
        return None;
    };
    let Shape::Capsule {
        local_center: cc,
        radius_mm: rc,
        half_height_mm,
    } = cap
    else {
        return None;
    };
    let c = posed_point(local_center, ps);
    let (p0, p1) = capsule_ends(cc, half_height_mm, pc);
    let q = closest_on_segment(c, p0, p1);
    let mut hit = sphere_points(c, rs, q, rc, 2)?;
    if flip {
        hit.normal = (-hit.normal.0, -hit.normal.1, -hit.normal.2);
    }
    Some(hit)
}

fn capsule_capsule(a: Shape, pa: PoseMm, b: Shape, pb: PoseMm) -> Option<QuantizedContact> {
    let Shape::Capsule {
        local_center: ca,
        radius_mm: ra,
        half_height_mm: ha,
    } = a
    else {
        return None;
    };
    let Shape::Capsule {
        local_center: cb,
        radius_mm: rb,
        half_height_mm: hb,
    } = b
    else {
        return None;
    };
    let (a0, a1) = capsule_ends(ca, ha, pa);
    let (b0, b1) = capsule_ends(cb, hb, pb);
    let (pa, pb) = closest_segments(a0, a1, b0, b1);
    sphere_points(pa, ra, pb, rb, 3)
}

fn closest_on_segment(p: IVec3, a: IVec3, b: IVec3) -> IVec3 {
    let ab = b.wrapping_sub(a);
    let ap = p.wrapping_sub(a);
    let den = dot_i(ab, ab);
    if den <= 0 {
        return a;
    }
    let t = (dot_i(ap, ab)).clamp(0, den);
    IVec3 {
        x: a.x.wrapping_add(((i64::from(ab.x) * t) / den) as i32),
        y: a.y.wrapping_add(((i64::from(ab.y) * t) / den) as i32),
        z: a.z.wrapping_add(((i64::from(ab.z) * t) / den) as i32),
    }
}

fn closest_segments(a0: IVec3, a1: IVec3, b0: IVec3, b1: IVec3) -> (IVec3, IVec3) {
    let mut best_a = a0;
    let mut best_b = b0;
    let mut best = i64::MAX;
    for i in 0..=8 {
        let t = i64::from(i);
        let pa = IVec3 {
            x: (i64::from(a0.x) + (i64::from(a1.x) - i64::from(a0.x)) * t / 8) as i32,
            y: (i64::from(a0.y) + (i64::from(a1.y) - i64::from(a0.y)) * t / 8) as i32,
            z: (i64::from(a0.z) + (i64::from(a1.z) - i64::from(a0.z)) * t / 8) as i32,
        };
        let pb = closest_on_segment(pa, b0, b1);
        let d = dot_i(pa.wrapping_sub(pb), pa.wrapping_sub(pb));
        if d < best {
            best = d;
            best_a = pa;
            best_b = pb;
        }
    }
    (best_a, best_b)
}
