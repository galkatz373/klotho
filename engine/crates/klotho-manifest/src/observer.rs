//! Observer (eye) and GPU budget. Built by `klotho-runtime` from the snapshot,
//! not by the renderer (HLD §5).

use klotho_core::{Mm, PoseMm, YawMd};

/// Hearth eye height, millimetres.
pub const EYE_HEIGHT_MM: Mm = Mm(1600);

/// Camera / look state consumed by a presenter. No Sigil on the hot path.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Observer {
    /// Eye pose. Y is [`EYE_HEIGHT_MM`] for Hearth `from_look`.
    pub eye: PoseMm,
    /// Pitch about X, millidegrees, already clamped.
    pub pitch_md: i32,
}

impl Observer {
    /// Pitch clamp, millidegrees (−70° … +70°).
    pub const PITCH_MIN_MD: i32 = -70_000;
    /// Pitch clamp, millidegrees.
    pub const PITCH_MAX_MD: i32 = 70_000;

    /// Build from a standing pose + look pitch. Yaw is `ground.yaw`.
    #[must_use]
    pub fn from_look(ground: PoseMm, pitch_md: i32) -> Self {
        Self {
            eye: PoseMm::new(ground.x, EYE_HEIGHT_MM, ground.z, ground.yaw),
            pitch_md: clamp_pitch(pitch_md),
        }
    }

    /// Origin, looking +Z, pitch 0.
    #[must_use]
    pub const fn origin() -> Self {
        Self {
            eye: PoseMm::new(Mm(0), EYE_HEIGHT_MM, Mm(0), YawMd::ZERO),
            pitch_md: 0,
        }
    }
}

const fn clamp_pitch(v: i32) -> i32 {
    if v < Observer::PITCH_MIN_MD {
        Observer::PITCH_MIN_MD
    } else if v > Observer::PITCH_MAX_MD {
        Observer::PITCH_MAX_MD
    } else {
        v
    }
}

/// Per-frame GPU / extract caps (HLD engineering table). Gates, not proofs.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct GpuBudget {
    /// Present wall time, microseconds. Hearth target is 7_000 (7 ms).
    pub us_present: u32,
    /// Manifest extract, microseconds. Hearth target is 1_500.
    pub us_extract: u32,
    /// Cluster draw cap. Exceeding is drop-farthest, not a kernel reject.
    pub max_clusters: u16,
    /// Resident texture+geometry budget, mebibytes (KAI-17).
    pub vram_mb: u16,
    /// GPU particle emitter cap. Extra emitters drop.
    pub max_particles: u16,
    /// Ribbon strip cap. Extra ribbons drop.
    pub max_ribbons: u16,
}

impl GpuBudget {
    /// Hearth desktop defaults. GPU particles stay off.
    pub const HEARTH: Self = Self {
        us_present: 7_000,
        us_extract: 1_500,
        max_clusters: 256,
        vram_mb: 256,
        max_particles: 0,
        max_ribbons: 0,
    };

    /// 1080p adventure High: forward+ + cascades + probes + GPU VFX.
    pub const AAA_ADVENTURE: Self = Self {
        us_present: 11_000,
        us_extract: 1_500,
        max_clusters: 2048,
        vram_mb: 1_536,
        max_particles: 1_024,
        max_ribbons: 128,
    };

    /// 1080p shooter competitive: no GI, at most one cascade.
    pub const AAA_SHOOTER: Self = Self {
        us_present: 8_000,
        us_extract: 1_500,
        max_clusters: 1024,
        vram_mb: 1_024,
        max_particles: 256,
        max_ribbons: 32,
    };
}

impl Default for GpuBudget {
    fn default() -> Self {
        Self::HEARTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{Mm, YawMd};

    #[test]
    fn from_look_sets_eye_height_and_clamps_pitch() {
        let ground = PoseMm::new(Mm(100), Mm(0), Mm(200), YawMd(45_000));
        let o = Observer::from_look(ground, 90_000);
        assert_eq!(o.eye.y, EYE_HEIGHT_MM);
        assert_eq!(o.eye.x, Mm(100));
        assert_eq!(o.eye.z, Mm(200));
        assert_eq!(o.eye.yaw, YawMd(45_000));
        assert_eq!(o.pitch_md, Observer::PITCH_MAX_MD);
        let lo = Observer::from_look(ground, -90_000);
        assert_eq!(lo.pitch_md, Observer::PITCH_MIN_MD);
    }

    #[test]
    fn gpu_budget_profiles_match_hld() {
        assert_eq!(GpuBudget::HEARTH.us_present, 7_000);
        assert_eq!(GpuBudget::HEARTH.us_extract, 1_500);
        assert_eq!(GpuBudget::HEARTH.max_clusters, 256);
        assert_eq!(GpuBudget::HEARTH.vram_mb, 256);
        assert_eq!(GpuBudget::HEARTH.max_particles, 0);
        assert_eq!(GpuBudget::AAA_ADVENTURE.us_present, 11_000);
        assert_eq!(GpuBudget::AAA_ADVENTURE.us_extract, 1_500);
        assert_eq!(GpuBudget::AAA_ADVENTURE.max_clusters, 2048);
        assert_eq!(GpuBudget::AAA_ADVENTURE.vram_mb, 1_536);
        assert_eq!(GpuBudget::AAA_ADVENTURE.max_particles, 1_024);
        assert_eq!(GpuBudget::AAA_SHOOTER.us_present, 8_000);
        assert_eq!(GpuBudget::AAA_SHOOTER.us_extract, 1_500);
        assert_eq!(GpuBudget::AAA_SHOOTER.max_clusters, 1024);
        assert_eq!(GpuBudget::AAA_SHOOTER.vram_mb, 1_024);
    }
}
