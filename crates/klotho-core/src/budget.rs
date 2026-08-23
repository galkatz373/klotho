//! Per-tick kernel budgets. These are gates, not established facts (K14).

/// Caps applied to one `CommitKernel::step`. Exceeding a cap is
/// [`crate::RejectReason::Budget`], never a [`crate::KernelFault`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Budget {
    /// Kernel step wall time, microseconds. Hearth target is 4_000 (4 ms).
    pub us_sim: u32,
    /// Predicate bytecode ops per tick. Hearth cap is 8_192.
    pub pred_ops: u16,
    /// Rite ISA steps per tick. Hearth cap is 2_000.
    pub rite_steps: u16,
    /// Infer cancel threshold in ticks (default 12 ≈ 200 ms at 60 Hz).
    /// This is **not** "two epochs."
    pub eval_slo_ticks: u16,
}

impl Budget {
    /// v1 Hearth / Ash default: 4 ms kernel, 8_192 pred-ops, 2_000 rite-steps,
    /// infer SLO 12 ticks.
    pub const HEARTH: Self = Self {
        us_sim: 4_000,
        pred_ops: 8_192,
        rite_steps: 2_000,
        eval_slo_ticks: 12,
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
    }
}
