//! Per-tick kernel budgets. These are gates, not established facts (K14).

/// Deterministic caps applied to one `CommitKernel::step`. Exceeding a cap is
/// [`crate::RejectReason::Budget`], never a [`crate::KernelFault`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Budget {
    /// Kernel step wall-time target, microseconds. This is telemetry only and
    /// never changes admission.
    pub us_sim: u32,
    /// Predicate bytecode ops per tick. Hearth cap is 8_192.
    pub pred_ops: u32,
    /// Rite ISA steps per tick. Hearth cap is 2_000.
    pub rite_steps: u32,
    /// Infer cancel threshold in ticks (default 12 ≈ 200 ms at 60 Hz).
    /// This is **not** "two epochs."
    pub eval_slo_ticks: u16,
    /// Client rewind window in ticks. HEARTH is 0.
    pub rewind_ticks: u16,
}

impl Budget {
    /// v1 Hearth / Ash default: 4 ms kernel, 8_192 pred-ops, 2_000 rite-steps,
    /// infer SLO 12 ticks, no rewind.
    pub const HEARTH: Self = Self {
        us_sim: 4_000,
        pred_ops: 8_192,
        rite_steps: 2_000,
        eval_slo_ticks: 12,
        rewind_ticks: 0,
    };
    /// Adventure profile: 8 ms serial admit, 6-tick SLO (~200 ms at 30 Hz).
    pub const AAA_ADVENTURE: Self = Self {
        us_sim: 8_000,
        pred_ops: 65_536,
        rite_steps: 16_384,
        eval_slo_ticks: 6,
        rewind_ticks: 0,
    };
    /// Shooter profile: 5 ms, 12-tick SLO (~200 ms at 60 Hz), 12-tick rewind.
    pub const AAA_SHOOTER: Self = Self {
        us_sim: 5_000,
        pred_ops: 32_768,
        rite_steps: 8_192,
        eval_slo_ticks: 12,
        rewind_ticks: 12,
    };
}

impl Default for Budget {
    fn default() -> Self {
        Self::HEARTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hearth_matches_engineering_table() {
        let b = Budget::default();
        assert_eq!(b.us_sim, 4_000);
        assert_eq!(b.pred_ops, 8_192);
        assert_eq!(b.rite_steps, 2_000);
        assert_eq!(b.eval_slo_ticks, 12);
        assert_eq!(b.rewind_ticks, 0);
    }

    #[test]
    fn aaa_adventure_pred_ops_does_not_fit_u16() {
        assert_eq!(Budget::AAA_ADVENTURE.us_sim, 8_000);
        assert_eq!(Budget::AAA_ADVENTURE.pred_ops, 65_536);
        assert_eq!(Budget::AAA_ADVENTURE.rite_steps, 16_384);
        assert_eq!(Budget::AAA_ADVENTURE.eval_slo_ticks, 6);
        assert_eq!(Budget::AAA_ADVENTURE.rewind_ticks, 0);
        assert!(Budget::AAA_ADVENTURE.pred_ops > u32::from(u16::MAX));
    }

    #[test]
    fn aaa_shooter_rewind_is_twelve() {
        assert_eq!(Budget::AAA_SHOOTER.us_sim, 5_000);
        assert_eq!(Budget::AAA_SHOOTER.pred_ops, 32_768);
        assert_eq!(Budget::AAA_SHOOTER.rite_steps, 8_192);
        assert_eq!(Budget::AAA_SHOOTER.eval_slo_ticks, 12);
        assert_eq!(Budget::AAA_SHOOTER.rewind_ticks, 12);
    }
}
