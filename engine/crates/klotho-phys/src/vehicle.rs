//! Four-wheel ray-cast rig. Inputs reconstruct from Canon + Projection + tick.

use klotho_core::{
    IVec3, Mm, PoseMm, Support, VehicleDrive, VehiclePhysics, YawMd, rotate, rotation_axes,
};
use klotho_geom::{GeomError, MAX_VEHICLE_OBSTACLES, cast_wheel, cooked_shape};

use crate::solver::{Body, Occupancy};

const GRAVITY_MM_PER_TICK2: f32 = 2.725;
const MD_PER_RADIAN: f32 = 57_295.78;
const REST_SAG_MM: f32 = 80.0;

pub(crate) fn prepare_vehicles(statics: &[Occupancy]) -> Result<(), GeomError> {
    if statics.len() > MAX_VEHICLE_OBSTACLES {
        return Err(GeomError::Malformed);
    }
    Ok(())
}

pub(crate) fn apply_vehicles(
    bodies: &mut [Body],
    statics: &[Occupancy],
    dt: f32,
    support: &mut [Option<Support>],
) -> Result<(), GeomError> {
    if bodies.iter().all(|b| b.vehicle.is_none()) {
        return Ok(());
    }
    if statics.len() > MAX_VEHICLE_OBSTACLES {
        return Err(GeomError::Malformed);
    }
    let mut obstacles = Vec::with_capacity(statics.len());
    let mut friction = Vec::with_capacity(statics.len());
    for st in statics {
        obstacles.push((cooked_shape(st.kind, st.local)?, st.pose));
        friction.push(st.friction);
    }
    for i in 0..bodies.len() {
        let Some(policy) = bodies[i].vehicle else {
            continue;
        };
        if bodies[i].inv_mass <= 0.0 || !policy.is_valid() {
            continue;
        }
        apply_one(i, policy, bodies, &obstacles, &friction, dt, support)?;
    }
    Ok(())
}

fn apply_one(
    i: usize,
    policy: VehiclePhysics,
    bodies: &mut [Body],
    obstacles: &[(klotho_geom::Shape, PoseMm)],
    friction: &[f32],
    dt: f32,
    support: &mut [Option<Support>],
) -> Result<(), GeomError> {
    // Era-1 chassis attitude is yaw-only; pitch/roll stay presentation-derived.
    bodies[i].angle_md[1] = 0.0;
    bodies[i].angle_md[2] = 0.0;
    bodies[i].omega_md[1] = 0.0;
    bodies[i].omega_md[2] = 0.0;
    let Some(pose) = live_pose(&bodies[i]) else {
        return Ok(());
    };
    let drive = bodies[i].drive.unwrap_or(VehicleDrive {
        chassis: bodies[i].sigil,
        ..VehicleDrive::default()
    });
    let n = f32::from(policy.wheel_count).max(1.0);
    let mass = 1.0 / bodies[i].inv_mass;
    let rest_load = (mass * GRAVITY_MM_PER_TICK2 / n).max(1.0);
    let k = (f32::from(policy.stiffness_permille) / 1_000.0) * rest_load / REST_SAG_MM;
    let c = (f32::from(policy.damper_permille) / 1_000.0) * 2.0 * (k * mass / n).sqrt();
    let (_, _, forward) = unit_axes(pose);
    let mut hits = 0.0_f32;
    let mut sum_n = [0.0_f32; 3];
    let mut sum_load = 0.0_f32;
    let mut best_depth = 0.0_f32;
    struct WheelContact {
        r: [f32; 3],
        v_at: [f32; 3],
        load: f32,
        surface: f32,
        steer: i32,
    }
    let mut tire: Vec<WheelContact> = Vec::new();
    for w in 0..usize::from(policy.wheel_count) {
        let local = policy.wheels[w];
        let offset = rotate(local, pose.yaw, YawMd::ZERO, YawMd::ZERO);
        let origin = pose.translation().wrapping_add(offset);
        let ray_len = policy.rest_mm.saturating_mul(2).max(1);
        let dir = IVec3 {
            x: 0,
            y: -ray_len,
            z: 0,
        };
        let Some(hit) = cast_wheel(origin, dir, obstacles)? else {
            continue;
        };
        let compression = (policy.rest_mm - hit.dist_mm) as f32;
        let normal = unpack_normal(hit.normal);
        let r = [offset.x as f32, offset.y as f32, offset.z as f32];
        let v_at = vel_at(&bodies[i], r);
        let closing = -dot(v_at, normal);
        let load = (k * compression - c * closing)
            .max(0.0)
            .min(rest_load * 4.0);
        hits += 1.0;
        sum_n = [
            sum_n[0] + normal[0],
            sum_n[1] + normal[1],
            sum_n[2] + normal[2],
        ];
        sum_load += load;
        best_depth = best_depth.max(compression.max(0.0));
        let surface = friction.get(hit.obstacle).copied().unwrap_or(0.9);
        let steer = if local.z > 0 { drive.steer_md } else { 0 };
        tire.push(WheelContact {
            r,
            v_at,
            load,
            surface,
            steer,
        });
    }
    if hits <= 0.0 {
        bodies[i].grounded = false;
        return Ok(());
    }
    let mut nrm = [sum_n[0] / hits, sum_n[1] / hits, sum_n[2] / hits];
    let nlen = dot(nrm, nrm).sqrt().max(1.0e-4);
    nrm = [nrm[0] / nlen, nrm[1] / nlen, nrm[2] / nlen];
    let spring_impulse = [
        nrm[0] * sum_load * dt,
        nrm[1] * sum_load * dt,
        nrm[2] * sum_load * dt,
    ];
    apply_linear(&mut bodies[i], spring_impulse);
    bodies[i].v[1] *= 0.65;
    for wheel in tire {
        let WheelContact {
            r,
            v_at,
            load,
            surface,
            steer,
        } = wheel;
        let heading = heading_at(pose, steer, forward);
        let lateral = cross(nrm, heading);
        let slip_long = dot(v_at, heading);
        let slip_lat = dot(v_at, lateral);
        let mu = surface
            * (f32::from(policy.long_friction_permille) / 1_000.0)
                .max(f32::from(policy.lat_friction_permille) / 1_000.0);
        let max_f = (mu * load).max(0.0);
        let f_drive = (drive.throttle as f32) * mass / n;
        let brake_k = if policy.brake_mm == 0 {
            0.0
        } else {
            (drive.brake as f32 / policy.brake_mm as f32).clamp(0.0, 1.0)
        };
        let f_brake = if brake_k > 0.0 && dt > 0.0 {
            -slip_long * mass / (n * dt) * brake_k
        } else {
            0.0
        };
        let long_scale = f32::from(policy.long_friction_permille) / 1_000.0;
        let lat_scale = f32::from(policy.lat_friction_permille) / 1_000.0;
        let f_long_raw = f_drive + f_brake - slip_long * load * 0.04 * long_scale;
        let f_lat_raw = -slip_lat * load * 0.08 * lat_scale;
        let f_long = f_long_raw.clamp(-max_f, max_f);
        let remain = (max_f * max_f - f_long * f_long).max(0.0).sqrt();
        let f_lat = f_lat_raw.clamp(-remain, remain);
        let force = [
            heading[0] * f_long + lateral[0] * f_lat,
            heading[1] * f_long + lateral[1] * f_lat,
            heading[2] * f_long + lateral[2] * f_lat,
        ];
        apply_linear(
            &mut bodies[i],
            [force[0] * dt, force[1] * dt, force[2] * dt],
        );
        apply_yaw(&mut bodies[i], r[0], r[2], force[0] * dt, force[2] * dt);
    }
    bodies[i].grounded = true;
    if let Some(packed) = pack_up_support(best_depth) {
        match support[i] {
            Some((_, ny, _, d)) if ny >= packed.1 && d >= packed.3 => {}
            _ => support[i] = Some(packed),
        }
    }
    Ok(())
}

fn apply_linear(body: &mut Body, impulse: [f32; 3]) {
    if body.inv_mass <= 0.0 || !impulse.into_iter().all(f32::is_finite) {
        return;
    }
    for (axis, impulse_axis) in impulse.into_iter().enumerate() {
        body.v[axis] += impulse_axis * body.inv_mass;
    }
}

fn apply_yaw(body: &mut Body, rx: f32, rz: f32, fx: f32, fz: f32) {
    let tau = rx * fz - rz * fx;
    if !tau.is_finite() {
        return;
    }
    body.omega_md[0] += tau * body.inv_inertia[1] * MD_PER_RADIAN;
    body.omega_md[0] = body.omega_md[0].clamp(-4_000.0, 4_000.0);
}

fn unpack_normal(n: (i16, i16, i16)) -> [f32; 3] {
    let s = 32_767.0;
    let v = [f32::from(n.0) / s, f32::from(n.1) / s, f32::from(n.2) / s];
    let len = dot(v, v).sqrt().max(1.0e-4);
    [v[0] / len, v[1] / len, v[2] / len]
}

fn live_pose(body: &Body) -> Option<PoseMm> {
    if !body.x.into_iter().chain(body.angle_md).all(f32::is_finite) {
        return None;
    }
    Some(PoseMm {
        x: Mm(body.x[0].floor() as i32),
        y: Mm(body.x[1].floor() as i32),
        z: Mm(body.x[2].floor() as i32),
        yaw: YawMd(body.angle_md[0].floor() as i32),
        pitch: YawMd(body.angle_md[1].floor() as i32),
        roll: YawMd(body.angle_md[2].floor() as i32),
    })
}

fn unit_axes(pose: PoseMm) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (right, up, forward) = rotation_axes(pose.yaw, pose.pitch, pose.roll);
    let s = 65_536.0;
    (
        [right.x as f32 / s, right.y as f32 / s, right.z as f32 / s],
        [up.x as f32 / s, up.y as f32 / s, up.z as f32 / s],
        [
            forward.x as f32 / s,
            forward.y as f32 / s,
            forward.z as f32 / s,
        ],
    )
}

fn heading_at(pose: PoseMm, steer_md: i32, forward: [f32; 3]) -> [f32; 3] {
    if steer_md == 0 {
        return forward;
    }
    let steered = rotate(
        IVec3 {
            x: 0,
            y: 0,
            z: 1_000,
        },
        YawMd(pose.yaw.0.saturating_add(steer_md)),
        pose.pitch,
        pose.roll,
    );
    [
        steered.x as f32 / 1_000.0,
        steered.y as f32 / 1_000.0,
        steered.z as f32 / 1_000.0,
    ]
}

fn vel_at(body: &Body, r: [f32; 3]) -> [f32; 3] {
    let omega = [
        body.omega_md[1] / MD_PER_RADIAN,
        body.omega_md[0] / MD_PER_RADIAN,
        body.omega_md[2] / MD_PER_RADIAN,
    ];
    let w = cross(omega, r);
    [body.v[0] + w[0], body.v[1] + w[1], body.v[2] + w[2]]
}

fn pack_up_support(depth: f32) -> Option<Support> {
    if !depth.is_finite() {
        return None;
    }
    Some((0, 32_767, 0, depth.floor() as i32))
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
