//! Cooked bytecode types. Filled by the PR 04b compiler; shapes are frozen here.

use klotho_ir::RiteOp;

/// Pred ops allowed in one predicate eval.
pub const PRED_OPS_PER_EVAL: u16 = 64;
/// Pred ops allowed in one kernel tick (matches [`klotho_core::Budget::HEARTH`]).
pub const PRED_OPS_PER_TICK: u16 = 8_192;
/// `ExistsRelated` / `CountRelated` neighbor scan cap.
pub const RELATED_SCAN_CAP: u8 = 64;
/// ISA steps of one rite in one tick.
pub const RITE_STEPS_PER_RITE_TICK: u16 = 64;
/// ISA steps across all rites in one tick.
pub const RITE_STEPS_PER_TICK: u16 = 2_000;

/// Cooked predicate. Interpreter is PR 04b.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct PredChunk {
    /// Stack bytecode.
    pub ops: Vec<PredOp>,
}

/// One pred bytecode op.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum PredOp {
    /// Push a compiled atom (table index).
    PushAtom(u16),
    /// `And` the top two stack values.
    And,
    /// `Or` the top two stack values.
    Or,
    /// `Not` the top stack value.
    Not,
    /// Scan related neighbors (cap [`RELATED_SCAN_CAP`]).
    ExistsRelated,
    /// Count related neighbors (cap [`RELATED_SCAN_CAP`]).
    CountRelated,
    /// End.
    Halt,
}

/// Cooked rite. Interpreter is `klotho-commit`.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct RiteChunk {
    /// Entry pc.
    pub entry: u16,
    /// Per-tick op cap for this rite.
    pub cap_steps: u16,
    /// Wall ticks.
    pub cap_ticks: u16,
    /// Ops in authoring order, each with an explicit pc.
    pub instrs: Vec<RiteInstr>,
}

/// One labeled instruction.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct RiteInstr {
    /// Program counter.
    pub pc: u16,
    /// Authoring op (still AST; packed encoding is 04b).
    pub op: RiteOp,
}

#[cfg(test)]
mod tests {
    use klotho_core::Budget;

    use super::*;

    #[test]
    fn caps_match_hearth_budget() {
        assert_eq!(PRED_OPS_PER_TICK, Budget::HEARTH.pred_ops);
        assert_eq!(RITE_STEPS_PER_TICK, Budget::HEARTH.rite_steps);
        assert_eq!(RELATED_SCAN_CAP, 64);
    }
}
