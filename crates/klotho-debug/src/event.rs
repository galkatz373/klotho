//! Per-tick view of a kernel delta.

use klotho_core::{LawId, RejectReason, Tick};
use klotho_trace::{ProposalKind, TraceDelta, TraceEvent};

/// Observed counters for one step. `us_sim` is wall time of `CommitKernel::step`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct BudgetUsed {
    /// Predicate bytecode ops used this tick.
    pub pred_ops: u16,
    /// Rite ISA steps used this tick.
    pub rite_steps: u16,
    /// Observed step wall time, microseconds.
    pub us_sim: u32,
}

/// One tick of a Trace replay.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DebugEvent {
    /// Tick these events committed on.
    pub tick: Tick,
    /// Admitted events, in commit order.
    pub admitted: Vec<TraceEvent>,
    /// Legal rejects for this tick.
    pub rejected: Vec<(ProposalKind, RejectReason)>,
    /// Laws that fired this tick.
    pub laws_fired: Vec<LawId>,
    /// Observed budget use.
    pub budget: BudgetUsed,
    /// Snapshot blob size published this tick (0 if none).
    pub snap_bytes: u32,
    /// Observed projection/step wall time, microseconds.
    pub proj_us: u32,
}

impl DebugEvent {
    /// Map a kernel delta. `proj_us` is the observed step wall time.
    #[must_use]
    pub fn from_delta(delta: TraceDelta, proj_us: u32) -> Self {
        Self {
            tick: delta.tick,
            admitted: delta.events,
            rejected: delta.rejects,
            laws_fired: Vec::new(),
            budget: BudgetUsed {
                pred_ops: 0,
                rite_steps: 0,
                us_sim: proj_us,
            },
            snap_bytes: delta.snap_bytes,
            proj_us,
        }
    }
}
