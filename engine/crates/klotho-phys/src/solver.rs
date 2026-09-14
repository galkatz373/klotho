//! Sequential positional correction on AABB rigid bodies. Contacts rebuilt each substep.

use std::collections::BTreeSet;

use klotho_commit::{BodyDelta, Proposal};
use klotho_core::{
    AabbMm, BlobId, HullWitness, IVec3, LocusKind, NO_ISLAND, PoseMm, Sigil, Support, Vel3, VelFx,
    YawMd,
};
use klotho_world::{WorldView, world_aabb};

use crate::quant::{pose_and_residual, vel3};

const SUBSTEPS: u32 = 8;
const ITERS: u32 = 8;
const GRAVITY_MM_PER_TICK2: f32 = 2.725;
const INV_MASS: f32 = 1.0;

#[derive(Clone, Copy)]
struct AabbF {
    min: [f32; 3],
    max: [f32; 3],
}

impl AabbF {
    fn from_local(local: AabbMm, x: [f32; 3]) -> Self {
        Self {
            min: [
                local.min.x as f32 + x[0],
                local.min.y as f32 + x[1],
                local.min.z as f32 + x[2],
            ],
            max: [
                local.max.x as f32 + x[0],
                local.max.y as f32 + x[1],
                local.max.z as f32 + x[2],
            ],
        }
    }

    fn centre(self) -> [f32; 3] {
        [
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        ]
    }

    fn query_mm(self) -> AabbMm {
        AabbMm {
            min: IVec3 {
                x: self.min[0].floor() as i32 - 1,
                y: self.min[1].floor() as i32 - 1,
                z: self.min[2].floor() as i32 - 1,
            },
            max: IVec3 {
                x: self.max[0].ceil() as i32 + 1,
                y: self.max[1].ceil() as i32 + 1,
                z: self.max[2].ceil() as i32 + 1,
            },
        }
    }
}

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
    static_aabb: Option<AabbF>,
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

fn collect_statics(view: &WorldView<'_>, bodies: &[Body]) -> Vec<AabbF> {
    let mut skip: BTreeSet<Sigil> = BTreeSet::new();
    for b in bodies {
        skip.insert(b.sigil);
    }
    let mut out = Vec::new();
    let mut seen: BTreeSet<Sigil> = BTreeSet::new();
    for b in bodies {
        let aabb = AabbF::from_local(b.local, b.x).query_mm();
        for s in view.space_candidates(aabb, false) {
            if skip.contains(&s) || !seen.insert(s) {
                continue;
            }
            if !is_occupancy(view, s) {
                continue;
            }
            let Some(h) = view.posed_hull(s) else {
                continue;
            };
            out.push(AabbF {
                min: [h.min.x as f32, h.min.y as f32, h.min.z as f32],
                max: [h.max.x as f32, h.max.y as f32, h.max.z as f32],
            });
        }
    }
    out
}

fn is_occupancy(view: &WorldView<'_>, s: Sigil) -> bool {
    s.kind() == Some(LocusKind::Place) || view.opaque_closed(s)
}

fn build_contacts(bodies: &[Body], statics: &[AabbF]) -> Vec<Contact> {
    let mut out = Vec::new();
    for i in 0..bodies.len() {
        for j in (i + 1)..bodies.len() {
            let a = AabbF::from_local(bodies[i].local, bodies[i].x);
            let b = AabbF::from_local(bodies[j].local, bodies[j].x);
            if overlap_n(a, b).is_some() {
                out.push(Contact {
                    a: i,
                    b: Some(j),
                    static_aabb: None,
                });
            }
        }
        let a = AabbF::from_local(bodies[i].local, bodies[i].x);
        for &st in statics {
            if overlap_n(a, st).is_some() {
                out.push(Contact {
                    a: i,
                    b: None,
                    static_aabb: Some(st),
                });
            }
        }
    }
    out
}

fn overlap_n(a: AabbF, b: AabbF) -> Option<([f32; 3], f32)> {
    let ox = a.max[0].min(b.max[0]) - a.min[0].max(b.min[0]);
    let oy = a.max[1].min(b.max[1]) - a.min[1].max(b.min[1]);
    let oz = a.max[2].min(b.max[2]) - a.min[2].max(b.min[2]);
    if ox < 0.0 || oy < 0.0 || oz < 0.0 {
        return None;
    }
    let ac = a.centre();
    let bc = b.centre();
    if ox <= oy && ox <= oz {
        let n = if ac[0] >= bc[0] { 1.0 } else { -1.0 };
        Some(([n, 0.0, 0.0], ox))
    } else if oy <= oz {
        let n = if ac[1] >= bc[1] { 1.0 } else { -1.0 };
        Some(([0.0, n, 0.0], oy))
    } else {
        let n = if ac[2] >= bc[2] { 1.0 } else { -1.0 };
        Some(([0.0, 0.0, n], oz))
    }
}

fn apply_contact(bodies: &mut [Body], c: &Contact) {
    let a_aabb = AabbF::from_local(bodies[c.a].local, bodies[c.a].x);
    let b_aabb = match (c.b, c.static_aabb) {
        (Some(j), _) => AabbF::from_local(bodies[j].local, bodies[j].x),
        (None, Some(st)) => st,
        (None, None) => return,
    };
    let Some((n, depth)) = overlap_n(a_aabb, b_aabb) else {
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

fn fill_support(bodies: &[Body], contacts: &[Contact], out: &mut [Option<Support>]) {
    for c in contacts {
        let a_aabb = AabbF::from_local(bodies[c.a].local, bodies[c.a].x);
        let b_aabb = match c.b {
            Some(j) => AabbF::from_local(bodies[j].local, bodies[j].x),
            None => match c.static_aabb {
                Some(s) => s,
                None => continue,
            },
        };
        let Some((n, depth)) = overlap_n(a_aabb, b_aabb) else {
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
        let from = world_aabb(b.local, b.prev.translation());
        let to = world_aabb(b.local, pose.translation());
        let hint = hits_closed(view, b.sigil, from.swept_union(to));
        deltas.push(BodyDelta {
            mover: b.sigil,
            pose,
            vel,
            yaw_rate: b.yaw_rate,
            pitch_rate: b.pitch_rate,
            roll_rate: b.roll_rate,
            sleep_ticks: 0,
            hull: b.hull,
            witness: HullWitness::new(b.sigil, pose, hint),
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

fn hits_closed(view: &WorldView<'_>, mover: Sigil, swept: AabbMm) -> bool {
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
