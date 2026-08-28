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
}
