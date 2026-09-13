//! Presenter permutation from [`PostFlags`]. Competitive wins over GI.

use klotho_compile::{PresentProfile, QualityTier, TapestryStress, estimate_cost, gpu_budget};
use klotho_manifest::{GpuBudget, PostFlags, VisualManifest};

/// Which presenter path a frame takes.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum PresenterPerm {
    /// Existing unlit+lambert family. Hearth pixel goldens.
    Unlit,
    /// Forward+ PBR, probes, SSGI, three cascades.
    Adventure,
    /// Forward+ PBR, no GI, one cascade.
    Competitive,
}

/// Map post flags to a permutation. `competitive` forces the shooter path.
#[must_use]
pub fn permutation(post: PostFlags) -> PresenterPerm {
    if post.competitive {
        PresenterPerm::Competitive
    } else if post.gi || post.bloom || post.taa {
        PresenterPerm::Adventure
    } else {
        PresenterPerm::Unlit
    }
}

/// Shadow cascade count: 0 / 3 / 1.
#[must_use]
pub fn cascade_count(perm: PresenterPerm) -> u8 {
    match perm {
        PresenterPerm::Unlit => 0,
        PresenterPerm::Adventure => 3,
        PresenterPerm::Competitive => 1,
    }
}

/// Irradiance probes. Competitive stays off even if `post.gi` was set.
#[must_use]
pub fn gi_enabled(perm: PresenterPerm) -> bool {
    matches!(perm, PresenterPerm::Adventure)
}

/// Screen-space GI. Same as [`gi_enabled`].
#[must_use]
pub fn ssgi_enabled(perm: PresenterPerm) -> bool {
    matches!(perm, PresenterPerm::Adventure)
}

/// Resolved pass flags after last-frame budget pressure.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct PresentPlan {
    /// Pipeline family.
    pub perm: PresenterPerm,
    /// Cascades to render this frame.
    pub cascades: u8,
    /// Bind a ready probe grid.
    pub gi: bool,
    /// Run the half-res SSGI pass.
    pub ssgi: bool,
    /// Half-res bright extract.
    pub bloom: bool,
    /// Neighborhood history blend.
    pub taa: bool,
}

/// Plan this frame. Over-budget last present skips SSGI, then extra cascades.
#[must_use]
pub fn present_plan(post: PostFlags, last_present_us: u32, budget: GpuBudget) -> PresentPlan {
    let perm = permutation(post);
    let mut plan = PresentPlan {
        perm,
        cascades: cascade_count(perm),
        gi: gi_enabled(perm) && post.gi,
        ssgi: ssgi_enabled(perm) && post.gi,
        bloom: matches!(perm, PresenterPerm::Adventure) && post.bloom,
        taa: matches!(perm, PresenterPerm::Adventure) && post.taa,
    };
    if last_present_us > budget.us_present {
        if plan.ssgi {
            plan.ssgi = false;
        } else if plan.cascades > 1 {
            plan.cascades = 1;
        }
    }
    plan
}

/// Map a quality tier onto post flags. Low is unlit; Medium drops SSGI.
#[must_use]
pub fn post_for_tier(tier: QualityTier) -> PostFlags {
    match tier {
        QualityTier::High => PostFlags::ADVENTURE,
        QualityTier::Medium => PostFlags {
            taa: true,
            bloom: true,
            gi: false,
            competitive: false,
            lut: None,
        },
        QualityTier::Low => PostFlags::UNLIT,
    }
}

/// Clamp GPU VFX lists to the budget. Drop order is later-first.
#[must_use]
pub fn clamp_gpu_vfx(vis: &VisualManifest, budget: GpuBudget) -> (usize, usize) {
    (
        vis.particles.len().min(budget.max_particles as usize),
        vis.ribbons.len().min(budget.max_ribbons as usize),
    )
}

/// Deterministic High→Medium→Low fallback for a counted stress scene.
#[must_use]
pub fn fallback_tier(scene: TapestryStress, requested: QualityTier) -> (QualityTier, GpuBudget) {
    let mut tier = requested;
    loop {
        let cost = estimate_cost(scene, tier);
        let profile = PresentProfile::for_tier(tier);
        if cost.us_present <= profile.us_present && cost.vram_mb <= profile.vram_mb {
            return (tier, gpu_budget(profile));
        }
        match tier.fallback() {
            Some(next) => tier = next,
            None => return (QualityTier::Low, gpu_budget(PresentProfile::low())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlit_adventure_competitive() {
        assert_eq!(permutation(PostFlags::UNLIT), PresenterPerm::Unlit);
        assert_eq!(permutation(PostFlags::ADVENTURE), PresenterPerm::Adventure);
        assert_eq!(
            permutation(PostFlags::COMPETITIVE),
            PresenterPerm::Competitive
        );
    }

    #[test]
    fn competitive_forces_gi_off_and_one_cascade() {
        let post = PostFlags {
            taa: true,
            bloom: true,
            gi: true,
            competitive: true,
            lut: None,
        };
        let perm = permutation(post);
        assert_eq!(perm, PresenterPerm::Competitive);
        assert!(!gi_enabled(perm));
        assert!(!ssgi_enabled(perm));
        assert_eq!(cascade_count(perm), 1);
    }

    #[test]
    fn cascade_counts() {
        assert_eq!(cascade_count(PresenterPerm::Unlit), 0);
        assert_eq!(cascade_count(PresenterPerm::Adventure), 3);
        assert_eq!(cascade_count(PresenterPerm::Competitive), 1);
    }

    #[test]
    fn over_budget_skips_ssgi_then_extra_cascades() {
        let under = present_plan(PostFlags::ADVENTURE, 0, GpuBudget::AAA_ADVENTURE);
        assert!(under.ssgi);
        assert_eq!(under.cascades, 3);

        let drop_ssgi = present_plan(PostFlags::ADVENTURE, 12_000, GpuBudget::AAA_ADVENTURE);
        assert!(!drop_ssgi.ssgi);
        assert_eq!(drop_ssgi.cascades, 3);

        let no_ssgi = PostFlags {
            taa: true,
            bloom: true,
            gi: false,
            competitive: false,
            lut: None,
        };
        let drop_cascades = present_plan(no_ssgi, 12_000, GpuBudget::AAA_ADVENTURE);
        assert!(!drop_cascades.ssgi);
        assert_eq!(drop_cascades.cascades, 1);
    }

    #[test]
    fn tapestry_high_stress_stays_high() {
        let (tier, budget) = fallback_tier(TapestryStress::high_gate(), QualityTier::High);
        assert_eq!(tier, QualityTier::High);
        assert_eq!(budget.us_present, 11_000);
        assert_eq!(budget.vram_mb, 1_536);
        assert_eq!(permutation(post_for_tier(tier)), PresenterPerm::Adventure);
    }

    #[test]
    fn over_budget_fallback_is_deterministic() {
        let a = fallback_tier(TapestryStress::over_budget(), QualityTier::High);
        let b = fallback_tier(TapestryStress::over_budget(), QualityTier::High);
        assert_eq!(a, b);
        assert_ne!(a.0, QualityTier::High);
        assert_eq!(
            permutation(post_for_tier(QualityTier::Low)),
            PresenterPerm::Unlit
        );
    }
}
