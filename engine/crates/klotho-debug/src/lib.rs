//! Trace player and determinism tooling.
//!
//! Replay a recorded [`PlayerIntent`] script through [`CommitKernel`], inspect
//! legal rejects, and gate the 64-awake 4 ms budget.
//!
//! `KLOTHO_BUDGET_FAIL=1` panics when the step is ≥ 4 ms; `=0` prints a warning.
//! Unset: warn in debug, fail in release.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod budget;
mod diag;
mod event;
mod journey;
mod optimize;
mod player;
mod reject;

pub use budget::BudgetMode;
pub use diag::{diagnose_budget_miss, diagnose_unreachable_journey};
pub use event::{BudgetUsed, DebugEvent};
pub use journey::{CaptureMark, JourneyKernel};
pub use optimize::OptimizationMap;
pub use player::{Played, TracePlayer, prefix_of_events};
pub use reject::{
    diagnose_unclaimed_agency, explain_phys_reject, format_rejects, inspect_event, inspect_rejects,
};

pub use klotho_commit::CommitKernel;
pub use klotho_ir::PlayerIntent;
pub use klotho_phys::{
    BodyOverlay, ConstraintOverlay, IslandCapture, ReplayMismatch, SolveTimings, SweepOverlay,
    capture_island, replay_capture, summarize_timings,
};
pub use klotho_ui::{LoadError, SaveQuad, check_load, load, save_from_snapshot};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::CommitKernel;
    use klotho_core::{Budget, Hash, LocusKind, PlayerId, Sigil, Tick};
    use klotho_ir::{CanonDiff, from_ron};
    use klotho_world::World;

    use super::*;

    fn kernel() -> CommitKernel {
        let d: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&d).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let s = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        k.bind_player(PlayerId(0), s);
        k.world_mut().insert_locus(s, LocusKind::Actor).unwrap();
        k
    }

    #[test]
    fn load_refuses_wrong_prefix() {
        let mut k = kernel();
        k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        let snap = k.snapshot();
        let quad = save_from_snapshot(&snap);
        assert_eq!(
            check_load(&quad, Hash::ZERO, snap.canon_hash),
            Err(LoadError::PrefixMismatch)
        );
        assert_eq!(
            check_load(&quad, snap.trace_prefix_hash, snap.canon_hash),
            Ok(())
        );
        let loaded = load(quad, snap.trace_prefix_hash, snap.canon_hash).unwrap();
        assert_eq!(loaded.tick, snap.tick);
        assert_eq!(loaded.trace_prefix_hash, snap.trace_prefix_hash);
    }
}
