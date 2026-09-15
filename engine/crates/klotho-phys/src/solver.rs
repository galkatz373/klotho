//! Sequential positional correction on oriented boxes. Contacts rebuilt each substep.

use std::collections::BTreeSet;

use klotho_commit::{BodyDelta, Proposal};
use klotho_core::{
    AabbMm, BlobId, BodyMode, BodyPhysics, HullWitness, LocusKind, NO_ISLAND, PoseMm,
    SLEEP_AFTER_TICKS, ShapeKind, Sigil, Support, Vel3, VelFx, YawMd, rotate,
};
use klotho_geom::{bounds, contact, cooked_shape, manifold};
use klotho_world::WorldView;

use crate::joints::{Joint, apply_joint, emit_refs};
use crate::quant::{pose_and_residual, vel3};

const SUBSTEPS: u32 = 8;
const ITERS: u32 = 8;
const GRAVITY_MM_PER_TICK2: f32 = 2.725;
const MD_PER_RADIAN: f32 = 57_295.78;
const ANGULAR_POSITION_SCALE: f32 = 0.02;

pub(crate) struct Body {
    pub(crate) sigil: Sigil,
    local: AabbMm,
    hull: BlobId,
    kind: ShapeKind,
    pub(crate) x: [f32; 3],
    pub(crate) v: [f32; 3],
    pub(crate) angle_md: [f32; 3],
    pub(crate) omega_md: [f32; 3],
    pub(crate) inv_mass: f32,
    pub(crate) inv_inertia: [f32; 3],
    center_of_mass: klotho_core::IVec3,
    friction: f32,
    restitution: f32,
    prev: PoseMm,
    character: Option<klotho_core::CharacterPhysics>,
    pub(crate) vehicle: Option<klotho_core::VehiclePhysics>,
    pub(crate) drive: Option<klotho_core::VehicleDrive>,
    kinematic: bool,
    pub(crate) grounded: bool,
}

struct Contact {
    a: usize,
    b: Option<usize>,
    static_local: Option<AabbMm>,
    static_pose: Option<PoseMm>,
    static_kind: Option<ShapeKind>,
    static_friction: Option<f32>,
    static_restitution: Option<f32>,
}

pub(crate) struct Occupancy {
    pub(crate) sigil: Sigil,
    pub(crate) local: AabbMm,
    pub(crate) kind: ShapeKind,
    pub(crate) pose: PoseMm,
    pub(crate) friction: f32,
    pub(crate) restitution: f32,
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
    /// Driven actors whose bounded geometry query failed; no island is emitted.
    pub rejected_character_geometry: Vec<Sigil>,
    /// Bound vehicles whose wheel query overflowed; no island is emitted.
    pub rejected_vehicle_geometry: Vec<Sigil>,
}

/// Solve Relics and Canon-driven Actors in `island`; skip attached children.
#[must_use]
pub fn solve_island(island: u16, view: &WorldView<'_>) -> SolveOut {
    if island == NO_ISLAND {
        return SolveOut {
            proposals: Vec::new(),
            residuals_mm: Vec::new(),
            rejected_non_finite: Vec::new(),
            rejected_character_geometry: Vec::new(),
            rejected_vehicle_geometry: Vec::new(),
        };
    }
    let members = collect_members(island, view);
    let mut bodies = collect_bodies(island, view);
    if bodies.is_empty() {
        return SolveOut {
            proposals: Vec::new(),
            residuals_mm: Vec::new(),
            rejected_non_finite: Vec::new(),
            rejected_character_geometry: Vec::new(),
            rejected_vehicle_geometry: Vec::new(),
        };
    }
    let statics = collect_statics(view, &bodies);
    if prepare_characters(view, &mut bodies, &statics).is_err() {
        return SolveOut {
            proposals: Vec::new(),
            residuals_mm: Vec::new(),
            rejected_non_finite: Vec::new(),
            rejected_character_geometry: bodies
                .iter()
                .filter(|b| b.character.is_some())
                .map(|b| b.sigil)
                .collect(),
            rejected_vehicle_geometry: Vec::new(),
        };
    }
    if crate::vehicle::prepare_vehicles(&statics).is_err() {
        return SolveOut {
            proposals: Vec::new(),
            residuals_mm: Vec::new(),
            rejected_non_finite: Vec::new(),
            rejected_character_geometry: Vec::new(),
            rejected_vehicle_geometry: bodies
                .iter()
                .filter(|b| b.vehicle.is_some())
                .map(|b| b.sigil)
                .collect(),
        };
    }
    let mut joints = collect_joints(view, &bodies);
    let dt = 1.0 / SUBSTEPS as f32;
    let mut last_support: Vec<Option<Support>> = vec![None; bodies.len()];
    for _ in 0..SUBSTEPS {
        for b in &mut bodies {
            if !b.kinematic && !(b.character.is_some() && b.grounded) {
                b.v[1] -= GRAVITY_MM_PER_TICK2 * dt;
            }
        }
        if crate::vehicle::apply_vehicles(&mut bodies, &statics, dt, &mut last_support).is_err() {
            return SolveOut {
                proposals: Vec::new(),
                residuals_mm: Vec::new(),
                rejected_non_finite: Vec::new(),
                rejected_character_geometry: Vec::new(),
                rejected_vehicle_geometry: bodies
                    .iter()
                    .filter(|b| b.vehicle.is_some())
                    .map(|b| b.sigil)
                    .collect(),
            };
        }
        let impact_v: Vec<[f32; 3]> = bodies.iter().map(|b| b.v).collect();
        for b in &mut bodies {
            b.x[0] += b.v[0] * dt;
            b.x[1] += b.v[1] * dt;
            b.x[2] += b.v[2] * dt;
            for axis in 0..3 {
                b.angle_md[axis] += b.omega_md[axis] * dt;
            }
        }
        let contacts = build_contacts(&bodies, &statics);
        for _ in 0..ITERS {
            for c in &contacts {
                apply_contact(&mut bodies, c);
            }
            for joint in &mut joints {
                apply_joint(&mut bodies, joint);
            }
        }
        for c in &contacts {
            apply_velocity_contact(&mut bodies, c, &impact_v);
        }
        fill_support(&bodies, &contacts, &mut last_support);
    }
    emit(island, members, view, &bodies, &last_support, &joints)
}

fn prepare_characters(
    view: &WorldView<'_>,
    bodies: &mut [Body],
    statics: &[Occupancy],
) -> Result<(), klotho_geom::GeomError> {
    use klotho_core::IVec3;
    use klotho_geom::{CharacterObstacle, resolve_character};
    if bodies.iter().all(|b| b.character.is_none()) {
        return Ok(());
    }
    let mut obstacles = Vec::new();
    for s in statics {
        obstacles.push(CharacterObstacle {
            shape: cooked_shape(s.kind, s.local)?,
            pose: s.pose,
        });
    }
    let platforms: Vec<_> = bodies
        .iter()
        .filter(|b| b.kinematic)
        .map(|b| (b.sigil, b.local, b.kind, b.prev, b.v, b.omega_md))
        .collect();
    for body in bodies.iter_mut().filter(|b| b.character.is_some()) {
        let policy = body.character.unwrap();
        let shape = cooked_shape(body.kind, body.local)?;
        let drive = klotho_motion::Motion::drive(view, body.sigil)
            .ok_or(klotho_geom::GeomError::Malformed)?;
        let mut desire = drive.root;
        let mut character_obstacles = obstacles.clone();
        let mut carried = false;
        for &(_, local, kind, pose, v, omega) in &platforms {
            let platform = cooked_shape(kind, local)?;
            let probe = PoseMm {
                y: klotho_core::Mm(body.prev.y.0 - 3),
                ..body.prev
            };
            let riding = !carried
                && contact(shape, probe, platform, pose)?
                    .is_some_and(|h| h.normal.1 >= policy.slope_min_y);
            let mut future = pose;
            future.x.0 = future.x.0.saturating_add(v[0].floor() as i32);
            future.y.0 = future.y.0.saturating_add(v[1].floor() as i32);
            future.z.0 = future.z.0.saturating_add(v[2].floor() as i32);
            future.yaw.0 = future.yaw.0.saturating_add(omega[0].floor() as i32);
            future.pitch.0 = future.pitch.0.saturating_add(omega[1].floor() as i32);
            future.roll.0 = future.roll.0.saturating_add(omega[2].floor() as i32);
            if riding {
                carried = true;
                body.angle_md[0] += omega[0];
                let relative = body.prev.translation().wrapping_sub(pose.translation());
                let rotated = rotate(
                    relative,
                    YawMd(omega[0].floor() as i32),
                    YawMd(omega[1].floor() as i32),
                    YawMd(omega[2].floor() as i32),
                );
                desire = desire
                    .wrapping_add(rotated.wrapping_sub(relative))
                    .wrapping_add(future.translation().wrapping_sub(pose.translation()));
            }
            character_obstacles.push(CharacterObstacle {
                shape: platform,
                pose: future,
            });
        }
        let current = resolve_character(shape, body.prev, IVec3::ZERO, policy, &obstacles)?;
        body.grounded = current.support.is_some() || carried;
        let resolved = resolve_character(shape, body.prev, desire, policy, &character_obstacles)?;
        // Lift before horizontal integration so a stair path is up/over/down,
        // rather than a diagonal penetration into the riser.
        body.x[1] = resolved.pose.y.0 as f32;
        body.v[0] = (resolved.pose.x.0 - body.prev.x.0) as f32;
        body.v[2] = (resolved.pose.z.0 - body.prev.z.0) as f32;
        if body.grounded {
            body.v[1] = 0.0;
        }
        body.omega_md = [0.0; 3];
    }
    Ok(())
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
        if s.kind() != Some(LocusKind::Relic) && view.character_physics(s).is_none() {
            continue;
        }
        if view.attach_parent(s).is_some() {
            continue;
        }
        let physics = view.body_physics(s);
        if physics.mode == BodyMode::Static || !physics.is_valid() {
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
        let vehicle = view.vehicle_physics(s);
        let drive = view.vehicle_drive(s);
        if vehicle.is_none() {
            if let Some(req) = view.phys_req(s) {
                // One-shot Δv (mm/tick). Kernel clears the column on admit.
                v[0] += req.lin.x as f32;
                v[1] += req.lin.y as f32;
                v[2] += req.lin.z as f32;
            }
        }
        let mut omega_md = [yaw_rate as f32, pitch_rate as f32, roll_rate as f32];
        if vehicle.is_none() {
            if let Some(req) = view.phys_req(s) {
                omega_md[0] += req.ang.y as f32;
                omega_md[1] += req.ang.x as f32;
                omega_md[2] += req.ang.z as f32;
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
        }
        out.push(Body {
            sigil: s,
            local,
            hull: view.hull_id(s).unwrap_or(BlobId::ZERO),
            kind: physics.shape,
            x: [pose.x.0 as f32, pose.y.0 as f32, pose.z.0 as f32],
            v,
            angle_md: [pose.yaw.0 as f32, pose.pitch.0 as f32, pose.roll.0 as f32],
            omega_md,
            inv_mass: if physics.mode == BodyMode::Kinematic {
                0.0
            } else {
                mass_properties(local, physics).0
            },
            inv_inertia: if physics.mode == BodyMode::Kinematic || physics.character.is_some() {
                [0.0; 3]
            } else {
                mass_properties(local, physics).1
            },
            center_of_mass: physics.center_of_mass,
            friction: f32::from(physics.friction_permille) / 1_000.0,
            restitution: f32::from(physics.restitution_permille) / 1_000.0,
            prev: pose,
            character: view.character_physics(s),
            vehicle,
            drive,
            kinematic: physics.mode == BodyMode::Kinematic,
            grounded: false,
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
        let Ok(shape) = cooked_shape(b.kind, b.local) else {
            continue;
        };
        let Ok(aabb) = bounds(shape, pose) else {
            continue;
        };
        let mut future = pose;
        future.x.0 = future.x.0.saturating_add(b.v[0].floor() as i32);
        future.y.0 = future
            .y
            .0
            .saturating_add((b.v[1] - GRAVITY_MM_PER_TICK2).floor() as i32);
        future.z.0 = future.z.0.saturating_add(b.v[2].floor() as i32);
        let query = bounds(shape, future).map_or(aabb, |end| aabb.swept_union(end));
        for s in view.space_candidates(query, false) {
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
            let physics = view.body_physics(s);
            out.push(Occupancy {
                sigil: s,
                local,
                kind: physics.shape,
                pose,
                friction: f32::from(physics.friction_permille) / 1_000.0,
                restitution: f32::from(physics.restitution_permille) / 1_000.0,
            });
        }
    }
    // Character steps and vehicle wheel rays need occupancy beyond the linear AABB.
    for b in bodies
        .iter()
        .filter(|b| b.character.is_some() || b.vehicle.is_some())
    {
        let Ok(shape) = cooked_shape(b.kind, b.local) else {
            continue;
        };
        let Ok(mut query) = bounds(shape, b.prev) else {
            continue;
        };
        query.min.x = query.min.x.saturating_sub(2000);
        query.min.z = query.min.z.saturating_sub(2000);
        query.max.x = query.max.x.saturating_add(2000);
        query.max.z = query.max.z.saturating_add(2000);
        query.min.y = query.min.y.saturating_sub(1000);
        query.max.y = query.max.y.saturating_add(1000);
        let mut loci = view.space_candidates(query, false);
        loci.sort_unstable();
        for s in loci {
            if skip.contains(&s) || !seen.insert(s) || !is_occupancy(view, s) {
                continue;
            }
            if let (Some(local), Some(pose)) = (view.hull(s), view.pose(s)) {
                let p = view.body_physics(s);
                out.push(Occupancy {
                    sigil: s,
                    local,
                    kind: p.shape,
                    pose,
                    friction: f32::from(p.friction_permille) / 1000.0,
                    restitution: f32::from(p.restitution_permille) / 1000.0,
                });
            }
        }
    }
    if bodies
        .iter()
        .any(|b| b.character.is_some() || b.vehicle.is_some())
    {
        out.sort_by_key(|o| o.sigil);
    }
    out
}

fn is_occupancy(view: &WorldView<'_>, s: Sigil) -> bool {
    s.kind() == Some(LocusKind::Place)
        || view.opaque_closed(s)
        || view.body_physics(s).mode != BodyMode::Dynamic
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
                    static_kind: None,
                    static_friction: None,
                    static_restitution: None,
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
                    static_kind: Some(st.kind),
                    static_friction: Some(st.friction),
                    static_restitution: Some(st.restitution),
                });
            }
        }
    }
    out
}

fn trunc_pose(b: &Body) -> Option<PoseMm> {
    let attitude = quantized_attitude(b)?;
    pose_and_residual(
        b.x[0],
        b.x[1],
        b.x[2],
        attitude[0],
        attitude[1],
        attitude[2],
    )
    .map(|(p, _)| p)
}

fn quantized_attitude(b: &Body) -> Option<[YawMd; 3]> {
    let mut out = [YawMd::ZERO; 3];
    for (i, value) in b.angle_md.into_iter().enumerate() {
        if !value.is_finite() || value < i32::MIN as f32 || value > i32::MAX as f32 {
            return None;
        }
        out[i] = YawMd(value.floor() as i32);
    }
    Some(out)
}

fn mass_properties(local: AabbMm, physics: BodyPhysics) -> (f32, [f32; 3]) {
    let sx = (local.max.x - local.min.x).unsigned_abs().max(1) as f32;
    let sy = (local.max.y - local.min.y).unsigned_abs().max(1) as f32;
    let sz = (local.max.z - local.min.z).unsigned_abs().max(1) as f32;
    // Uniform canonical density. The common factor cancels in contact pairs;
    // retaining volume still gives different bodies different mass/inertia.
    let mass = if physics.mass_grams == 0 {
        (sx * sy * sz / 1_000.0).max(1.0)
    } else {
        physics.mass_grams as f32
    };
    let derived = [
        mass * (sy * sy + sz * sz) / 12.0,
        mass * (sx * sx + sz * sz) / 12.0,
        mass * (sx * sx + sy * sy) / 12.0,
    ];
    let inertia: [f32; 3] = core::array::from_fn(|i| {
        if physics.inertia_diag[i] == 0 {
            derived[i]
        } else {
            physics.inertia_diag[i] as f32
        }
    });
    (
        1.0 / mass,
        [1.0 / inertia[0], 1.0 / inertia[1], 1.0 / inertia[2]],
    )
}

fn geom_overlap(a: &Body, b: Option<&Body>, st: Option<&Occupancy>) -> bool {
    let Some(pa) = trunc_pose(a) else {
        return false;
    };
    let Ok(sa) = cooked_shape(a.kind, a.local) else {
        return false;
    };
    let (sb, pb) = if let Some(other) = b {
        let Some(p) = trunc_pose(other) else {
            return false;
        };
        let Ok(s) = cooked_shape(other.kind, other.local) else {
            return false;
        };
        (s, p)
    } else if let Some(occ) = st {
        let Ok(s) = cooked_shape(occ.kind, occ.local) else {
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
    if depth <= 0.0 || depth > 4_000.0 || !n.into_iter().all(f32::is_finite) {
        return;
    }
    let ai = c.a;
    let wa = bodies[ai].inv_mass;
    let wb = c.b.map_or(0.0, |j| bodies[j].inv_mass);
    let point = contact_point(bodies, c).unwrap_or(bodies[ai].x);
    let ra = sub3(point, body_center(&bodies[ai]));
    let rb =
        c.b.map(|j| sub3(point, body_center(&bodies[j])))
            .unwrap_or([0.0; 3]);
    let ang_a = if c.b.is_some() {
        angular_weight(ra, n, bodies[ai].inv_inertia)
    } else {
        0.0
    };
    let ang_b =
        c.b.map_or(0.0, |j| angular_weight(rb, n, bodies[j].inv_inertia));
    let w = wa + wb + ang_a + ang_b;
    if w <= 0.0 {
        return;
    }
    let dlambda = depth / w;
    let corr = [
        wa * dlambda * n[0],
        wa * dlambda * n[1],
        wa * dlambda * n[2],
    ];
    bodies[ai].x[0] += corr[0];
    bodies[ai].x[1] += corr[1];
    bodies[ai].x[2] += corr[2];
    // Flat static floors keep translation-only correction. Sloped occupancy
    // needs the contact lever arm so boxes can rest flush.
    if c.b.is_some() || n[1].abs() < 0.98 {
        apply_angular_position(&mut bodies[ai], ra, n, dlambda);
    }
    if let Some(j) = c.b {
        bodies[j].x[0] -= wb * dlambda * n[0];
        bodies[j].x[1] -= wb * dlambda * n[1];
        bodies[j].x[2] -= wb * dlambda * n[2];
        apply_angular_position(&mut bodies[j], rb, [-n[0], -n[1], -n[2]], dlambda);
    }
}

fn contact_point(bodies: &[Body], c: &Contact) -> Option<[f32; 3]> {
    let pa = trunc_pose(&bodies[c.a])?;
    let sa = cooked_shape(bodies[c.a].kind, bodies[c.a].local).ok()?;
    let (sb, pb) = if let Some(j) = c.b {
        (
            cooked_shape(bodies[j].kind, bodies[j].local).ok()?,
            trunc_pose(&bodies[j])?,
        )
    } else {
        (
            cooked_shape(c.static_kind?, c.static_local?).ok()?,
            c.static_pose?,
        )
    };
    let patch = manifold(sa, pa, sb, pb).ok().flatten()?;
    let (sum, n) = patch
        .as_slice()
        .iter()
        .fold(([0i64; 3], 0i64), |(mut sum, n), hit| {
            sum[0] += i64::from(hit.point.x);
            sum[1] += i64::from(hit.point.y);
            sum[2] += i64::from(hit.point.z);
            (sum, n + 1)
        });
    Some([
        sum[0] as f32 / n as f32,
        sum[1] as f32 / n as f32,
        sum[2] as f32 / n as f32,
    ])
}

fn angular_weight(r: [f32; 3], n: [f32; 3], inv_i: [f32; 3]) -> f32 {
    let rn = cross3(r, n);
    rn[0] * rn[0] * inv_i[0] + rn[1] * rn[1] * inv_i[1] + rn[2] * rn[2] * inv_i[2]
}

fn body_center(body: &Body) -> [f32; 3] {
    let offset = quantized_attitude(body)
        .map(|a| rotate(body.center_of_mass, a[0], a[1], a[2]))
        .unwrap_or(body.center_of_mass);
    [
        body.x[0] + offset.x as f32,
        body.x[1] + offset.y as f32,
        body.x[2] + offset.z as f32,
    ]
}

fn apply_angular_position(body: &mut Body, r: [f32; 3], n: [f32; 3], lambda: f32) {
    let torque = cross3(r, n);
    // Solver axes are x=pitch, y=yaw, z=roll; Projection stores yaw first.
    body.angle_md[0] +=
        torque[1] * body.inv_inertia[1] * lambda * MD_PER_RADIAN * ANGULAR_POSITION_SCALE;
    body.angle_md[1] +=
        torque[0] * body.inv_inertia[0] * lambda * MD_PER_RADIAN * ANGULAR_POSITION_SCALE;
    body.angle_md[2] +=
        torque[2] * body.inv_inertia[2] * lambda * MD_PER_RADIAN * ANGULAR_POSITION_SCALE;
    for angle in &mut body.angle_md {
        *angle = angle.clamp(-180_000.0, 180_000.0);
    }
}

fn apply_velocity_contact(bodies: &mut [Body], c: &Contact, impact_v: &[[f32; 3]]) {
    let Some((n, _)) = contact_normal_depth(bodies, c) else {
        return;
    };
    let ai = c.a;
    let bv = c.b.map_or([0.0; 3], |j| impact_v[j]);
    let rv = sub3(impact_v[ai], bv);
    let vn = dot3(rv, n);
    if vn >= 0.0 {
        return;
    }
    let wa = bodies[ai].inv_mass;
    let wb = c.b.map_or(0.0, |j| bodies[j].inv_mass);
    let denom = (wa + wb).max(f32::EPSILON);
    let (friction, restitution) = contact_material(bodies, c);
    let impulse_n = -(1.0 + restitution) * vn / denom;
    let tangent_v = sub3(rv, [n[0] * vn, n[1] * vn, n[2] * vn]);
    let speed = dot3(tangent_v, tangent_v).sqrt();
    let tangent = if speed <= f32::EPSILON {
        [0.0; 3]
    } else {
        [
            tangent_v[0] / speed,
            tangent_v[1] / speed,
            tangent_v[2] / speed,
        ]
    };
    let impulse_t = if speed <= f32::EPSILON {
        0.0
    } else {
        (-speed / denom).clamp(-friction * impulse_n, friction * impulse_n)
    };
    let impulse = [
        n[0] * impulse_n + tangent[0] * impulse_t,
        n[1] * impulse_n + tangent[1] * impulse_t,
        n[2] * impulse_n + tangent[2] * impulse_t,
    ];
    for (axis, impulse_axis) in impulse.into_iter().enumerate() {
        bodies[ai].v[axis] += impulse_axis * wa;
        if let Some(j) = c.b {
            bodies[j].v[axis] -= impulse_axis * wb;
        }
    }
    if let Some(j) = c.b {
        let point = contact_point(bodies, c).unwrap_or(body_center(&bodies[ai]));
        let ra = sub3(point, body_center(&bodies[ai]));
        apply_angular_velocity(&mut bodies[ai], ra, impulse);
        let rb = sub3(point, body_center(&bodies[j]));
        apply_angular_velocity(&mut bodies[j], rb, [-impulse[0], -impulse[1], -impulse[2]]);
    }
}

fn apply_angular_velocity(body: &mut Body, r: [f32; 3], impulse: [f32; 3]) {
    let torque = cross3(r, impulse);
    let delta = [
        torque[1] * body.inv_inertia[1] * MD_PER_RADIAN,
        torque[0] * body.inv_inertia[0] * MD_PER_RADIAN,
        torque[2] * body.inv_inertia[2] * MD_PER_RADIAN,
    ];
    for (rate, change) in body.omega_md.iter_mut().zip(delta) {
        *rate += change.clamp(-100.0, 100.0);
    }
}

fn contact_material(bodies: &[Body], c: &Contact) -> (f32, f32) {
    let a = &bodies[c.a];
    if let Some(j) = c.b {
        (
            a.friction.min(bodies[j].friction),
            a.restitution.max(bodies[j].restitution),
        )
    } else {
        (
            a.friction.min(c.static_friction.unwrap_or(0.9)),
            a.restitution.max(c.static_restitution.unwrap_or(0.0)),
        )
    }
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn contact_normal_depth(bodies: &[Body], c: &Contact) -> Option<([f32; 3], f32)> {
    let pa = trunc_pose(&bodies[c.a])?;
    let sa = cooked_shape(bodies[c.a].kind, bodies[c.a].local).ok()?;
    let (sb, pb) = if let Some(j) = c.b {
        (
            cooked_shape(bodies[j].kind, bodies[j].local).ok()?,
            trunc_pose(&bodies[j])?,
        )
    } else {
        let local = c.static_local?;
        let pose = c.static_pose?;
        (cooked_shape(c.static_kind?, local).ok()?, pose)
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

fn collect_joints(view: &WorldView<'_>, bodies: &[Body]) -> Vec<Joint> {
    let mut index: std::collections::BTreeMap<Sigil, usize> = std::collections::BTreeMap::new();
    for (i, b) in bodies.iter().enumerate() {
        index.insert(b.sigil, i);
    }
    let mut out = Vec::new();
    for (id, canon) in view.constraints() {
        if view.constraint_state(id).is_some_and(|s| s.broken) {
            continue;
        }
        let a = index.get(&canon.a).copied();
        let b = index.get(&canon.b).copied();
        let (ai, bi, static_x, static_angle) = match (a, b) {
            (Some(i), Some(j)) => (i, Some(j), [0.0; 3], [0.0; 3]),
            (Some(i), None) => {
                let Some(pose) = view.pose(canon.b) else {
                    continue;
                };
                (
                    i,
                    None,
                    [pose.x.0 as f32, pose.y.0 as f32, pose.z.0 as f32],
                    [pose.yaw.0 as f32, pose.pitch.0 as f32, pose.roll.0 as f32],
                )
            }
            (None, Some(j)) => {
                let Some(pose) = view.pose(canon.a) else {
                    continue;
                };
                let mut flipped = canon;
                flipped.a = canon.b;
                flipped.b = canon.a;
                flipped.anchor_a = canon.anchor_b;
                flipped.anchor_b = canon.anchor_a;
                out.push(Joint {
                    id,
                    canon: flipped,
                    a: j,
                    b: None,
                    static_x: [pose.x.0 as f32, pose.y.0 as f32, pose.z.0 as f32],
                    static_angle: [pose.yaw.0 as f32, pose.pitch.0 as f32, pose.roll.0 as f32],
                    impulse: 0.0,
                });
                continue;
            }
            (None, None) => continue,
        };
        out.push(Joint {
            id,
            canon,
            a: ai,
            b: bi,
            static_x,
            static_angle,
            impulse: 0.0,
        });
    }
    out.sort_by_key(|j| j.id);
    out
}

fn emit(
    island: u16,
    members: Vec<Sigil>,
    view: &WorldView<'_>,
    bodies: &[Body],
    support: &[Option<Support>],
    joints: &[Joint],
) -> SolveOut {
    let mut deltas = Vec::with_capacity(bodies.len());
    let mut residuals_mm = Vec::with_capacity(bodies.len());
    let mut rejected_non_finite = Vec::new();
    let island_active = bodies.iter().enumerate().any(|(i, b)| {
        b.character.is_some()
            || b.kinematic
            || view.phys_req(b.sigil).is_some()
            || b.drive
                .is_some_and(|d| d.throttle != 0 || d.brake != 0 || d.steer_md != 0)
            || !is_quiet(b, support[i])
    });
    for (i, b) in bodies.iter().enumerate() {
        let Some(attitude) = quantized_attitude(b) else {
            rejected_non_finite.push(b.sigil);
            continue;
        };
        let Some((pose, residual)) = pose_and_residual(
            b.x[0],
            b.x[1],
            b.x[2],
            attitude[0],
            attitude[1],
            attitude[2],
        ) else {
            rejected_non_finite.push(b.sigil);
            continue;
        };
        let mut vel = match vel3(b.v[0], b.v[1], b.v[2]) {
            Some(v) => v,
            None => {
                rejected_non_finite.push(b.sigil);
                continue;
            }
        };
        let mut yaw_rate = b.omega_md[0].floor() as i32;
        let mut pitch_rate = b.omega_md[1].floor() as i32;
        let mut roll_rate = b.omega_md[2].floor() as i32;
        let prev_sleep = view.island(b.sigil).map(|(_, s)| s).unwrap_or(0);
        let sleep_ticks = if island_active {
            0
        } else {
            prev_sleep.saturating_add(1).min(SLEEP_AFTER_TICKS)
        };
        if b.character.is_some() {
            // Horizontal velocity is the existing locomotion request; the
            // resolved pose carries physical displacement and wall correction.
            let command = view.vel(b.sigil).map_or(Vel3::ZERO, |(v, _)| v);
            vel.x = command.x;
            vel.z = command.z;
        }
        if sleep_ticks >= SLEEP_AFTER_TICKS {
            vel = Vel3::ZERO;
            yaw_rate = 0;
            pitch_rate = 0;
            roll_rate = 0;
        }
        let hint = hits_closed_oriented(view, b.sigil, b.local, b.prev, pose);
        let mut witness = HullWitness::new(b.sigil, pose, hint);
        witness.epoch = view.epoch();
        witness.shape = b.kind;
        deltas.push(BodyDelta {
            mover: b.sigil,
            pose,
            vel,
            yaw_rate,
            pitch_rate,
            roll_rate,
            sleep_ticks,
            hull: b.hull,
            witness,
            support: support[i],
        });
        residuals_mm.push(residual);
    }
    let (constraints, breaks) = emit_refs(joints);
    let motion_contacts = match klotho_motion::Motion::contacts(view, &deltas) {
        Ok(contacts) => contacts,
        Err(_) => {
            return SolveOut {
                proposals: Vec::new(),
                residuals_mm,
                rejected_non_finite,
                rejected_character_geometry: deltas
                    .iter()
                    .filter(|b| view.contact_window(b.mover).is_some())
                    .map(|b| b.mover)
                    .collect(),
                rejected_vehicle_geometry: Vec::new(),
            };
        }
    };
    let proposals = if rejected_non_finite.is_empty() && !deltas.is_empty() {
        vec![Proposal::PhysIsland {
            epoch: view.epoch(),
            tick: view.tick(),
            island,
            members,
            bodies: deltas,
            contacts: Vec::new(),
            motion_contacts,
            constraints,
            breaks,
        }]
    } else {
        Vec::new()
    };
    SolveOut {
        proposals,
        residuals_mm,
        rejected_non_finite,
        rejected_character_geometry: Vec::new(),
        rejected_vehicle_geometry: Vec::new(),
    }
}

fn is_quiet(body: &Body, support: Option<Support>) -> bool {
    support.is_some()
        && body.v.iter().all(|v| v.abs() < 2.0)
        && body.omega_md.iter().all(|w| w.abs() < 200.0)
}

fn hits_closed_oriented(
    view: &WorldView<'_>,
    mover: Sigil,
    local: AabbMm,
    prev: PoseMm,
    pose: PoseMm,
) -> bool {
    let Ok(shape) = cooked_shape(view.body_physics(mover).shape, local) else {
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

#[cfg(test)]
mod tests {
    use klotho_core::{AabbMm, BlobId, BodyPhysics, IVec3, PoseMm, ShapeKind, Sigil};

    use super::{Body, Contact, apply_velocity_contact, mass_properties};

    fn cube() -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -200,
                y: -200,
                z: -200,
            },
            IVec3 {
                x: 200,
                y: 200,
                z: 200,
            },
        )
    }

    #[test]
    fn canonical_mass_and_inertia_change_solver_weights() {
        let light = BodyPhysics {
            mass_grams: 1_000,
            inertia_diag: [10_000; 3],
            ..BodyPhysics::default()
        };
        let heavy = BodyPhysics {
            mass_grams: 10_000,
            inertia_diag: [100_000; 3],
            ..BodyPhysics::default()
        };
        let (light_m, light_i) = mass_properties(cube(), light);
        let (heavy_m, heavy_i) = mass_properties(cube(), heavy);
        assert!(light_m > heavy_m);
        assert!(light_i.into_iter().zip(heavy_i).all(|(a, b)| a > b));
    }

    fn body(sigil: Sigil, x: [f32; 3], v: [f32; 3]) -> Body {
        let physics = BodyPhysics::default();
        let (inv_mass, inv_inertia) = mass_properties(cube(), physics);
        Body {
            sigil,
            local: cube(),
            hull: BlobId::ZERO,
            kind: ShapeKind::OrientedBox,
            x,
            v,
            angle_md: [0.0; 3],
            omega_md: [0.0; 3],
            inv_mass,
            inv_inertia,
            center_of_mass: IVec3::ZERO,
            friction: 0.9,
            restitution: 0.0,
            prev: PoseMm::default(),
            character: None,
            vehicle: None,
            drive: None,
            kinematic: false,
            grounded: false,
        }
    }

    #[test]
    fn off_center_impact_produces_angular_impulse() {
        let a = Sigil::from_raw(1);
        let b = Sigil::from_raw(2);
        let mut bodies = vec![
            body(a, [0.0, 200.0, 0.0], [40.0, 0.0, 0.0]),
            body(b, [350.0, 0.0, 0.0], [0.0; 3]),
        ];
        let impact = bodies.iter().map(|body| body.v).collect::<Vec<_>>();
        apply_velocity_contact(
            &mut bodies,
            &Contact {
                a: 0,
                b: Some(1),
                static_local: None,
                static_pose: None,
                static_kind: None,
                static_friction: None,
                static_restitution: None,
            },
            &impact,
        );
        assert!(bodies[0].omega_md.iter().any(|rate| rate.abs() > 0.01));
        assert!(bodies[1].omega_md.iter().any(|rate| rate.abs() > 0.01));
    }
}
