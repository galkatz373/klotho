//! Sequential positional correction on oriented boxes. Contacts rebuilt each substep.

use std::collections::BTreeSet;

use klotho_commit::{BodyDelta, Proposal};
use klotho_core::{
    AabbMm, BlobId, HullWitness, LocusKind, NO_ISLAND, PoseMm, ShapeKind, Sigil, Support, Vel3,
    VelFx, YawMd,
};
use klotho_geom::{Shape, bounds, contact};
use klotho_world::WorldView;

use crate::quant::{pose_and_residual, vel3};

const SUBSTEPS: u32 = 8;
const ITERS: u32 = 8;
const GRAVITY_MM_PER_TICK2: f32 = 2.725;
const INV_MASS: f32 = 1.0;

struct Body {
    sigil: Sigil,
    local: AabbMm,
    hull: BlobId,
    x: [f32; 3],
    v: [f32; 3],
    yaw: YawMd,
    pitch: YawMd,
    roll: YawMd,
    yaw_rate: i32,
    pitch_rate: i32,
    roll_rate: i32,
    prev: PoseMm,
}

struct Contact {
    a: usize,
    b: Option<usize>,
    static_local: Option<AabbMm>,
    static_pose: Option<PoseMm>,
}

struct Occupancy {
    local: AabbMm,
    pose: PoseMm,
}

/// One island solve. Residuals are per emitted pose (max-axis mm).
pub struct SolveOut {
    /// Zero or one atomic `PhysIsland` proposal.
    pub proposals: Vec<Proposal>,
    /// [`crate::METRIC_QUANT_RESIDUAL_MM`] samples.
    pub residuals_mm: Vec<f32>,
    /// Bodies whose f32 output was non-finite and was therefore discarded.
    ///
    /// A non-finite value must never be converted into an apparently valid
    /// integer body delta (in particular, never into an origin pose).
    pub rejected_non_finite: Vec<Sigil>,
}

/// Solve every Relic in `island`. Attached children and Actors are skipped.
#[must_use]
pub fn solve_island(island: u16, view: &WorldView<'_>) -> SolveOut {
    if island == NO_ISLAND {
        return SolveOut {
            proposals: Vec::new(),
            residuals_mm: Vec::new(),
            rejected_non_finite: Vec::new(),
        };
    }
    let members = collect_members(island, view);
    let mut bodies = collect_bodies(island, view);
    if bodies.is_empty() {
        return SolveOut {
            proposals: Vec::new(),
            residuals_mm: Vec::new(),
            rejected_non_finite: Vec::new(),
        };
    }
    let statics = collect_statics(view, &bodies);
    let dt = 1.0 / SUBSTEPS as f32;
    let mut last_support: Vec<Option<Support>> = vec![None; bodies.len()];
    for _ in 0..SUBSTEPS {
        for b in &mut bodies {
            b.v[1] -= GRAVITY_MM_PER_TICK2 * dt;
            b.x[0] += b.v[0] * dt;
            b.x[1] += b.v[1] * dt;
            b.x[2] += b.v[2] * dt;
        }
        let x0: Vec<[f32; 3]> = bodies.iter().map(|b| b.x).collect();
        let contacts = build_contacts(&bodies, &statics);
        for _ in 0..ITERS {
            for c in &contacts {
                apply_contact(&mut bodies, c);
            }
        }
        for (i, b) in bodies.iter_mut().enumerate() {
            if dt > 0.0 {
                b.v[0] = (b.x[0] - x0[i][0]) / dt;
                b.v[1] = (b.x[1] - x0[i][1]) / dt;
                b.v[2] = (b.x[2] - x0[i][2]) / dt;
            }
        }
        fill_support(&bodies, &contacts, &mut last_support);
    }
    emit(island, members, view, &bodies, &last_support)
}

fn collect_members(island: u16, view: &WorldView<'_>) -> Vec<Sigil> {
    let mut members: Vec<Sigil> = view
        .loci()
        .filter(|&s| view.island(s).map(|(id, _)| id) == Some(island))
        .collect();
    members.sort_unstable();
    members
}

fn collect_bodies(island: u16, view: &WorldView<'_>) -> Vec<Body> {
    let mut out = Vec::new();
    for s in view.loci() {
        if view.island(s).map(|(id, _)| id) != Some(island) {
            continue;
        }
        if s.kind() != Some(LocusKind::Relic) {
            continue;
        }
        if view.attach_parent(s).is_some() {
            continue;
        }
        let Some(pose) = view.pose(s) else {
            continue;
        };
        let Some(local) = view.hull(s) else {
            continue;
        };
        let (vel, yaw_rate) = view.vel(s).unwrap_or((Vel3::ZERO, 0));
        let (yaw_rate, pitch_rate, roll_rate) = view.rates(s).unwrap_or((yaw_rate, 0, 0));
        let scale = VelFx::SCALE as f32;
        let mut v = [
            vel.x.0 as f32 / scale,
            vel.y.0 as f32 / scale,
            vel.z.0 as f32 / scale,
        ];
        if let Some(req) = view.phys_req(s) {
            // One-shot Δv (mm/tick). Kernel clears the column on admit.
            v[0] += req.lin.x as f32;
            v[1] += req.lin.y as f32;
            v[2] += req.lin.z as f32;
        }
        // Steer writes PHYS_REQ on the driver; only the Relic parent is a body.
        for child in view.loci() {
            if view.attach_parent(child) != Some(s) {
                continue;
            }
            if let Some(req) = view.phys_req(child) {
                v[0] += req.lin.x as f32;
                v[1] += req.lin.y as f32;
                v[2] += req.lin.z as f32;
            }
        }
        out.push(Body {
            sigil: s,
            local,
            hull: view.hull_id(s).unwrap_or(BlobId::ZERO),
            x: [pose.x.0 as f32, pose.y.0 as f32, pose.z.0 as f32],
            v,
            yaw: pose.yaw,
            pitch: pose.pitch,
            roll: pose.roll,
            yaw_rate,
            pitch_rate,
            roll_rate,
            prev: pose,
        });
    }
    out.sort_by_key(|b| b.sigil);
    out
}

fn collect_statics(view: &WorldView<'_>, bodies: &[Body]) -> Vec<Occupancy> {
    let mut skip: BTreeSet<Sigil> = BTreeSet::new();
    for b in bodies {
        skip.insert(b.sigil);
    }
    let mut out = Vec::new();
    let mut seen: BTreeSet<Sigil> = BTreeSet::new();
    for b in bodies {
        let Some(pose) = trunc_pose(b) else {
            continue;
        };
        let Ok(shape) = Shape::oriented_box(b.local) else {
            continue;
        };
        let Ok(aabb) = bounds(shape, pose) else {
            continue;
        };
        for s in view.space_candidates(aabb, false) {
            if skip.contains(&s) || !seen.insert(s) {
                continue;
            }
            if !is_occupancy(view, s) {
                continue;
            }
            let Some(local) = view.hull(s) else {
                continue;
            };
            let Some(pose) = view.pose(s) else {
                continue;
            };
            out.push(Occupancy { local, pose });
        }
    }
    out
}

fn is_occupancy(view: &WorldView<'_>, s: Sigil) -> bool {
    s.kind() == Some(LocusKind::Place) || view.opaque_closed(s)
}

fn build_contacts(bodies: &[Body], statics: &[Occupancy]) -> Vec<Contact> {
    let mut out = Vec::new();
    for i in 0..bodies.len() {
        for j in (i + 1)..bodies.len() {
            if geom_overlap(&bodies[i], Some(&bodies[j]), None) {
                out.push(Contact {
                    a: i,
                    b: Some(j),
                    static_local: None,
                    static_pose: None,
                });
            }
        }
        for st in statics {
            if geom_overlap(&bodies[i], None, Some(st)) {
                out.push(Contact {
                    a: i,
                    b: None,
                    static_local: Some(st.local),
                    static_pose: Some(st.pose),
                });
            }
        }
    }
    out
}

fn trunc_pose(b: &Body) -> Option<PoseMm> {
    pose_and_residual(b.x[0], b.x[1], b.x[2], b.yaw, b.pitch, b.roll).map(|(p, _)| p)
}

fn geom_overlap(a: &Body, b: Option<&Body>, st: Option<&Occupancy>) -> bool {
    let Some(pa) = trunc_pose(a) else {
        return false;
    };
    let Ok(sa) = Shape::oriented_box(a.local) else {
        return false;
    };
    let (sb, pb) = if let Some(other) = b {
        let Some(p) = trunc_pose(other) else {
            return false;
        };
        let Ok(s) = Shape::oriented_box(other.local) else {
            return false;
        };
        (s, p)
    } else if let Some(occ) = st {
        let Ok(s) = Shape::oriented_box(occ.local) else {
            return false;
        };
        (s, occ.pose)
    } else {
        return false;
    };
    contact(sa, pa, sb, pb).ok().flatten().is_some()
}

fn apply_contact(bodies: &mut [Body], c: &Contact) {
    let Some((n, depth)) = contact_normal_depth(bodies, c) else {
        return;
    };
    if depth <= 0.0 {
        return;
    }
    let wa = INV_MASS;
    let wb = if c.b.is_some() { INV_MASS } else { 0.0 };
    let w = wa + wb;
    if w <= 0.0 {
        return;
    }
    let dlambda = depth / w;
    let corr = [
        wa * dlambda * n[0],
        wa * dlambda * n[1],
        wa * dlambda * n[2],
    ];
    let ai = c.a;
    bodies[ai].x[0] += corr[0];
    bodies[ai].x[1] += corr[1];
    bodies[ai].x[2] += corr[2];
    if let Some(j) = c.b {
        bodies[j].x[0] -= wb * dlambda * n[0];
        bodies[j].x[1] -= wb * dlambda * n[1];
        bodies[j].x[2] -= wb * dlambda * n[2];
    }
}

fn contact_normal_depth(bodies: &[Body], c: &Contact) -> Option<([f32; 3], f32)> {
    let pa = trunc_pose(&bodies[c.a])?;
    let sa = Shape::oriented_box(bodies[c.a].local).ok()?;
    let (sb, pb) = if let Some(j) = c.b {
        (
            Shape::oriented_box(bodies[j].local).ok()?,
            trunc_pose(&bodies[j])?,
        )
    } else {
        let local = c.static_local?;
        let pose = c.static_pose?;
        (Shape::oriented_box(local).ok()?, pose)
    };
    let hit = contact(sa, pa, sb, pb).ok().flatten()?;
    let s = 32767.0;
    let n = [
        f32::from(hit.normal.0) / s,
        f32::from(hit.normal.1) / s,
        f32::from(hit.normal.2) / s,
    ];
    if !n.into_iter().all(f32::is_finite) {
        return None;
    }
    Some((n, hit.depth_mm as f32))
}

fn fill_support(bodies: &[Body], contacts: &[Contact], out: &mut [Option<Support>]) {
    for c in contacts {
        let Some((n, depth)) = contact_normal_depth(bodies, c) else {
            continue;
        };
        if n[1] <= 0.0 {
            continue;
        }
        let Some(packed) = pack_support(n, depth) else {
            continue;
        };
        match out[c.a] {
            Some((_, ny, _, d)) if ny >= packed.1 && d >= packed.3 => {}
            _ => out[c.a] = Some(packed),
        }
        if let Some(j) = c.b {
            if n[1] < 0.0 {
                continue;
            }
            let n_b = [-n[0], -n[1], -n[2]];
            if n_b[1] <= 0.0 {
                continue;
            }
            let Some(packed_b) = pack_support(n_b, depth) else {
                continue;
            };
            match out[j] {
                Some((_, ny, _, d)) if ny >= packed_b.1 && d >= packed_b.3 => {}
                _ => out[j] = Some(packed_b),
            }
        }
    }
}

fn pack_support(n: [f32; 3], depth: f32) -> Option<Support> {
    // Support is optional advisory contact metadata. A non-finite support
    // normal/depth is omitted; pose/velocity are independently quantized and
    // the kernel still re-derives gameplay-visible swept overlap (K24).
    if !n.into_iter().all(f32::is_finite) || !depth.is_finite() {
        return None;
    }
    let s = 32767.0;
    Some((
        (n[0] * s).round().clamp(-32767.0, 32767.0) as i16,
        (n[1] * s).round().clamp(-32767.0, 32767.0) as i16,
        (n[2] * s).round().clamp(-32767.0, 32767.0) as i16,
        crate::quant::trunc_mm(depth)?,
    ))
}

fn emit(
    island: u16,
    members: Vec<Sigil>,
    view: &WorldView<'_>,
    bodies: &[Body],
    support: &[Option<Support>],
) -> SolveOut {
    let mut deltas = Vec::with_capacity(bodies.len());
    let mut residuals_mm = Vec::with_capacity(bodies.len());
    let mut rejected_non_finite = Vec::new();
    for (i, b) in bodies.iter().enumerate() {
        let Some((pose, residual)) =
            pose_and_residual(b.x[0], b.x[1], b.x[2], b.yaw, b.pitch, b.roll)
        else {
            rejected_non_finite.push(b.sigil);
            continue;
        };
        let Some(vel) = vel3(b.v[0], b.v[1], b.v[2]) else {
            rejected_non_finite.push(b.sigil);
            continue;
        };
        let hint = hits_closed_oriented(view, b.sigil, b.local, b.prev, pose);
        let mut witness = HullWitness::new(b.sigil, pose, hint);
        witness.epoch = view.epoch();
        witness.shape = ShapeKind::OrientedBox;
        deltas.push(BodyDelta {
            mover: b.sigil,
            pose,
            vel,
            yaw_rate: b.yaw_rate,
            pitch_rate: b.pitch_rate,
            roll_rate: b.roll_rate,
            sleep_ticks: 0,
            hull: b.hull,
            witness,
            support: support[i],
        });
        residuals_mm.push(residual);
    }
    let proposals = if rejected_non_finite.is_empty() && !deltas.is_empty() {
        vec![Proposal::PhysIsland {
            epoch: view.epoch(),
            tick: view.tick(),
            island,
            members,
            bodies: deltas,
            contacts: Vec::new(),
            constraints: Vec::new(),
            breaks: Vec::new(),
        }]
    } else {
        Vec::new()
    };
    SolveOut {
        proposals,
        residuals_mm,
        rejected_non_finite,
    }
}

fn hits_closed_oriented(
    view: &WorldView<'_>,
    mover: Sigil,
    local: AabbMm,
    prev: PoseMm,
    pose: PoseMm,
) -> bool {
    let Ok(shape) = Shape::oriented_box(local) else {
        return false;
    };
    let Ok(a) = bounds(shape, prev) else {
        return false;
    };
    let Ok(b) = bounds(shape, pose) else {
        return false;
    };
    let swept = a.swept_union(b);
    for o in view.space_candidates(swept, true) {
        if o == mover {
            continue;
        }
        if let Some(h) = view.posed_hull(o) {
            if h.intersects(swept) {
                return true;
            }
        }
    }
    false
}
