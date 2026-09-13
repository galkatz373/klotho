//! Animation quality measurements (KAI-18). Presentation-only; no Trace writes.

use klotho_core::IVec3;

use crate::clip::Clip;

/// Foot-slide gate on planted frames, millimetres.
pub const FOOT_SLIDE_CAP_MM: i32 = 20;
/// Root-sample discontinuity cap, millimetres.
pub const ROOT_DISCONTINUITY_CAP_MM: i32 = 50;

/// Integer Euclidean length of a millimetre delta.
#[must_use]
pub fn length_mm(delta: IVec3) -> i32 {
    isqrt(
        i64::from(delta.x) * i64::from(delta.x)
            + i64::from(delta.y) * i64::from(delta.y)
            + i64::from(delta.z) * i64::from(delta.z),
    )
}

fn isqrt(n: i64) -> i32 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    i32::try_from(x).unwrap_or(i32::MAX)
}

/// Planted-foot translation between two samples. Zero when not planted.
#[must_use]
pub fn foot_slide_mm(planted: bool, from: IVec3, to: IVec3) -> i32 {
    if !planted {
        return 0;
    }
    length_mm(IVec3 {
        x: to.x.wrapping_sub(from.x),
        y: to.y.wrapping_sub(from.y),
        z: to.z.wrapping_sub(from.z),
    })
}

/// Frame-to-frame root translation magnitude.
#[must_use]
pub fn root_discontinuity_mm(prev: IVec3, next: IVec3) -> i32 {
    length_mm(IVec3 {
        x: next.x.wrapping_sub(prev.x),
        y: next.y.wrapping_sub(prev.y),
        z: next.z.wrapping_sub(prev.z),
    })
}

/// Visual clip swap must not change Rite `WAIT` ticks.
#[must_use]
pub fn wait_timing_preserved(before_wait_ticks: u32, after_wait_ticks: u32) -> bool {
    before_wait_ticks == after_wait_ticks
}

/// Skin vertex that ends inside a millimetre hull is a penetration report.
#[must_use]
pub fn skin_penetration_mm(vertex: IVec3, hull_min: IVec3, hull_max: IVec3) -> i32 {
    let inside = vertex.x >= hull_min.x
        && vertex.x <= hull_max.x
        && vertex.y >= hull_min.y
        && vertex.y <= hull_max.y
        && vertex.z >= hull_min.z
        && vertex.z <= hull_max.z;
    if !inside {
        return 0;
    }
    let dx = (vertex.x - hull_min.x).min(hull_max.x - vertex.x);
    let dy = (vertex.y - hull_min.y).min(hull_max.y - vertex.y);
    let dz = (vertex.z - hull_min.z).min(hull_max.z - vertex.z);
    dx.min(dy).min(dz)
}

/// Consecutive root samples of `clip` against the discontinuity cap.
#[must_use]
pub fn clip_root_ok(clip: &Clip) -> bool {
    clip.samples
        .windows(2)
        .all(|pair| root_discontinuity_mm(pair[0], pair[1]) <= ROOT_DISCONTINUITY_CAP_MM)
}

#[cfg(test)]
mod tests {
    use klotho_core::IVec3;
    use klotho_ir::Verb;

    use super::*;
    use crate::clip::Clip;

    #[test]
    fn planted_slide_and_root_pop_fail() {
        let planted = IVec3 { x: 0, y: 0, z: 0 };
        let slipped = IVec3 { x: 40, y: 0, z: 0 };
        assert_eq!(foot_slide_mm(false, planted, slipped), 0);
        assert_eq!(foot_slide_mm(true, planted, slipped), 40);
        assert!(foot_slide_mm(true, planted, slipped) > FOOT_SLIDE_CAP_MM);
        assert!(
            root_discontinuity_mm(planted, IVec3 { x: 0, y: 0, z: 80 }) > ROOT_DISCONTINUITY_CAP_MM
        );
        assert!(wait_timing_preserved(6, 6));
        assert!(!wait_timing_preserved(6, 7));
        let clip = Clip {
            id: 1,
            verb: Verb::Move,
            grounded: true,
            looping: true,
            samples: vec![IVec3::ZERO, IVec3 { x: 0, y: 0, z: 20 }],
            joints: Vec::new(),
        };
        assert!(clip_root_ok(&clip));
        assert_eq!(
            skin_penetration_mm(
                IVec3 { x: 5, y: 5, z: 5 },
                IVec3::ZERO,
                IVec3 {
                    x: 10,
                    y: 10,
                    z: 10
                }
            ),
            5
        );
    }
}
