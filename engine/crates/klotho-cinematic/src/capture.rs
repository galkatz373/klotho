//! Camera capture measurements (KAI-18). Presentation-only.

use klotho_core::{IVec3, Mm};
use klotho_ir::CameraResponse;
use klotho_manifest::{Observer, PoseMm};

/// Hero / camera pair at a capture marker.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct CameraCapture {
    /// Hero pose, millimetres.
    pub hero: PoseMm,
    /// Presented camera eye.
    pub eye: Observer,
    /// Authored shake this frame, millimetres.
    pub shake: IVec3,
    /// Camera hull contacts this frame.
    pub hull_hits: u32,
    /// Translation from the previous cut, millimetres.
    pub cut_delta_mm: i32,
}

impl CameraCapture {
    /// Hero is on-screen when inside a millimetre half-extent of the eye on XZ.
    #[must_use]
    pub fn hero_visible(self, half_w_mm: i32, half_d_mm: i32) -> bool {
        let dx = (self.hero.x.0 - self.eye.eye.x.0).unsigned_abs();
        let dz = (self.hero.z.0 - self.eye.eye.z.0).unsigned_abs();
        dx <= half_w_mm.unsigned_abs() && dz <= half_d_mm.unsigned_abs()
    }

    /// Shake after accessibility is within the authored cap and hull.
    #[must_use]
    pub fn shake_ok(self, response: CameraResponse, reduce_shake: bool) -> bool {
        let cap = i32::from(response.presented_shake_mm(reduce_shake));
        let hull = i32::from(response.hull_radius_mm);
        let clamp = cap.min(hull);
        self.shake.x.unsigned_abs() <= clamp.unsigned_abs()
            && self.shake.y.unsigned_abs() <= clamp.unsigned_abs()
            && self.shake.z.unsigned_abs() <= clamp.unsigned_abs()
    }

    /// Collision-free camera hull: no contacts.
    #[must_use]
    pub fn hull_clear(self) -> bool {
        self.hull_hits == 0
    }

    /// Cut / handoff continuity. Zero is a hard cut (allowed); pops above `cap_mm` fail.
    #[must_use]
    pub fn cut_ok(self, cap_mm: i32) -> bool {
        self.cut_delta_mm <= cap_mm
    }
}

/// Seeded hero-off-frame capture used by eval.
#[must_use]
pub fn seeded_hidden_hero() -> CameraCapture {
    CameraCapture {
        hero: PoseMm {
            x: Mm(8_000),
            y: Mm(0),
            z: Mm(0),
            yaw: klotho_core::YawMd(0),
            pitch: klotho_core::YawMd(0),
            roll: klotho_core::YawMd(0),
        },
        eye: Observer::origin(),
        shake: IVec3::ZERO,
        hull_hits: 0,
        cut_delta_mm: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_ir::CameraResponse;

    #[test]
    fn hidden_hero_and_hull_hit_fail() {
        let hidden = seeded_hidden_hero();
        assert!(!hidden.hero_visible(2_000, 1_200));
        let mut hit = hidden;
        hit.hero.x = Mm(0);
        hit.hull_hits = 1;
        assert!(hit.hero_visible(2_000, 1_200));
        assert!(!hit.hull_clear());
        let mut shake = hit;
        shake.hull_hits = 0;
        shake.shake = IVec3 { x: 400, y: 0, z: 0 };
        assert!(!shake.shake_ok(CameraResponse::first_title(), false));
        shake.cut_delta_mm = 2_000;
        assert!(!shake.cut_ok(500));
    }
}
