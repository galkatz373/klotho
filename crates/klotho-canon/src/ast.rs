//! Cooked bytecode types. `PredOp` / `PredChunk` / `RiteChunk` shapes are frozen;
//! the compiler fills them and wraps atoms in [`PredProgram`].

use klotho_core::{AffordanceId, IVec3, Mm, ResourceId, SimLod};
use klotho_ir::{Channel, Cmp, Rel, RiteOp, SourceKind, Verb};

/// Pred ops allowed in one predicate eval.
pub const PRED_OPS_PER_EVAL: u16 = 64;
/// Pred ops allowed in one kernel tick (matches [`klotho_core::Budget::HEARTH`]).
pub const PRED_OPS_PER_TICK: u32 = 8_192;
/// `ExistsRelated` / `CountRelated` neighbor scan cap.
pub const RELATED_SCAN_CAP: u8 = 64;
/// ISA steps of one rite in one tick.
pub const RITE_STEPS_PER_RITE_TICK: u16 = 64;
/// ISA steps across all rites in one tick.
pub const RITE_STEPS_PER_TICK: u32 = 2_000;
/// Heat threshold for [`klotho_ir::Pred::Burning`] desugar (`Qty(s, heat) Ge 400`).
pub const IGNITE: i32 = 400;
/// Resource name [`Pred::Burning`](klotho_ir::Pred::Burning) desugars into.
pub const HEAT: &str = "heat";
/// Affordance name [`Pred::OpaqueClosed`](klotho_ir::Pred::OpaqueClosed) desugars into.
pub const OPAQUE: &str = "Opaque";

/// Cooked predicate. Interpreter is [`crate::eval_pred`].
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct PredChunk {
    /// Stack bytecode.
    pub ops: Vec<PredOp>,
}

/// Self-contained compiled predicate: frozen ops plus interned atoms / scans.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct PredProgram {
    /// Bytecode. `PushAtom(i)` indexes [`Self::atoms`].
    pub chunk: PredChunk,
    /// Compiled atoms. Indices are dense from this program's intern.
    pub atoms: Vec<Atom>,
    /// `ExistsRelated` / `CountRelated` payloads, in bytecode encounter order.
    pub scans: Vec<RelatedScan>,
}

/// Index into a [`crate::Canon`] pred table.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct PredId(pub u16);

/// Index into a [`crate::Canon`] rite table.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct RiteId(pub u16);

/// Resolved predicate slot. `Pin(i)` is `Canon.pin_sigils[i]` / eval `pins[i]`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum CookedSlot {
    /// Acting locus.
    This,
    /// Intent target.
    Target,
    /// Bound by a related-scan.
    Other,
    /// Cook-time `Name` interned to a pin index.
    Pin(u16),
}

/// Compiled pred atom. No authoring [`klotho_ir::Name`] strings on the eval path.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Atom {
    /// Projection bitset.
    Affordance(CookedSlot, AffordanceId),
    /// Relation triple.
    Rel(CookedSlot, Rel, CookedSlot),
    /// Quantity compare.
    Qty(CookedSlot, ResourceId, Cmp, i32),
    /// Current proposal verb.
    EqVerb(Verb),
    /// `RiteMachine` row for the acting locus + rite.
    RiteActive(RiteId),
    /// Conservative AABB distance ≤ `Mm`.
    AabbNear(CookedSlot, CookedSlot, Mm),
    /// Current tick is inside that WAIT.
    InWindow(RiteId, Channel),
    /// Knows table. `u16` is a Canon fact intern.
    Knows(CookedSlot, u16),
    /// Proposer kind.
    SourceIs(SourceKind),
    /// On the *current* proposal.
    AgencyClaimed(Channel),
    /// Current Space/Motion swept AABB vs any OpaqueClosed hull.
    SweptHitsOpaqueClosed,
    /// `sleep_ticks == 0`.
    IslandAwake(CookedSlot),
    /// Slot equals the acting locus.
    SelfIs(CookedSlot),
    /// Slot equals the intent target.
    TargetIs(CookedSlot),
    /// Slot equals the scan-bound Other.
    OtherIs(CookedSlot),
    /// Hitscan vs hulls. Eval is false until phys.
    RayHits {
        /// Ray origin.
        from: CookedSlot,
        /// Direction, millimetres.
        dir: IVec3,
        /// Maximum length.
        max: Mm,
        /// Hull mask.
        mask: u8,
    },
    /// Sim LOD. Missing column is `Full`.
    SimLodIs(CookedSlot, SimLod),
    /// Place membership (`Rel::In` until the place column exists).
    InPlace(CookedSlot, CookedSlot),
}

/// Payload for [`PredOp::ExistsRelated`] / [`PredOp::CountRelated`].
///
/// Nested `pred` ops share this program's atom table. Nested quantifiers are
/// rejected by `klotho-ir`, so a nested chunk never contains scan ops.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct RelatedScan {
    /// Origin slot.
    pub of: CookedSlot,
    /// Edge label.
    pub rel: Rel,
    /// Quantifier-free nested chunk (ends with [`PredOp::Halt`]).
    pub pred: PredChunk,
    /// `None` = exists; `Some((cmp, n))` = count compare.
    pub count: Option<(Cmp, i32)>,
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

/// Cooked rite. Interpreter is `klotho-commit`; Guard/Branch preds are compiled
/// beside the chunk in [`crate::CookedRite`].
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
    /// Authoring op. The rite VM in `klotho-commit` is the packed interpreter.
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
        assert_eq!(IGNITE, 400);
    }
}
