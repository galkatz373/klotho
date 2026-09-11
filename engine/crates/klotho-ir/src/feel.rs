//! Typed game-feel contract (K84 / KAI-10).
//!
//! Commit-visible windows and curves use ticks and frozen 16.16 units.
//! Camera smoothing, shake, hit-stop imagery, and haptics are Manifest
//! presentation. Hit-stop never dilates the authoritative [`crate::Tick`].

use serde::{Deserialize, Serialize};

use klotho_core::VelFx;

use crate::error::IrError;
use crate::name::Name;
use crate::verb::Verb;

/// Inclusive tick window relative to the current action's start tick.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickWindow {
    /// First legal tick, inclusive.
    pub start: u8,
    /// Last legal tick, inclusive.
    pub end: u8,
    /// Verb this window admits.
    pub verb: Verb,
}

impl TickWindow {
    /// `true` when `elapsed` is inside `[start, end]`.
    #[must_use]
    pub const fn contains(self, elapsed: u8) -> bool {
        elapsed >= self.start && elapsed <= self.end
    }

    pub(crate) fn check(&self) -> Result<(), IrError> {
        if self.start > self.end {
            return Err(IrError::InvalidFeel {
                field: "window".into(),
                reason: format!("start {} > end {}", self.start, self.end),
            });
        }
        if self.end > FeelContract::MAX_WINDOW {
            return Err(IrError::InvalidFeel {
                field: "window".into(),
                reason: format!("end {} > {}", self.end, FeelContract::MAX_WINDOW),
            });
        }
        Ok(())
    }
}

/// One knot on a piecewise-linear 16.16 response curve.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveKnot {
    /// Stick magnitude, per-mille (`0..=1000`).
    pub x: u16,
    /// 16.16 scale factor. [`VelFx::ONE`] is unity.
    pub y: i32,
}

/// Piecewise-linear fixed-point curve. Knots are sorted by `x`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizedCurve {
    /// Sorted knots. First `x` is 0; last `x` is 1000.
    pub knots: Vec<CurveKnot>,
}

impl QuantizedCurve {
    /// Linear 0 → 0, 1000 → [`VelFx::ONE`].
    #[must_use]
    pub fn linear() -> Self {
        Self {
            knots: vec![
                CurveKnot { x: 0, y: 0 },
                CurveKnot {
                    x: 1000,
                    y: VelFx::ONE.0,
                },
            ],
        }
    }

    /// Sample at stick magnitude per-mille. Integer lerp, no floats.
    #[must_use]
    pub fn sample(&self, x_permille: u16) -> VelFx {
        let x = x_permille.min(1000);
        if self.knots.is_empty() {
            return VelFx::ZERO;
        }
        if x <= self.knots[0].x {
            return VelFx(self.knots[0].y);
        }
        for pair in self.knots.windows(2) {
            if x <= pair[1].x {
                let span = u32::from(pair[1].x.saturating_sub(pair[0].x));
                if span == 0 {
                    return VelFx(pair[1].y);
                }
                let t = u32::from(x.saturating_sub(pair[0].x));
                let dy = i64::from(pair[1].y) - i64::from(pair[0].y);
                let y = i64::from(pair[0].y) + dy * i64::from(t) / i64::from(span);
                return VelFx(y as i32);
            }
        }
        VelFx(self.knots[self.knots.len() - 1].y)
    }

    /// Scale a signed stick axis through this curve. Magnitude is per-mille of i16::MAX.
    #[must_use]
    pub fn apply_i16(&self, stick: i16) -> i16 {
        if stick == 0 {
            return 0;
        }
        let mag = u16::try_from((i32::from(stick.unsigned_abs()) * 1000) / 32767).unwrap_or(1000);
        let scale = self.sample(mag);
        let scaled = (i64::from(stick) * i64::from(scale.0)) >> VelFx::FRAC_BITS;
        scaled.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
    }

    pub(crate) fn check(&self) -> Result<(), IrError> {
        if self.knots.len() < 2 {
            return Err(IrError::InvalidFeel {
                field: "curve".into(),
                reason: "need at least two knots".into(),
            });
        }
        if self.knots[0].x != 0 || self.knots[self.knots.len() - 1].x != 1000 {
            return Err(IrError::InvalidFeel {
                field: "curve".into(),
                reason: "knots must start at 0 and end at 1000".into(),
            });
        }
        for pair in self.knots.windows(2) {
            if pair[0].x >= pair[1].x {
                return Err(IrError::InvalidFeel {
                    field: "curve".into(),
                    reason: "knots must be strictly increasing in x".into(),
                });
            }
            if pair[0].x > 1000 || pair[1].x > 1000 {
                return Err(IrError::InvalidFeel {
                    field: "curve".into(),
                    reason: "knot x is per-mille 0..=1000".into(),
                });
            }
        }
        Ok(())
    }
}

/// Camera follow, shake, and accessibility caps. Manifest presentation.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraResponse {
    /// Presentation lerp horizon in ticks.
    pub smoothing_ticks: u8,
    /// Follow stiffness, per-mille of the remaining error applied each tick.
    pub follow_stiffness: u16,
    /// Authored shake amplitude, millimetres.
    pub shake_amp_mm: u16,
    /// Accessibility shake cap, millimetres. Applied shake is `min(amp, cap)`.
    pub shake_cap_mm: u16,
    /// Maximum look acceleration, millidegrees per tick².
    pub accel_cap_md: u32,
    /// Collision-free camera hull radius around the eye, millimetres.
    pub hull_radius_mm: u16,
}

impl CameraResponse {
    /// Readable first-title follow. No authored shake.
    #[must_use]
    pub const fn first_title() -> Self {
        Self {
            smoothing_ticks: 2,
            follow_stiffness: 500,
            shake_amp_mm: 8,
            shake_cap_mm: 16,
            accel_cap_md: 12_000,
            hull_radius_mm: 250,
        }
    }

    /// Shake actually presented after accessibility.
    #[must_use]
    pub const fn presented_shake_mm(self, reduce_shake: bool) -> u16 {
        if reduce_shake {
            0
        } else if self.shake_amp_mm < self.shake_cap_mm {
            self.shake_amp_mm
        } else {
            self.shake_cap_mm
        }
    }

    pub(crate) fn check(&self) -> Result<(), IrError> {
        if self.follow_stiffness > 1000 {
            return Err(IrError::InvalidFeel {
                field: "camera.follow_stiffness".into(),
                reason: "per-mille 0..=1000".into(),
            });
        }
        if self.shake_amp_mm > self.shake_cap_mm {
            return Err(IrError::InvalidFeel {
                field: "camera.shake".into(),
                reason: "amp exceeds accessibility cap".into(),
            });
        }
        Ok(())
    }
}

/// Analog look magnet. Applied to [`crate::Analog`], never to Projection pose.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimAssistContract {
    /// Fraction of yaw error pulled in, per-mille.
    pub magnet_permille: u16,
    /// Cone inside which magnet applies, millidegrees.
    pub cone_md: u32,
    /// Hard per-tick correction cap, millidegrees.
    pub max_correction_md: u32,
}

impl AimAssistContract {
    /// Off. Explicit `None` on [`FeelContract::aim_assist`] is preferred.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            magnet_permille: 0,
            cone_md: 0,
            max_correction_md: 0,
        }
    }

    pub(crate) fn check(&self) -> Result<(), IrError> {
        if self.magnet_permille > 1000 {
            return Err(IrError::InvalidFeel {
                field: "aim_assist.magnet_permille".into(),
                reason: "per-mille 0..=1000".into(),
            });
        }
        Ok(())
    }
}

/// Authoritative recovery versus presentation hit-stop.
///
/// `recovery_wait_ticks` is a Rite `WAIT`. `hit_stop_present_ticks` freezes
/// Manifest pose only; the global tick still advances.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactPresentation {
    /// Presentation freeze frames. Never pauses [`klotho_core::Tick`].
    pub hit_stop_present_ticks: u8,
    /// Gameplay stun/recovery, authored as Rite `WAIT`.
    pub recovery_wait_ticks: u8,
    /// Impact shake, millimetres, still subject to [`CameraResponse::shake_cap_mm`].
    pub shake_amp_mm: u16,
}

impl ImpactPresentation {
    /// Spindle Use: two presentation frames, four-tick recovery.
    #[must_use]
    pub const fn spindle_use() -> Self {
        Self {
            hit_stop_present_ticks: 2,
            recovery_wait_ticks: 4,
            shake_amp_mm: 6,
        }
    }
}

/// Accessibility knobs on the same action. Not a hidden script.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeelAccessibility {
    /// Zero presented camera shake.
    pub reduce_shake: bool,
    /// Suppress haptic cues; visual/audio fallback remains.
    pub reduce_haptics: bool,
    /// Hold becomes toggle for this action.
    pub hold_to_toggle: bool,
    /// Require a non-zero [`AimAssistContract`] before the action is playable.
    pub aim_assist_required: bool,
}

/// Typed feel for one playable action (K84).
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeelContract {
    /// Action this contract tunes (`use`, `move`, `fire`).
    pub action: Name,
    /// How many ticks a discrete press may wait for a legal window.
    pub input_buffer_ticks: u8,
    /// Ticks after leaving support during which a grounded action still fires.
    pub coyote_ticks: u8,
    /// Cancel windows, sorted by start.
    pub cancel_windows: Vec<TickWindow>,
    /// Combo windows, sorted by start.
    pub combo_windows: Vec<TickWindow>,
    /// Stick acceleration response.
    pub accel_curve: QuantizedCurve,
    /// Stick deceleration / release response.
    pub decel_curve: QuantizedCurve,
    /// Camera follow and shake.
    pub camera: CameraResponse,
    /// Optional analog magnet.
    pub aim_assist: Option<AimAssistContract>,
    /// Hit-stop imagery versus Rite recovery.
    pub impact: ImpactPresentation,
    /// Haptic pattern id. Empty name means no cue.
    pub haptics: Name,
    /// Same-action accessibility.
    pub accessibility: FeelAccessibility,
}

impl FeelContract {
    /// Authored buffer/coyote/window cap. Longer values fail closed.
    pub const MAX_BUFFER: u8 = 8;
    /// See [`Self::MAX_BUFFER`].
    pub const MAX_COYOTE: u8 = 8;
    /// Inclusive window end cap.
    pub const MAX_WINDOW: u8 = 32;

    /// Spindle Use/Open action suite. Human feel-owner reviews this snapshot.
    #[must_use]
    pub fn spindle_use() -> Self {
        Self {
            action: Name::from("use"),
            input_buffer_ticks: 2,
            coyote_ticks: 2,
            cancel_windows: vec![TickWindow {
                start: 0,
                end: 3,
                verb: Verb::Drop,
            }],
            combo_windows: vec![TickWindow {
                start: 4,
                end: 8,
                verb: Verb::Use,
            }],
            accel_curve: QuantizedCurve::linear(),
            decel_curve: QuantizedCurve::linear(),
            camera: CameraResponse::first_title(),
            aim_assist: None,
            impact: ImpactPresentation::spindle_use(),
            haptics: Name::from("hit"),
            accessibility: FeelAccessibility::default(),
        }
    }

    /// Apply accessibility overlays without changing the authored recovery WAIT.
    #[must_use]
    pub fn with_accessibility(&self, access: FeelAccessibility) -> Self {
        let mut next = self.clone();
        next.accessibility = access;
        if access.reduce_haptics {
            next.haptics = Name::from("none");
        }
        if access.aim_assist_required && next.aim_assist.is_none() {
            next.aim_assist = Some(AimAssistContract {
                magnet_permille: 250,
                cone_md: 8_000,
                max_correction_md: 2_000,
            });
        }
        next
    }

    /// Structural bounds. Recovery WAIT is never rewritten here.
    pub fn validate(&self) -> Result<(), IrError> {
        self.action.check()?;
        self.haptics.check()?;
        if self.input_buffer_ticks > Self::MAX_BUFFER {
            return Err(IrError::InvalidFeel {
                field: "input_buffer_ticks".into(),
                reason: format!("{} > {}", self.input_buffer_ticks, Self::MAX_BUFFER),
            });
        }
        if self.coyote_ticks > Self::MAX_COYOTE {
            return Err(IrError::InvalidFeel {
                field: "coyote_ticks".into(),
                reason: format!("{} > {}", self.coyote_ticks, Self::MAX_COYOTE),
            });
        }
        for w in self.cancel_windows.iter().chain(self.combo_windows.iter()) {
            w.check()?;
        }
        self.accel_curve.check()?;
        self.decel_curve.check()?;
        self.camera.check()?;
        if let Some(assist) = &self.aim_assist {
            assist.check()?;
        }
        if self.accessibility.aim_assist_required && self.aim_assist.is_none() {
            return Err(IrError::InvalidFeel {
                field: "aim_assist".into(),
                reason: "accessibility requires an aim-assist contract".into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::VelFx;

    #[test]
    fn spindle_use_validates() {
        FeelContract::spindle_use().validate().unwrap();
    }

    #[test]
    fn linear_curve_is_unity_at_full_deflection() {
        let c = QuantizedCurve::linear();
        assert_eq!(c.sample(0), VelFx::ZERO);
        assert_eq!(c.sample(1000), VelFx::ONE);
        assert_eq!(c.sample(500), VelFx(VelFx::ONE.0 / 2));
        assert_eq!(c.apply_i16(0), 0);
        assert_eq!(c.apply_i16(32767), 32767);
        assert_eq!(c.apply_i16(-32767), -32767);
    }

    #[test]
    fn curve_rejects_unsorted_knots() {
        let c = QuantizedCurve {
            knots: vec![
                CurveKnot { x: 0, y: 0 },
                CurveKnot { x: 100, y: 1 },
                CurveKnot { x: 50, y: 2 },
                CurveKnot {
                    x: 1000,
                    y: VelFx::ONE.0,
                },
            ],
        };
        assert!(c.check().is_err());
    }

    #[test]
    fn buffer_over_cap_fails() {
        let mut f = FeelContract::spindle_use();
        f.input_buffer_ticks = 9;
        assert!(f.validate().is_err());
    }

    #[test]
    fn accessibility_does_not_rewrite_recovery_wait() {
        let base = FeelContract::spindle_use();
        let tuned = base.with_accessibility(FeelAccessibility {
            reduce_shake: true,
            reduce_haptics: true,
            hold_to_toggle: true,
            aim_assist_required: false,
        });
        assert_eq!(
            tuned.impact.recovery_wait_ticks,
            base.impact.recovery_wait_ticks
        );
        assert_eq!(tuned.haptics.as_str(), "none");
        assert_eq!(tuned.camera.presented_shake_mm(true), 0);
    }

    #[test]
    fn window_contains_inclusive() {
        let w = TickWindow {
            start: 2,
            end: 4,
            verb: Verb::Use,
        };
        assert!(!w.contains(1));
        assert!(w.contains(2));
        assert!(w.contains(4));
        assert!(!w.contains(5));
    }
}
