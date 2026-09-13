//! Pinned presentation toolchain, quality mapping, and Tapestry stress (KAI-17).

use std::collections::BTreeMap;

use klotho_core::Hash;
use klotho_ir::{PresentProfile, QualityTier};
use klotho_manifest::GpuBudget;
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::error::CompileError;

/// One pinned presentation tool (shader compiler, baker, encoder, …).
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentToolPin {
    /// Human-readable tool id.
    pub id: String,
    /// Exact version string.
    pub version: String,
    /// Executable or bundle hash.
    pub hash: Hash,
}

/// Project-wide deterministic presentation lock.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationLock {
    /// glTF / USD / MaterialX / OCIO versions.
    pub interchange: BTreeMap<String, String>,
    /// OpenColorIO config hash.
    pub ocio_config: Hash,
    /// Shader compiler, probe baker, texture encoder, scalable-geometry pin.
    pub tools: BTreeMap<String, PresentToolPin>,
}

impl PresentationLock {
    /// Checked-in first-title lock.
    pub fn first_title() -> Result<Self, CompileError> {
        ron::from_str(include_str!("../../../data/presentation.lock.ron"))
            .map_err(|e| CompileError::Header(e.to_string()))
    }

    /// Validate and return the content hash of the lock.
    pub fn validate_and_hash(&self) -> Result<Hash, CompileError> {
        if self.interchange.is_empty() || self.ocio_config == Hash::ZERO || self.tools.is_empty() {
            return Err(CompileError::Header("incomplete presentation lock".into()));
        }
        for (name, pin) in &self.tools {
            if name.trim().is_empty()
                || pin.id.trim().is_empty()
                || pin.version.trim().is_empty()
                || pin.hash == Hash::ZERO
            {
                return Err(CompileError::Header("invalid presentation tool pin".into()));
            }
        }
        for key in [
            "gltf",
            "usd",
            "materialx",
            "ocio",
            "shader",
            "probe_bake",
            "texture",
            "scalable_geometry",
        ] {
            if !self.interchange.contains_key(key) && !self.tools.contains_key(key) {
                return Err(CompileError::Header(format!("missing pin {key}")));
            }
        }
        let text = ron::ser::to_string(self).map_err(|e| CompileError::Header(e.to_string()))?;
        Ok(hash_bytes(text.as_bytes()))
    }
}

/// Map an authored quality profile onto the presenter GPU budget.
#[must_use]
pub fn gpu_budget(profile: PresentProfile) -> GpuBudget {
    GpuBudget {
        us_present: profile.us_present,
        us_extract: 1_500,
        max_clusters: profile.max_clusters,
        vram_mb: profile.vram_mb,
        max_particles: profile.max_particles,
        max_ribbons: profile.max_ribbons,
    }
}

/// Counted Tapestry presentation stress scene. Not title content.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct TapestryStress {
    /// Opaque clustered instances.
    pub clusters: u16,
    /// GPU particle emitters.
    pub particles: u16,
    /// Ribbon strips.
    pub ribbons: u16,
    /// Skinned instances.
    pub skinned: u16,
    /// Resident texture mebibytes before LOD.
    pub texture_mb: u16,
    /// Probe cells.
    pub probe_cells: u16,
    /// Punctual lights.
    pub lights: u16,
}

impl TapestryStress {
    /// Scene sized to meet pinned 1080p High.
    #[must_use]
    pub const fn high_gate() -> Self {
        Self {
            clusters: 1_200,
            particles: 400,
            ribbons: 40,
            skinned: 80,
            texture_mb: 768,
            probe_cells: 256,
            lights: 24,
        }
    }

    /// Over-High scene that must fall back deterministically.
    #[must_use]
    pub const fn over_budget() -> Self {
        Self {
            clusters: 2_048,
            particles: 1_024,
            ribbons: 40,
            skinned: 80,
            texture_mb: 1_800,
            probe_cells: 256,
            lights: 24,
        }
    }
}

/// Integer present-cost estimate. Diagnostics, not a wall-clock proof.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct PresentCost {
    /// Estimated present microseconds.
    pub us_present: u32,
    /// Estimated resident mebibytes.
    pub vram_mb: u16,
}

/// Estimate High-tier cost. Cheaper tiers scale counts down before the estimate.
#[must_use]
pub fn estimate_cost(scene: TapestryStress, tier: QualityTier) -> PresentCost {
    let scale = match tier {
        QualityTier::High => 1_000u32,
        QualityTier::Medium => 500,
        QualityTier::Low => 250,
    };
    let clusters = u32::from(scene.clusters) * scale / 1_000;
    let particles = u32::from(scene.particles) * scale / 1_000;
    let ribbons = u32::from(scene.ribbons) * scale / 1_000;
    let skinned = u32::from(scene.skinned) * scale / 1_000;
    let tex = u32::from(scene.texture_mb) * scale / 1_000;
    let probes = u32::from(scene.probe_cells) * scale / 1_000;
    let lights = u32::from(scene.lights) * scale / 1_000;
    let us = 400 + clusters / 4 + particles / 8 + ribbons + skinned * 2 + probes / 16 + lights * 4;
    let vram = tex + clusters / 16 + particles / 32 + probes / 64;
    PresentCost {
        us_present: us,
        vram_mb: u16::try_from(vram.min(u32::from(u16::MAX))).unwrap_or(u16::MAX),
    }
}

/// Deterministic fallback: High → Medium → Low until the pinned budget fits.
#[must_use]
pub fn select_tier(scene: TapestryStress, requested: QualityTier) -> QualityTier {
    let mut tier = requested;
    loop {
        let cost = estimate_cost(scene, tier);
        let cap = PresentProfile::for_tier(tier);
        if cost.us_present <= cap.us_present && cost.vram_mb <= cap.vram_mb {
            return tier;
        }
        match tier.fallback() {
            Some(next) => tier = next,
            None => return QualityTier::Low,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_lock_is_complete() {
        let lock = PresentationLock::first_title().unwrap();
        assert_ne!(lock.validate_and_hash().unwrap(), Hash::ZERO);
        assert!(lock.tools.contains_key("shader"));
        assert!(lock.tools.contains_key("probe_bake"));
        assert!(lock.tools.contains_key("texture"));
        assert!(lock.tools.contains_key("scalable_geometry"));
        assert_eq!(lock.interchange.get("gltf").unwrap(), "2.0");
        assert_eq!(lock.interchange.get("materialx").unwrap(), "1.38");
    }

    #[test]
    fn high_stress_meets_high_budget() {
        let scene = TapestryStress::high_gate();
        assert_eq!(select_tier(scene, QualityTier::High), QualityTier::High);
        let cost = estimate_cost(scene, QualityTier::High);
        let cap = PresentProfile::high();
        assert!(cost.us_present <= cap.us_present);
        assert!(cost.vram_mb <= cap.vram_mb);
        assert_eq!(gpu_budget(cap).us_present, 11_000);
        assert_eq!(gpu_budget(cap).vram_mb, 1_536);
    }

    #[test]
    fn over_budget_falls_back_the_same_on_every_call() {
        let scene = TapestryStress::over_budget();
        let a = select_tier(scene, QualityTier::High);
        let b = select_tier(scene, QualityTier::High);
        assert_eq!(a, b);
        assert_eq!(a, QualityTier::Medium);
        let cost = estimate_cost(scene, a);
        let cap = PresentProfile::for_tier(a);
        assert!(cost.us_present <= cap.us_present);
        assert!(cost.vram_mb <= cap.vram_mb);
        assert!(estimate_cost(scene, QualityTier::High).vram_mb > PresentProfile::high().vram_mb);
    }
}
