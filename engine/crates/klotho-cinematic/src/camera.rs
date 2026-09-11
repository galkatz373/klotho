//! Camera response, accessibility caps, and presentation hit-stop (KAI-10).
//!
//! Hit-stop freezes the presented [`Observer`]; the sampled tick still
//! advances. Recovery/stun is a Rite `WAIT` and is not applied here.

use klotho_core::{IVec3, Mm};
use klotho_ir::{CameraResponse, FeelAccessibility};
use klotho_manifest::{Observer, PoseMm};

/// Presentation camera driven by a [`CameraResponse`].
#[derive(Clone, Debug)]
pub struct CameraFeel {
    response: CameraResponse,
    access: FeelAccessibility,
    last: Option<Observer>,
    last_yaw_md: i32,
    hit_stop_remaining: u8,
}

impl CameraFeel {
    /// Follow `response`, with accessibility overlays.
    #[must_use]
    pub fn new(response: CameraResponse, access: FeelAccessibility) -> Self {
        Self {
            response,
            access,
            last: None,
            last_yaw_md: 0,
            hit_stop_remaining: 0,
        }
    }

    /// First-title readable follow.
    #[must_use]
    pub fn first_title() -> Self {
        Self::new(CameraResponse::first_title(), FeelAccessibility::default())
    }

    /// Authored response.
    #[must_use]
    pub fn response(&self) -> CameraResponse {
        self.response
    }

    /// Trigger presentation hit-stop. Does not pause the global tick.
    pub fn trigger_hit_stop(&mut self, ticks: u8) {
        self.hit_stop_remaining = ticks;
    }

    /// Remaining presentation freeze frames.
    #[must_use]
    pub fn hit_stop_remaining(&self) -> u8 {
        self.hit_stop_remaining
    }

    /// Presented shake after accessibility.
    #[must_use]
    pub fn presented_shake_mm(&self) -> u16 {
        self.response.presented_shake_mm(self.access.reduce_shake)
    }

    /// Apply follow, accel cap, hull clamp, and hit-stop to `raw`.
    ///
    /// `shake` is an authored millimetre offset; it is clamped to the
    /// accessibility cap and to [`CameraResponse::hull_radius_mm`].
    #[must_use]
    pub fn present(&mut self, raw: Observer, shake: IVec3) -> Observer {
        let frozen = self.hit_stop_remaining > 0;
        if self.hit_stop_remaining > 0 {
            self.hit_stop_remaining -= 1;
        }
        if frozen {
            if let Some(last) = self.last {
                return last;
            }
        }
        let followed = self.follow(raw);
        let capped = self.cap_accel(followed);
        let shaken = self.apply_shake(capped, shake);
        self.last_yaw_md = shaken.eye.yaw.0;
        self.last = Some(shaken);
        shaken
    }

    fn follow(&self, raw: Observer) -> Observer {
        let Some(prev) = self.last else {
            return raw;
        };
        let stiffness = u32::from(self.response.follow_stiffness).min(1000);
        if stiffness == 0 {
            return prev;
        }
        if stiffness == 1000 {
            return raw;
        }
        Observer {
            eye: PoseMm {
                x: lerp_mm(prev.eye.x, raw.eye.x, stiffness),
                y: lerp_mm(prev.eye.y, raw.eye.y, stiffness),
                z: lerp_mm(prev.eye.z, raw.eye.z, stiffness),
                yaw: klotho_core::YawMd(lerp_i32(prev.eye.yaw.0, raw.eye.yaw.0, stiffness)),
                pitch: klotho_core::YawMd(lerp_i32(prev.eye.pitch.0, raw.eye.pitch.0, stiffness)),
                roll: klotho_core::YawMd(lerp_i32(prev.eye.roll.0, raw.eye.roll.0, stiffness)),
            },
            pitch_md: lerp_i32(prev.pitch_md, raw.pitch_md, stiffness)
                .clamp(Observer::PITCH_MIN_MD, Observer::PITCH_MAX_MD),
        }
    }

    fn cap_accel(&self, observer: Observer) -> Observer {
        let cap = self.response.accel_cap_md as i32;
        if cap == 0 {
            return observer;
        }
        let delta = observer.eye.yaw.0.saturating_sub(self.last_yaw_md);
        if delta.unsigned_abs() <= cap.unsigned_abs() {
            return observer;
        }
        let clamped = self.last_yaw_md.saturating_add(delta.signum() * cap);
        let mut out = observer;
        out.eye.yaw = klotho_core::YawMd(clamped);
        out
    }

    fn apply_shake(&self, mut observer: Observer, shake: IVec3) -> Observer {
        let cap = i32::from(self.presented_shake_mm());
        let hull = i32::from(self.response.hull_radius_mm);
        let clamp = |v: i32| v.clamp(-cap, cap).clamp(-hull, hull);
        observer.eye.x = Mm(observer.eye.x.0.saturating_add(clamp(shake.x)));
        observer.eye.y = Mm(observer.eye.y.0.saturating_add(clamp(shake.y)));
        observer.eye.z = Mm(observer.eye.z.0.saturating_add(clamp(shake.z)));
        observer
    }
}

fn lerp_mm(a: Mm, b: Mm, stiffness_permille: u32) -> Mm {
    Mm(lerp_i32(a.0, b.0, stiffness_permille))
}

fn lerp_i32(a: i32, b: i32, stiffness_permille: u32) -> i32 {
    let delta = i64::from(b) - i64::from(a);
    let step = delta * i64::from(stiffness_permille) / 1000;
    i32::try_from(i64::from(a) + step).unwrap_or(if step.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

#[cfg(test)]
mod tests {
    use klotho_core::{IVec3, Mm, PoseMm};
    use klotho_ir::FeelAccessibility;
    use klotho_manifest::Observer;

    use super::*;

    fn eye(x: i32) -> Observer {
        let mut o = Observer::origin();
        o.eye.x = Mm(x);
        o
    }

    #[test]
    fn hit_stop_holds_pose_while_caller_tick_still_advances() {
        let mut cam = CameraFeel::first_title();
        let a = cam.present(eye(0), IVec3::ZERO);
        cam.trigger_hit_stop(2);
        let b = cam.present(eye(1000), IVec3::ZERO);
        let c = cam.present(eye(2000), IVec3::ZERO);
        let d = cam.present(eye(3000), IVec3::ZERO);
        assert_eq!(b.eye.x, a.eye.x);
        assert_eq!(c.eye.x, a.eye.x);
        assert_ne!(d.eye.x, a.eye.x);
        assert_eq!(cam.hit_stop_remaining(), 0);
    }

    #[test]
    fn reduce_shake_zeros_presented_offset() {
        let mut cam = CameraFeel::new(
            CameraResponse::first_title(),
            FeelAccessibility {
                reduce_shake: true,
                ..FeelAccessibility::default()
            },
        );
        let o = cam.present(Observer::origin(), IVec3 { x: 40, y: 0, z: 0 });
        assert_eq!(o.eye.x, PoseMm::new(Mm(0), o.eye.y, o.eye.z, o.eye.yaw).x);
        assert_eq!(cam.presented_shake_mm(), 0);
    }

    #[test]
    fn shake_is_clamped_to_hull() {
        let mut response = CameraResponse::first_title();
        response.shake_amp_mm = 16;
        response.shake_cap_mm = 16;
        response.hull_radius_mm = 4;
        let mut cam = CameraFeel::new(response, FeelAccessibility::default());
        let o = cam.present(Observer::origin(), IVec3 { x: 100, y: 0, z: 0 });
        assert_eq!(o.eye.x, Mm(4));
    }

    #[test]
    fn stiffness_follows_toward_raw() {
        let mut cam = CameraFeel::first_title();
        let _ = cam.present(eye(0), IVec3::ZERO);
        let next = cam.present(eye(1000), IVec3::ZERO);
        assert!(next.eye.x.0 > 0);
        assert!(next.eye.x.0 < 1000);
    }
}
