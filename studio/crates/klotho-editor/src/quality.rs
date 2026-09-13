//! Distaff multimodal quality review (KAI-18). Designers never open RON.

use klotho_eval::{
    CapturePolicy, Coverage, EvalTier, FarmInventory, FlakePolicy, MetricLock, QualityVerdict,
    derive_slo,
};

use crate::error::EditorError;

/// Designer-facing quality summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualityReview {
    /// Farm-derived E4 p95, milliseconds.
    pub e4_p95_ms: u64,
    /// Required E4 cells.
    pub e4_cells: u32,
    /// Pinned SSIM plugin id.
    pub ssim: String,
    /// Pinned LPIPS-slot plugin id.
    pub lpips: String,
    /// Infrastructure retry budget.
    pub infra_retries: u32,
    /// Whether trusted gates passed. Critics are not consulted.
    pub passed: bool,
}

impl core::fmt::Display for QualityReview {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "quality e4={}cells p95={}ms ssim={} lpips={} retries={} {}",
            self.e4_cells,
            self.e4_p95_ms,
            self.ssim,
            self.lpips,
            self.infra_retries,
            if self.passed { "pass" } else { "fail" }
        )
    }
}

/// Review first-title multimodal quality without opening RON.
pub fn review_quality(verdict: &QualityVerdict) -> Result<QualityReview, EditorError> {
    let policy = CapturePolicy::first_title();
    policy
        .metrics
        .verify()
        .map_err(|error| EditorError::Boot(error.to_string()))?;
    MetricLock::first_title()
        .verify()
        .map_err(|error| EditorError::Boot(error.to_string()))?;
    let farm = FarmInventory::KAI_FARM_A;
    let slo = derive_slo(&farm, EvalTier::E4, &Coverage::e4_first_title(), 250);
    let flake = FlakePolicy::kai_farm_a();
    Ok(QualityReview {
        e4_p95_ms: slo.p95_ms,
        e4_cells: slo.cells,
        ssim: policy.metrics.ssim.id.to_string(),
        lpips: policy.metrics.lpips.id.to_string(),
        infra_retries: flake.max_infra_retries,
        passed: verdict.passed(),
    })
}
