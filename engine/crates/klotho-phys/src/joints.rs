//! XPBD positional joints. Inputs are Canon; outputs are quantized impulses.

use klotho_commit::{ConstraintBreakClaim, ConstraintRef};
use klotho_core::{ConstraintKind, ConstraintPhysics, IVec3, Sigil, rotate};

#[derive(Copy, Clone)]
pub(crate) struct Joint {
    pub id: Sigil,
    pub canon: ConstraintPhysics,
    pub a: usize,
    pub b: Option<usize>,
    pub static_x: [f32; 3],
    pub static_angle: [f32; 3],
    pub impulse: f32,
}

pub(crate) fn apply_joint(bodies: &mut [crate::solver::Body], joint: &mut Joint) {
    match joint.canon.kind {
        ConstraintKind::Spring => apply_spring(bodies, joint),
        ConstraintKind::Slider => apply_slider(bodies, joint),
        ConstraintKind::Hinge => {
            apply_fixed(bodies, joint);
            apply_hinge_axis(bodies, joint);
        }
        ConstraintKind::Fixed => apply_fixed(bodies, joint),
    }
}

pub(crate) fn emit_refs(joints: &[Joint]) -> (Vec<ConstraintRef>, Vec<ConstraintBreakClaim>) {
    let mut constraints = Vec::new();
    let mut breaks = Vec::new();
    for joint in joints {
        let impulse = joint.impulse.abs().floor() as i32;
        constraints.push(ConstraintRef {
            constraint: joint.id,
            binding: joint.canon.binding,
            impulse,
        });
        if joint.canon.break_impulse > 0 && impulse >= joint.canon.break_impulse {
            breaks.push(ConstraintBreakClaim {
                constraint: joint.id,
                impulse,
            });
        }
    }
    constraints.sort_by_key(|c| c.constraint);
    breaks.sort_by_key(|b| b.constraint);
    (constraints, breaks)
}

fn apply_fixed(bodies: &mut [crate::solver::Body], joint: &mut Joint) {
    let (wa, ra) = world_anchor(&bodies[joint.a], joint.canon.anchor_a);
    let (wb, rb) = match joint.b {
        Some(j) => world_anchor(&bodies[j], joint.canon.anchor_b),
        None => (
            add3(
                joint.static_x,
                rotated(joint.canon.anchor_b, joint.static_angle),
            ),
            [0.0; 3],
        ),
    };
    let d = sub3(wa, wb);
    let dist = length(d);
    if dist <= 0.001 {
        return;
    }
    let n = [d[0] / dist, d[1] / dist, d[2] / dist];
    let inv_a = bodies[joint.a].inv_mass;
    let inv_b = joint.b.map_or(0.0, |j| bodies[j].inv_mass);
    let w = inv_a + inv_b;
    if w <= 0.0 {
        return;
    }
    let lambda = dist / w;
    joint.impulse += lambda;
    bodies[joint.a].x[0] -= n[0] * lambda * inv_a;
    bodies[joint.a].x[1] -= n[1] * lambda * inv_a;
    bodies[joint.a].x[2] -= n[2] * lambda * inv_a;
    apply_torque(&mut bodies[joint.a], ra, n, -lambda);
    if let Some(j) = joint.b {
        bodies[j].x[0] += n[0] * lambda * inv_b;
        bodies[j].x[1] += n[1] * lambda * inv_b;
        bodies[j].x[2] += n[2] * lambda * inv_b;
        apply_torque(&mut bodies[j], rb, n, lambda);
    }
}

fn apply_hinge_axis(bodies: &mut [crate::solver::Body], joint: &mut Joint) {
    let axis = joint.canon.axis;
    if axis == IVec3::ZERO {
        return;
    }
    let offset = IVec3 {
        x: axis.x.saturating_mul(100) / l1(axis).max(1),
        y: axis.y.saturating_mul(100) / l1(axis).max(1),
        z: axis.z.saturating_mul(100) / l1(axis).max(1),
    };
    let mut secondary = Joint {
        canon: ConstraintPhysics {
            anchor_a: add_i(joint.canon.anchor_a, offset),
            anchor_b: add_i(joint.canon.anchor_b, offset),
            ..joint.canon
        },
        impulse: 0.0,
        ..*joint
    };
    apply_fixed(bodies, &mut secondary);
    joint.impulse += secondary.impulse;
    if joint.canon.limit_md > 0 {
        let rel = bodies[joint.a].angle_md[0]
            - joint
                .b
                .map_or(joint.static_angle[0], |j| bodies[j].angle_md[0]);
        let limit = joint.canon.limit_md as f32;
        if rel.abs() > limit {
            let corr = rel.signum() * (rel.abs() - limit);
            bodies[joint.a].angle_md[0] -= corr;
            joint.impulse += corr.abs();
        }
    }
}

fn apply_slider(bodies: &mut [crate::solver::Body], joint: &mut Joint) {
    let (wa, _) = world_anchor(&bodies[joint.a], joint.canon.anchor_a);
    let wb = match joint.b {
        Some(j) => world_anchor(&bodies[j], joint.canon.anchor_b).0,
        None => add3(
            joint.static_x,
            rotated(joint.canon.anchor_b, joint.static_angle),
        ),
    };
    let axis = rotated(joint.canon.axis, bodies[joint.a].angle_md);
    let al = length(axis).max(0.001);
    let n = [axis[0] / al, axis[1] / al, axis[2] / al];
    let d = sub3(wa, wb);
    let along = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
    let ortho = sub3(d, [n[0] * along, n[1] * along, n[2] * along]);
    let dist = length(ortho);
    if dist <= 0.001 {
        return;
    }
    let on = [ortho[0] / dist, ortho[1] / dist, ortho[2] / dist];
    let inv_a = bodies[joint.a].inv_mass;
    let inv_b = joint.b.map_or(0.0, |j| bodies[j].inv_mass);
    let w = inv_a + inv_b;
    if w <= 0.0 {
        return;
    }
    let lambda = dist / w;
    joint.impulse += lambda;
    bodies[joint.a].x[0] -= on[0] * lambda * inv_a;
    bodies[joint.a].x[1] -= on[1] * lambda * inv_a;
    bodies[joint.a].x[2] -= on[2] * lambda * inv_a;
    if let Some(j) = joint.b {
        bodies[j].x[0] += on[0] * lambda * inv_b;
        bodies[j].x[1] += on[1] * lambda * inv_b;
        bodies[j].x[2] += on[2] * lambda * inv_b;
    }
}

fn apply_spring(bodies: &mut [crate::solver::Body], joint: &mut Joint) {
    let (wa, _) = world_anchor(&bodies[joint.a], joint.canon.anchor_a);
    let wb = match joint.b {
        Some(j) => world_anchor(&bodies[j], joint.canon.anchor_b).0,
        None => add3(
            joint.static_x,
            rotated(joint.canon.anchor_b, joint.static_angle),
        ),
    };
    let d = sub3(wa, wb);
    let dist = length(d);
    let rest = joint.canon.rest_mm as f32;
    let stretch = dist - rest;
    if stretch.abs() <= 0.001 || dist <= 0.001 {
        return;
    }
    let n = [d[0] / dist, d[1] / dist, d[2] / dist];
    let k = f32::from(joint.canon.stiffness_permille) / 1_000.0;
    let inv_a = bodies[joint.a].inv_mass;
    let inv_b = joint.b.map_or(0.0, |j| bodies[j].inv_mass);
    let w = inv_a + inv_b;
    if w <= 0.0 {
        return;
    }
    let lambda = k * stretch / w;
    joint.impulse += lambda.abs();
    bodies[joint.a].x[0] -= n[0] * lambda * inv_a;
    bodies[joint.a].x[1] -= n[1] * lambda * inv_a;
    bodies[joint.a].x[2] -= n[2] * lambda * inv_a;
    if let Some(j) = joint.b {
        bodies[j].x[0] += n[0] * lambda * inv_b;
        bodies[j].x[1] += n[1] * lambda * inv_b;
        bodies[j].x[2] += n[2] * lambda * inv_b;
    }
}

fn world_anchor(body: &crate::solver::Body, local: IVec3) -> ([f32; 3], [f32; 3]) {
    let r = rotated(local, body.angle_md);
    ([body.x[0] + r[0], body.x[1] + r[1], body.x[2] + r[2]], r)
}

fn rotated(local: IVec3, angle_md: [f32; 3]) -> [f32; 3] {
    let yaw = klotho_core::YawMd(angle_md[0].floor() as i32);
    let pitch = klotho_core::YawMd(angle_md[1].floor() as i32);
    let roll = klotho_core::YawMd(angle_md[2].floor() as i32);
    let r = rotate(local, yaw, pitch, roll);
    [r.x as f32, r.y as f32, r.z as f32]
}

fn apply_torque(body: &mut crate::solver::Body, r: [f32; 3], n: [f32; 3], lambda: f32) {
    let t = [
        r[1] * n[2] - r[2] * n[1],
        r[2] * n[0] - r[0] * n[2],
        r[0] * n[1] - r[1] * n[0],
    ];
    const MD: f32 = 57_295.78 * 0.02;
    body.angle_md[0] += t[1] * body.inv_inertia[1] * lambda * MD;
    body.angle_md[1] += t[0] * body.inv_inertia[0] * lambda * MD;
    body.angle_md[2] += t[2] * body.inv_inertia[2] * lambda * MD;
    for angle in &mut body.angle_md {
        *angle = angle.clamp(-180_000.0, 180_000.0);
    }
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn l1(v: IVec3) -> i32 {
    v.x.unsigned_abs() as i32 + v.y.unsigned_abs() as i32 + v.z.unsigned_abs() as i32
}
fn add_i(a: IVec3, b: IVec3) -> IVec3 {
    IVec3 {
        x: a.x.saturating_add(b.x),
        y: a.y.saturating_add(b.y),
        z: a.z.saturating_add(b.z),
    }
}
