//! Presenter floats. Kernel numbers stay millimetres / millidegrees (K20).

use klotho_core::{PoseMm, YawMd};
use klotho_manifest::Observer;

/// Column-major 4×4, wgpu layout.
pub(crate) type Mat4 = [f32; 16];

const PI: f32 = core::f32::consts::PI;

pub(crate) fn identity() -> Mat4 {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn mul(a: Mat4, b: Mat4) -> Mat4 {
    let mut o = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            o[col * 4 + row] = a[row] * b[col * 4]
                + a[4 + row] * b[col * 4 + 1]
                + a[8 + row] * b[col * 4 + 2]
                + a[12 + row] * b[col * 4 + 3];
        }
    }
    o
}

fn translate(x: f32, y: f32, z: f32) -> Mat4 {
    let mut m = identity();
    m[12] = x;
    m[13] = y;
    m[14] = z;
    m
}

fn rotate_y(rad: f32) -> Mat4 {
    let c = rad.cos();
    let s = rad.sin();
    [
        c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn md_to_rad(md: i32) -> f32 {
    (md as f32) * (PI / 180_000.0)
}

/// Model matrix from a millimetre pose. Yaw about Y; metres on GPU.
pub(crate) fn model_from_pose(pose: PoseMm) -> Mat4 {
    let t = translate(
        pose.x.0 as f32 / 1000.0,
        pose.y.0 as f32 / 1000.0,
        pose.z.0 as f32 / 1000.0,
    );
    mul(t, rotate_y(md_to_rad(pose.yaw.0)))
}

fn look_to(eye: [f32; 3], dir: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = norm(dir);
    let s = norm(cross(f, up));
    let u = cross(s, f);
    [
        s[0],
        u[0],
        -f[0],
        0.0,
        s[1],
        u[1],
        -f[1],
        0.0,
        s[2],
        u[2],
        -f[2],
        0.0,
        -dot(s, eye),
        -dot(u, eye),
        dot(f, eye),
        1.0,
    ]
}

fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy / 2.0).tan();
    let mut m = [0.0f32; 16];
    m[0] = f / aspect;
    m[5] = f;
    m[10] = (far + near) / (near - far);
    m[11] = -1.0;
    m[14] = (2.0 * far * near) / (near - far);
    m
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let n = dot(v, v).sqrt().max(1e-8);
    [v[0] / n, v[1] / n, v[2] / n]
}

fn look_dir(yaw: YawMd, pitch_md: i32) -> [f32; 3] {
    let yaw = md_to_rad(yaw.0);
    let pitch = md_to_rad(pitch_md);
    let cp = pitch.cos();
    [yaw.sin() * cp, pitch.sin(), yaw.cos() * cp]
}

/// View-projection from the Observer. Aspect from the presenter target.
pub(crate) fn view_proj(observer: Observer, aspect: f32) -> Mat4 {
    let eye = [
        observer.eye.x.0 as f32 / 1000.0,
        observer.eye.y.0 as f32 / 1000.0,
        observer.eye.z.0 as f32 / 1000.0,
    ];
    let dir = look_dir(observer.eye.yaw, observer.pitch_md);
    let view = look_to(eye, dir, [0.0, 1.0, 0.0]);
    mul(
        perspective(md_to_rad(60_000), aspect.max(0.1), 0.05, 200.0),
        view,
    )
}

/// Squared millimetre distance in XZ (budget drop-farthest). Integer.
pub(crate) fn dist2_xz(pose: PoseMm, observer: Observer) -> i64 {
    let dx = i64::from(pose.x.0.wrapping_sub(observer.eye.x.0));
    let dz = i64::from(pose.z.0.wrapping_sub(observer.eye.z.0));
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
}

#[cfg(test)]
mod tests {
    use klotho_core::{Mm, PoseMm, YawMd};
    use klotho_manifest::Observer;

    use super::*;

    #[test]
    fn identity_leaves_point() {
        let m = identity();
        assert_eq!(m[0], 1.0);
        assert_eq!(m[15], 1.0);
    }

    #[test]
    fn look_zero_faces_plus_z() {
        let d = look_dir(YawMd::ZERO, 0);
        assert!(d[2] > 0.9);
        assert!(d[0].abs() < 0.01);
    }

    #[test]
    fn dist2_is_integer() {
        let o = Observer::origin();
        let p = PoseMm::new(Mm(1000), Mm(0), Mm(0), YawMd::ZERO);
        assert_eq!(dist2_xz(p, o), 1_000_000);
    }
}
