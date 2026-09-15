//! Static mesh and heightfield occupancy queries.

use klotho_core::{AabbMm, IVec3, PoseMm, QuantizedContact, rotate};

use crate::query::{bounds, posed_point};
use crate::shape::{GeomError, MAX_HEIGHTFIELD_SAMPLES, MAX_MESH_TRIANGLES, Shape};

pub(crate) fn mesh_bounds(
    tris: &[[IVec3; 3]; MAX_MESH_TRIANGLES],
    len: u8,
    pose: PoseMm,
) -> AabbMm {
    let mut out: Option<AabbMm> = None;
    for tri in tris.iter().take(usize::from(len)) {
        for &p in tri {
            let w = posed_point(p, pose);
            out = Some(match out {
                None => AabbMm::from_point(w),
                Some(b) => b.union(AabbMm::from_point(w)),
            });
        }
    }
    out.unwrap_or_else(|| AabbMm::new(IVec3 { x: 1, y: 1, z: 1 }, IVec3::ZERO))
}

pub(crate) fn heightfield_bounds(
    origin: IVec3,
    cell_x_mm: i32,
    cell_z_mm: i32,
    nx: u8,
    nz: u8,
    samples: &[i32; MAX_HEIGHTFIELD_SAMPLES],
    pose: PoseMm,
) -> AabbMm {
    let nx_n = usize::from(nx).max(1);
    let nz_n = usize::from(nz).max(1);
    let live = nx_n.saturating_mul(nz_n).min(samples.len());
    let mut ymin = i32::MAX;
    let mut ymax = i32::MIN;
    for &h in samples.iter().take(live) {
        ymin = ymin.min(h);
        ymax = ymax.max(h);
    }
    if ymin > ymax {
        ymin = origin.y;
        ymax = origin.y;
    }
    let max = IVec3 {
        x: origin
            .x
            .saturating_add(cell_x_mm.saturating_mul(i32::from(nx.saturating_sub(1)))),
        y: ymax,
        z: origin
            .z
            .saturating_add(cell_z_mm.saturating_mul(i32::from(nz.saturating_sub(1)))),
    };
    let local = AabbMm::new(
        IVec3 {
            x: origin.x,
            y: ymin,
            z: origin.z,
        },
        max,
    );
    let mut out: Option<AabbMm> = None;
    for c in corners(local, pose) {
        out = Some(match out {
            None => AabbMm::from_point(c),
            Some(b) => b.union(AabbMm::from_point(c)),
        });
    }
    out.unwrap_or(local)
}

pub(crate) fn terrain_contact(
    terrain: Shape,
    terrain_pose: PoseMm,
    other: Shape,
    other_pose: PoseMm,
    flip: bool,
) -> Result<Option<QuantizedContact>, GeomError> {
    match terrain {
        Shape::TriangleMesh { tris, len } => {
            mesh_contact(tris, len, terrain_pose, other, other_pose, flip)
        }
        Shape::Heightfield {
            origin,
            cell_x_mm,
            cell_z_mm,
            nx,
            nz,
            samples,
        } => heightfield_contact(
            origin,
            cell_x_mm,
            cell_z_mm,
            nx,
            nz,
            samples,
            terrain_pose,
            other,
            other_pose,
            flip,
        ),
        _ => Err(GeomError::Unsupported),
    }
}

fn mesh_contact(
    tris: [[IVec3; 3]; MAX_MESH_TRIANGLES],
    len: u8,
    pose: PoseMm,
    other: Shape,
    other_pose: PoseMm,
    flip: bool,
) -> Result<Option<QuantizedContact>, GeomError> {
    if len == 0 || usize::from(len) > MAX_MESH_TRIANGLES {
        return Err(GeomError::Malformed);
    }
    let other_bounds = bounds(other, other_pose)?;
    let samples = sample_points(other, other_pose)?;
    let mut best: Option<QuantizedContact> = None;
    for (i, tri) in tris.iter().take(usize::from(len)).enumerate() {
        let w = [
            posed_point(tri[0], pose),
            posed_point(tri[1], pose),
            posed_point(tri[2], pose),
        ];
        let tb = AabbMm::from_point(w[0])
            .union(AabbMm::from_point(w[1]))
            .union(AabbMm::from_point(w[2]));
        if !tb.intersects(other_bounds) {
            continue;
        }
        for &p in &samples {
            let (q, n) = closest_on_triangle(p, w);
            let d = p.wrapping_sub(q);
            let dist2 = dot(d, d);
            let toward = dot(d, n);
            if toward > 0 && dist2 > 0 {
                continue;
            }
            let depth = isqrt(dist2).clamp(0, i64::from(i32::MAX)) as i32;
            let hit = QuantizedContact {
                point: q,
                normal: pack_normal(n, if flip { -1 } else { 1 }),
                depth_mm: depth,
                feature: i as u16,
            };
            if best.is_none_or(|old| hit.depth_mm > old.depth_mm) {
                best = Some(hit);
            }
        }
    }
    Ok(best)
}

#[allow(clippy::too_many_arguments)]
fn heightfield_contact(
    origin: IVec3,
    cell_x_mm: i32,
    cell_z_mm: i32,
    nx: u8,
    nz: u8,
    samples: [i32; MAX_HEIGHTFIELD_SAMPLES],
    pose: PoseMm,
    other: Shape,
    other_pose: PoseMm,
    flip: bool,
) -> Result<Option<QuantizedContact>, GeomError> {
    if cell_x_mm <= 0 || cell_z_mm <= 0 {
        return Err(GeomError::Malformed);
    }
    let pts = sample_points(other, other_pose)?;
    let mut best: Option<QuantizedContact> = None;
    for (i, &world) in pts.iter().enumerate() {
        let local = to_local(world, pose);
        let Some((height, n_local)) =
            sample_field(origin, cell_x_mm, cell_z_mm, nx, nz, &samples, local)
        else {
            continue;
        };
        let depth = height.saturating_sub(local.y);
        if depth < 0 {
            continue;
        }
        let n_world = rotate(n_local, pose.yaw, pose.pitch, pose.roll);
        let point = posed_point(
            IVec3 {
                x: local.x,
                y: height,
                z: local.z,
            },
            pose,
        );
        let hit = QuantizedContact {
            point,
            normal: pack_normal(n_world, if flip { -1 } else { 1 }),
            depth_mm: depth,
            feature: i as u16,
        };
        if best.is_none_or(|old| hit.depth_mm > old.depth_mm) {
            best = Some(hit);
        }
    }
    Ok(best)
}

pub(crate) fn heightfield_normal_at(
    shape: Shape,
    pose: PoseMm,
    world: IVec3,
) -> Option<(i16, i16, i16)> {
    let Shape::Heightfield {
        origin,
        cell_x_mm,
        cell_z_mm,
        nx,
        nz,
        samples,
    } = shape
    else {
        return None;
    };
    let local = to_local(world, pose);
    let (_, n_local) = sample_field(origin, cell_x_mm, cell_z_mm, nx, nz, &samples, local)?;
    let n_world = rotate(n_local, pose.yaw, pose.pitch, pose.roll);
    Some(pack_normal(n_world, 1))
}

pub(crate) fn heightfield_ray(
    origin: IVec3,
    dir: IVec3,
    shape: Shape,
    pose: PoseMm,
) -> Option<(i64, i64)> {
    let Shape::Heightfield {
        origin: field_origin,
        cell_x_mm,
        cell_z_mm,
        nx,
        nz,
        samples,
    } = shape
    else {
        return None;
    };
    const STEPS: i64 = 32;
    let start = to_local(origin, pose);
    let end = to_local(origin.wrapping_add(dir), pose);
    let delta = end.wrapping_sub(start);
    if let Some((h, _)) = sample_field(field_origin, cell_x_mm, cell_z_mm, nx, nz, &samples, start)
    {
        if start.y <= h {
            return Some((0, 1));
        }
    }
    let mut prev_above = true;
    for i in 1..=STEPS {
        let p = IVec3 {
            x: (i64::from(start.x) + i64::from(delta.x) * i / STEPS) as i32,
            y: (i64::from(start.y) + i64::from(delta.y) * i / STEPS) as i32,
            z: (i64::from(start.z) + i64::from(delta.z) * i / STEPS) as i32,
        };
        let Some((h, _)) = sample_field(field_origin, cell_x_mm, cell_z_mm, nx, nz, &samples, p)
        else {
            prev_above = true;
            continue;
        };
        let below = p.y <= h;
        if below && prev_above {
            return Some((i, STEPS));
        }
        prev_above = !below;
    }
    None
}

fn sample_field(
    origin: IVec3,
    cell_x_mm: i32,
    cell_z_mm: i32,
    nx: u8,
    nz: u8,
    samples: &[i32; MAX_HEIGHTFIELD_SAMPLES],
    local: IVec3,
) -> Option<(i32, IVec3)> {
    let dx = local.x.saturating_sub(origin.x);
    let dz = local.z.saturating_sub(origin.z);
    if dx < 0 || dz < 0 {
        return None;
    }
    let nx_n = i32::from(nx);
    let nz_n = i32::from(nz);
    let max_x = cell_x_mm.saturating_mul(nx_n.saturating_sub(1));
    let max_z = cell_z_mm.saturating_mul(nz_n.saturating_sub(1));
    if dx > max_x || dz > max_z {
        return None;
    }
    let ix = (dx / cell_x_mm).min(nx_n.saturating_sub(2)).max(0);
    let iz = (dz / cell_z_mm).min(nz_n.saturating_sub(2)).max(0);
    let fx = dx.saturating_sub(ix.saturating_mul(cell_x_mm));
    let fz = dz.saturating_sub(iz.saturating_mul(cell_z_mm));
    let h00 = height(samples, nx, ix, iz)?;
    let h10 = height(samples, nx, ix + 1, iz)?;
    let h01 = height(samples, nx, ix, iz + 1)?;
    let h11 = height(samples, nx, ix + 1, iz + 1)?;
    let a = lerp(h00, h10, fx, cell_x_mm);
    let b = lerp(h01, h11, fx, cell_x_mm);
    let h = lerp(a, b, fz, cell_z_mm);
    let n = IVec3 {
        x: (h00.saturating_sub(h10)).saturating_mul(cell_z_mm),
        y: cell_x_mm.saturating_mul(cell_z_mm),
        z: (h00.saturating_sub(h01)).saturating_mul(cell_x_mm),
    };
    Some((h, n))
}

fn height(samples: &[i32; MAX_HEIGHTFIELD_SAMPLES], nx: u8, ix: i32, iz: i32) -> Option<i32> {
    let i = (iz as usize)
        .saturating_mul(usize::from(nx))
        .saturating_add(ix as usize);
    samples.get(i).copied()
}

fn lerp(a: i32, b: i32, t: i32, den: i32) -> i32 {
    if den <= 0 {
        return a;
    }
    let t = t.clamp(0, den);
    (i64::from(a) + (i64::from(b) - i64::from(a)) * i64::from(t) / i64::from(den)) as i32
}

fn sample_points(shape: Shape, pose: PoseMm) -> Result<Vec<IVec3>, GeomError> {
    match shape {
        Shape::OrientedBox { local } => Ok(corners(local, pose).to_vec()),
        Shape::Sphere {
            local_center,
            radius_mm,
        } => {
            let c = posed_point(local_center, pose);
            Ok(vec![
                c,
                IVec3 {
                    x: c.x,
                    y: c.y.saturating_sub(radius_mm),
                    z: c.z,
                },
            ])
        }
        Shape::Capsule {
            local_center,
            radius_mm,
            half_height_mm,
        } => {
            let c = posed_point(local_center, pose);
            let d = rotate(
                IVec3 {
                    x: 0,
                    y: half_height_mm,
                    z: 0,
                },
                pose.yaw,
                pose.pitch,
                pose.roll,
            );
            let p0 = c.wrapping_sub(d);
            let p1 = c.wrapping_add(d);
            Ok(vec![
                p0,
                c,
                p1,
                IVec3 {
                    x: p0.x,
                    y: p0.y.saturating_sub(radius_mm),
                    z: p0.z,
                },
            ])
        }
        Shape::Convex { vertices, len } => Ok(vertices
            .iter()
            .take(usize::from(len))
            .map(|&v| posed_point(v, pose))
            .collect()),
        Shape::Compound { parts, len } => {
            let mut out = Vec::new();
            for part in parts.iter().take(usize::from(len)) {
                let child = compose(pose, part.local_pose);
                out.extend(sample_points(part.shape.as_shape(), child)?);
            }
            Ok(out)
        }
        Shape::TriangleMesh { .. } | Shape::Heightfield { .. } => Err(GeomError::Unsupported),
    }
}

fn compose(parent: PoseMm, local: PoseMm) -> PoseMm {
    let t = posed_point(local.translation(), parent);
    PoseMm {
        x: klotho_core::Mm(t.x),
        y: klotho_core::Mm(t.y),
        z: klotho_core::Mm(t.z),
        yaw: klotho_core::YawMd(parent.yaw.0.wrapping_add(local.yaw.0)),
        pitch: klotho_core::YawMd(parent.pitch.0.wrapping_add(local.pitch.0)),
        roll: klotho_core::YawMd(parent.roll.0.wrapping_add(local.roll.0)),
    }
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

fn closest_on_triangle(p: IVec3, tri: [IVec3; 3]) -> (IVec3, IVec3) {
    let a = tri[0];
    let b = tri[1];
    let c = tri[2];
    let ab = b.wrapping_sub(a);
    let ac = c.wrapping_sub(a);
    let n = cross(ab, ac);
    let q_ab = closest_on_segment(p, a, b);
    let q_bc = closest_on_segment(p, b, c);
    let q_ca = closest_on_segment(p, c, a);
    let mut best = q_ab;
    let mut best_d = dot(p.wrapping_sub(q_ab), p.wrapping_sub(q_ab));
    for q in [q_bc, q_ca] {
        let d = dot(p.wrapping_sub(q), p.wrapping_sub(q));
        if d < best_d {
            best_d = d;
            best = q;
        }
    }
    (best, n)
}

fn closest_on_segment(p: IVec3, a: IVec3, b: IVec3) -> IVec3 {
    let ab = b.wrapping_sub(a);
    let den = dot(ab, ab);
    if den <= 0 {
        return a;
    }
    let t = dot(p.wrapping_sub(a), ab).clamp(0, den);
    IVec3 {
        x: a.x.wrapping_add(((i64::from(ab.x) * t) / den) as i32),
        y: a.y.wrapping_add(((i64::from(ab.y) * t) / den) as i32),
        z: a.z.wrapping_add(((i64::from(ab.z) * t) / den) as i32),
    }
}

fn to_local(world: IVec3, pose: PoseMm) -> IVec3 {
    let d = world.wrapping_sub(pose.translation());
    let (ax, ay, az) = klotho_core::rotation_axes(pose.yaw, pose.pitch, pose.roll);
    IVec3 {
        x: axis(d, ax),
        y: axis(d, ay),
        z: axis(d, az),
    }
}

fn axis(v: IVec3, a: IVec3) -> i32 {
    ((i64::from(v.x) * i64::from(a.x)
        + i64::from(v.y) * i64::from(a.y)
        + i64::from(v.z) * i64::from(a.z))
        >> 16) as i32
}

fn cross(a: IVec3, b: IVec3) -> IVec3 {
    IVec3 {
        x: (i64::from(a.y) * i64::from(b.z) - i64::from(a.z) * i64::from(b.y))
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        y: (i64::from(a.z) * i64::from(b.x) - i64::from(a.x) * i64::from(b.z))
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        z: (i64::from(a.x) * i64::from(b.y) - i64::from(a.y) * i64::from(b.x))
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    }
}

fn dot(a: IVec3, b: IVec3) -> i64 {
    i64::from(a.x) * i64::from(b.x)
        + i64::from(a.y) * i64::from(b.y)
        + i64::from(a.z) * i64::from(b.z)
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

fn pack_normal(n: IVec3, sign: i64) -> (i16, i16, i16) {
    let mag = isqrt(dot(n, n)).max(1);
    let s = 32_767i64 * sign;
    (
        ((i64::from(n.x) * s) / mag).clamp(-32_767, 32_767) as i16,
        ((i64::from(n.y) * s) / mag).clamp(-32_767, 32_767) as i16,
        ((i64::from(n.z) * s) / mag).clamp(-32_767, 32_767) as i16,
    )
}
