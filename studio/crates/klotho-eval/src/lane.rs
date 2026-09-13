//! Farm-derived evaluation lane SLOs (KAI-18).
//!
//! Elapsed targets are computed from checked-in worker inventory, lane count,
//! repetitions, and retry policy. Adding locales or GPUs changes the target;
//! required coverage cannot be dropped to keep a wall-clock slogan.

use serde::{Deserialize, Serialize};

use crate::error::EvalError;

/// Checked-in farm inventory used to derive E2–E6 elapsed SLOs.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FarmInventory {
    /// Farm id (`kai-farm-a`).
    pub id: &'static str,
    /// `count * slots_each` across worker classes.
    pub worker_slots: u32,
    /// Exclusive concurrency limit.
    pub max_concurrency: u32,
    /// Infrastructure retries from the farm retry policy.
    pub infra_retries: u32,
}

impl FarmInventory {
    /// `ci/farms/kai-farm-a.ron`.
    pub const KAI_FARM_A: Self = Self {
        id: "kai-farm-a",
        worker_slots: 1,
        max_concurrency: 1,
        infra_retries: 2,
    };
}

/// Evaluation pyramid tier.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalTier {
    /// Headless journeys / goldens.
    E2,
    /// Local Place play.
    E3,
    /// Pixel / animation / audio / UI captures.
    E4,
    /// Stress / soak.
    E5,
    /// SKU / platform package.
    E6,
}

/// Declared capture coverage. Nightly and release ignore selection.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    /// Capture markers.
    pub captures: u32,
    /// Locales.
    pub locales: u32,
    /// GPU / backend lanes.
    pub gpus: u32,
    /// Input / device lanes.
    pub devices: u32,
    /// Repetitions per cell.
    pub repetitions: u32,
}

impl Coverage {
    /// First-title E4 matrix size used to derive the SLO.
    #[must_use]
    pub const fn e4_first_title() -> Self {
        Self {
            captures: 12,
            locales: 11,
            gpus: 1,
            devices: 2,
            repetitions: 1,
        }
    }

    /// Cartesian cell count.
    #[must_use]
    pub const fn cells(&self) -> u32 {
        self.captures
            .saturating_mul(self.locales)
            .saturating_mul(self.gpus)
            .saturating_mul(self.devices)
            .saturating_mul(self.repetitions)
    }

    /// Selected coverage must be a superset of required coverage.
    pub fn contains(&self, required: &Self) -> Result<(), EvalError> {
        let fields = [
            ("captures", self.captures, required.captures),
            ("locales", self.locales, required.locales),
            ("gpus", self.gpus, required.gpus),
            ("devices", self.devices, required.devices),
            ("repetitions", self.repetitions, required.repetitions),
        ];
        for (name, got, need) in fields {
            if got < need {
                return Err(EvalError::Lane(format!(
                    "cannot drop {name} coverage ({got} < {need}) to retain an elapsed slogan"
                )));
            }
        }
        Ok(())
    }
}

/// Derived elapsed SLO for one tier.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneSlo {
    /// Tier.
    pub tier: EvalTier,
    /// p50 target, milliseconds.
    pub p50_ms: u64,
    /// p95 target, milliseconds.
    pub p95_ms: u64,
    /// Cells that must run.
    pub cells: u32,
}

/// Derive elapsed SLO from farm inventory. Retry budget is part of the target.
#[must_use]
pub fn derive_slo(
    farm: &FarmInventory,
    tier: EvalTier,
    coverage: &Coverage,
    per_cell_ms: u32,
) -> LaneSlo {
    let cells = coverage.cells();
    let workers = farm.worker_slots.max(1).min(farm.max_concurrency.max(1));
    let retries = farm.infra_retries.saturating_add(1);
    let total_ms = u64::from(cells)
        .saturating_mul(u64::from(per_cell_ms))
        .saturating_mul(u64::from(retries))
        / u64::from(workers);
    LaneSlo {
        tier,
        p50_ms: total_ms,
        p95_ms: total_ms.saturating_mul(2),
        cells,
    }
}
