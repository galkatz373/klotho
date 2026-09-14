//! Integer yaw rotation. No `f32` on the commit path (K20).

use crate::{IVec3, YawMd};

/// 16.16 cosine of 0..=90 degrees. `COS[0] = 1`, `COS[90] = 0`.
const COS: [i32; 91] = [
    65536, 65526, 65496, 65446, 65376, 65287, 65177, 65048, 64898, 64729, 64540, 64332, 64104,
    63856, 63589, 63303, 62997, 62672, 62328, 61966, 61584, 61183, 60764, 60326, 59870, 59396,
    58903, 58393, 57865, 57319, 56756, 56175, 55578, 54963, 54332, 53684, 53020, 52339, 51643,
    50931, 50203, 49461, 48703, 47930, 47143, 46341, 45525, 44695, 43852, 42995, 42126, 41243,
    40348, 39441, 38521, 37590, 36647, 35693, 34729, 33754, 32768, 31772, 30767, 29753, 28729,
    27697, 26656, 25607, 24550, 23486, 22415, 21336, 20252, 19161, 18064, 16962, 15855, 14742,
    13626, 12505, 11380, 10252, 9121, 7987, 6850, 5712, 4572, 3430, 2287, 1144, 0,
];

/// Forward millimetre offset of length `range_mm` for yaw/pitch. Yaw 0 is +Z.
/// Pitch is clamped to ±90°.
#[must_use]
pub fn look_offset(yaw: YawMd, pitch: YawMd, range_mm: i32) -> IVec3 {
    let pitch_md = pitch.0.clamp(-YawMd::QUARTER_TURN, YawMd::QUARTER_TURN);
    let (c, s) = cos_sin_pitch_deg(pitch_md.div_euclid(1000));
    let range = i64::from(range_mm);
    let y = ((range * i64::from(s)) >> 16) as i32;
    let z = ((range * i64::from(c)) >> 16) as i32;
    rotate_xz(IVec3 { x: 0, y, z }, yaw)
}

/// Rotate an XZ millimetre vector by yaw about Y. Yaw 0 faces +Z.
#[must_use]
pub fn rotate_xz(v: IVec3, yaw: YawMd) -> IVec3 {
    let (c, s) = cos_sin_deg(yaw.normalize().0.div_euclid(1000));
    let x = v.x as i64;
    let z = v.z as i64;
    let c = c as i64;
    let s = s as i64;
    IVec3 {
        x: ((x * c + z * s) >> 16) as i32,
        y: v.y,
        z: ((-x * s + z * c) >> 16) as i32,
    }
}

/// 16.16 images of the local X, Y, Z axes after `Ry(yaw) * Rx(pitch) * Rz(roll)`.
#[must_use]
pub fn rotation_axes(yaw: YawMd, pitch: YawMd, roll: YawMd) -> (IVec3, IVec3, IVec3) {
    let (cy, sy) = cos_sin_deg(yaw.normalize().0.div_euclid(1000));
    let pitch_deg = pitch.0.div_euclid(1000).clamp(-90, 90);
    let (cp, sp) = cos_sin_pitch_deg(pitch_deg);
    let (cr, sr) = cos_sin_deg(roll.normalize().0.div_euclid(1000));
    let ry = [[cy, 0, sy], [0, COS[0], 0], [-sy, 0, cy]];
    // +pitch lifts +Z toward +Y so look_offset and rotate agree.
    let rx = [[COS[0], 0, 0], [0, cp, sp], [0, -sp, cp]];
    let rz = [[cr, -sr, 0], [sr, cr, 0], [0, 0, COS[0]]];
    let r = mat_mul(ry, mat_mul(rx, rz));
    (
        IVec3 {
            x: r[0][0],
            y: r[1][0],
            z: r[2][0],
        },
        IVec3 {
            x: r[0][1],
            y: r[1][1],
            z: r[2][1],
        },
        IVec3 {
            x: r[0][2],
            y: r[1][2],
            z: r[2][2],
        },
    )
}

/// Rotate a millimetre vector by the pose attitude. Pitch and roll of zero
/// match [`rotate_xz`] bit-for-bit.
#[must_use]
pub fn rotate(v: IVec3, yaw: YawMd, pitch: YawMd, roll: YawMd) -> IVec3 {
    if pitch.0 == 0 && roll.0 == 0 {
        return rotate_xz(v, yaw);
    }
    let (ax, ay, az) = rotation_axes(yaw, pitch, roll);
    apply_axes(v, ax, ay, az)
}

fn mat_mul(a: [[i32; 3]; 3], b: [[i32; 3]; 3]) -> [[i32; 3]; 3] {
    let mut c = [[0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = fx_mul(a[i][0], b[0][j])
                .wrapping_add(fx_mul(a[i][1], b[1][j]))
                .wrapping_add(fx_mul(a[i][2], b[2][j]));
        }
    }
    c
}

fn fx_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

pub(crate) fn apply_axes(v: IVec3, ax: IVec3, ay: IVec3, az: IVec3) -> IVec3 {
    IVec3 {
        x: fx_axis(v.x, ax.x, v.y, ay.x, v.z, az.x),
        y: fx_axis(v.x, ax.y, v.y, ay.y, v.z, az.y),
        z: fx_axis(v.x, ax.z, v.y, ay.z, v.z, az.z),
    }
}

fn fx_axis(x: i32, ax: i32, y: i32, ay: i32, z: i32, az: i32) -> i32 {
    ((i64::from(x) * i64::from(ax) + i64::from(y) * i64::from(ay) + i64::from(z) * i64::from(az))
        >> 16) as i32
}

fn cos_sin_deg(deg: i32) -> (i32, i32) {
    let d = deg.rem_euclid(360);
    let q = d / 90;
    let r = (d % 90) as usize;
    match q {
        0 => (COS[r], COS[90 - r]),
        1 => (-COS[90 - r], COS[r]),
        2 => (-COS[r], -COS[90 - r]),
        _ => (COS[90 - r], -COS[r]),
    }
}

fn cos_sin_pitch_deg(deg: i32) -> (i32, i32) {
    let d = deg.clamp(-90, 90);
    if d >= 0 {
        (COS[d as usize], COS[90 - d as usize])
    } else {
        let p = (-d) as usize;
        (COS[p], -COS[90 - p])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_zero_is_identity() {
        let v = IVec3 { x: 3, y: 9, z: 20 };
        assert_eq!(rotate_xz(v, YawMd::ZERO), v);
    }

    #[test]
    fn quarter_turn_sends_forward_to_plus_x() {
        let v = IVec3 { x: 0, y: 0, z: 20 };
        let r = rotate_xz(v, YawMd(YawMd::QUARTER_TURN));
        assert_eq!(r.x, 20);
        assert_eq!(r.z, 0);
        assert_eq!(r.y, 0);
    }

    #[test]
    fn half_turn_negates_z() {
        let v = IVec3 { x: 0, y: 0, z: 20 };
        let r = rotate_xz(v, YawMd(YawMd::HALF_TURN));
        assert_eq!(r.x, 0);
        assert_eq!(r.z, -20);
    }

    #[test]
    fn table_endpoints() {
        assert_eq!(COS[0], 1 << 16);
        assert_eq!(COS[90], 0);
        assert_eq!(COS[45], 46341);
    }

    #[test]
    fn y_is_unchanged() {
        let v = IVec3 {
            x: 0,
            y: 1800,
            z: 0,
        };
        assert_eq!(rotate_xz(v, YawMd(45_000)).y, 1800);
    }

    #[test]
    fn rotate_zero_pitch_roll_matches_rotate_xz() {
        let v = IVec3 {
            x: 400,
            y: 90,
            z: -250,
        };
        for yaw in [0, 45_000, 90_000, 180_000, 270_000, -15_000] {
            assert_eq!(
                rotate(v, YawMd(yaw), YawMd::ZERO, YawMd::ZERO),
                rotate_xz(v, YawMd(yaw)),
                "yaw={yaw}"
            );
        }
    }

    #[test]
    fn quarter_pitch_lifts_forward() {
        let v = IVec3 { x: 0, y: 0, z: 20 };
        let r = rotate(v, YawMd::ZERO, YawMd(YawMd::QUARTER_TURN), YawMd::ZERO);
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 20);
        assert_eq!(r.z, 0);
    }

    #[test]
    fn look_offset_yaw_zero_is_plus_z() {
        assert_eq!(
            look_offset(YawMd::ZERO, YawMd::ZERO, 20),
            IVec3 { x: 0, y: 0, z: 20 }
        );
    }

    #[test]
    fn look_offset_quarter_yaw_is_plus_x() {
        assert_eq!(
            look_offset(YawMd(YawMd::QUARTER_TURN), YawMd::ZERO, 20),
            IVec3 { x: 20, y: 0, z: 0 }
        );
    }

    #[test]
    fn look_offset_pitch_up_is_plus_y() {
        let v = look_offset(YawMd::ZERO, YawMd(YawMd::QUARTER_TURN), 20);
        assert_eq!(v.x, 0);
        assert_eq!(v.y, 20);
        assert_eq!(v.z, 0);
    }

    #[test]
    fn look_offset_pitch_down_is_minus_y() {
        let v = look_offset(YawMd::ZERO, YawMd(-YawMd::QUARTER_TURN), 20);
        assert_eq!(v.x, 0);
        assert_eq!(v.y, -20);
        assert_eq!(v.z, 0);
    }

    #[test]
    fn look_offset_pitch_clamps_past_quarter_turn() {
        assert_eq!(
            look_offset(YawMd::ZERO, YawMd(91_000), 20),
            look_offset(YawMd::ZERO, YawMd(YawMd::QUARTER_TURN), 20)
        );
        assert_eq!(
            look_offset(YawMd::ZERO, YawMd(-91_000), 20),
            look_offset(YawMd::ZERO, YawMd(-YawMd::QUARTER_TURN), 20)
        );
    }
}
