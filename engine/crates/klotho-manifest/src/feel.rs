//! Disposable feel presentation: hit-stop, shake, haptics (KAI-10).
//!
//! Hit-stop never dilates the authoritative tick. A frozen pose is overlay
//! only. Haptic id `0` means no cue; accessibility may force that fallback.

use klotho_core::{IVec3, PoseMm};

/// Presentation-only feel buffer extracted for render/audio/haptics.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct FeelManifest {
    /// Remaining presentation freeze frames. Zero means live.
    pub hit_stop_remaining: u8,
    /// Presented camera shake, millimetres, already accessibility-capped.
    pub shake_mm: IVec3,
    /// Packed haptic pattern. `0` = none (missing device or reduced haptics).
    pub haptic_id: u16,
    /// Camera follow horizon copied from the authored contract.
    pub camera_smoothing_ticks: u8,
    /// Pose held while hit-stop is presenting. `None` when live.
    pub frozen_pose: Option<PoseMm>,
}

impl FeelManifest {
    /// Empty live presentation. No freeze, no haptic.
    #[must_use]
    pub fn live() -> Self {
        Self::default()
    }

    /// `true` when presentation is holding a pose.
    #[must_use]
    pub const fn frozen(&self) -> bool {
        self.hit_stop_remaining > 0
    }

    /// Apply accessibility: reduced haptics clear the cue; missing devices already use `0`.
    #[must_use]
    pub fn with_haptic_fallback(mut self, reduce_haptics: bool, device_present: bool) -> Self {
        if reduce_haptics || !device_present {
            self.haptic_id = 0;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_stop_is_presentation_only() {
        let m = FeelManifest {
            hit_stop_remaining: 2,
            frozen_pose: Some(PoseMm::default()),
            ..FeelManifest::default()
        };
        assert!(m.frozen());
        assert!(m.frozen_pose.is_some());
    }

    #[test]
    fn missing_device_and_reduce_haptics_clear_cue() {
        let m = FeelManifest {
            haptic_id: 7,
            ..FeelManifest::default()
        };
        assert_eq!(m.clone().with_haptic_fallback(true, true).haptic_id, 0);
        assert_eq!(m.with_haptic_fallback(false, false).haptic_id, 0);
    }
}
