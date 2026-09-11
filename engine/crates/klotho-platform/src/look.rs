//! Look analog → [`Observer`]. Built by runtime from snapshot pose, not by render.

use klotho_core::{PoseMm, YawMd};
use klotho_ir::Analog;
use klotho_manifest::Observer;

/// Accumulated look. Yaw/pitch live here; committed pose is millimetre translation.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct LookAccum {
    /// Integrated yaw, millidegrees.
    pub yaw: YawMd,
    /// Integrated pitch, millidegrees (clamped when an [`Observer`] is built).
    pub pitch_md: i32,
}

impl LookAccum {
    /// Identity look (yaw 0, pitch 0).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            yaw: YawMd::ZERO,
            pitch_md: 0,
        }
    }

    /// Apply one tick of analog look deltas.
    pub fn apply_analog(&mut self, analog: Analog) {
        self.yaw = self.yaw.wrapping_add(analog.look_yaw);
        self.pitch_md = self.pitch_md.saturating_add(analog.look_pitch);
    }

    /// Apply a raw millidegree mouse delta (winit DeviceEvent).
    pub fn apply_delta(&mut self, yaw_md: i32, pitch_md: i32) {
        self.yaw = self.yaw.wrapping_add(YawMd(yaw_md));
        self.pitch_md = self.pitch_md.saturating_add(pitch_md);
    }

    /// Observer at Hearth eye height. Yaw is this accum; translation from `ground`.
    #[must_use]
    pub fn observer(&self, ground: PoseMm) -> Observer {
        Observer::from_look(
            PoseMm::new(ground.x, ground.y, ground.z, self.yaw),
            self.pitch_md,
        )
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{Mm, PoseMm, YawMd};
    use klotho_ir::Analog;
    use klotho_manifest::{EYE_HEIGHT_MM, Observer};

    use super::*;

    #[test]
    fn look_analog_builds_observer() {
        let mut look = LookAccum::new();
        look.apply_analog(Analog {
            look_yaw: YawMd(45_000),
            look_pitch: 10_000,
            ..Analog::default()
        });
        let o = look.observer(PoseMm::new(Mm(100), Mm(0), Mm(200), YawMd::ZERO));
        assert_eq!(o.eye.x, Mm(100));
        assert_eq!(o.eye.z, Mm(200));
        assert_eq!(o.eye.y, EYE_HEIGHT_MM);
        assert_eq!(o.eye.yaw, YawMd(45_000));
        assert_eq!(o.pitch_md, 10_000);
    }

    #[test]
    fn pitch_is_clamped_on_observer() {
        let mut look = LookAccum::new();
        look.apply_delta(0, 200_000);
        let o = look.observer(PoseMm::default());
        assert_eq!(o.pitch_md, Observer::PITCH_MAX_MD);
    }
}
