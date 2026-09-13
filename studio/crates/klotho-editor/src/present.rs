//! Distaff presentation review (KAI-17). Designers never open RON.

use klotho_author::AuthorError;
use klotho_compile::{
    CastingConsent, CompileError, MaterialGraph, PresentProfile, PresentationLock, QualityTier,
    TapestryStress, compile_material, gpu_budget, select_tier,
};
use klotho_ir::IrError;

use crate::error::EditorError;

/// Designer-facing presentation summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentReview {
    /// Pinned High-tier microseconds.
    pub us_present: u32,
    /// Pinned High-tier VRAM, mebibytes.
    pub vram_mb: u16,
    /// Selected tier for the Tapestry stress scene.
    pub stress_tier: QualityTier,
    /// Presentation lock content hash (hex).
    pub lock_hash: String,
    /// Organic material permutation bits.
    pub material_bits: u8,
}

impl core::fmt::Display for PresentReview {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "present {}us {}MiB tier={} lock={}",
            self.us_present,
            self.vram_mb,
            self.stress_tier.as_str(),
            self.lock_hash
        )
    }
}

/// Review first-title presentation without opening RON.
pub fn review_presentation() -> Result<PresentReview, EditorError> {
    let profile = PresentProfile::high();
    profile.validate().map_err(ir)?;
    MaterialGraph::organic().validate().map_err(ir)?;
    CastingConsent::first_title().validate().map_err(ir)?;
    let lock = PresentationLock::first_title().map_err(cook)?;
    let lock_hash = lock.validate_and_hash().map_err(cook)?;
    let perm = compile_material(&MaterialGraph::organic()).map_err(cook)?;
    let stress_tier = select_tier(TapestryStress::high_gate(), QualityTier::High);
    let budget = gpu_budget(profile);
    Ok(PresentReview {
        us_present: budget.us_present,
        vram_mb: budget.vram_mb,
        stress_tier,
        lock_hash: lock_hash.to_string(),
        material_bits: perm.bits(),
    })
}

fn ir(e: IrError) -> EditorError {
    EditorError::from(e)
}

fn cook(e: CompileError) -> EditorError {
    EditorError::from(AuthorError::Cook(e))
}
