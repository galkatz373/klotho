//! Sim-thread phase loop. Does **not** depend on infer, render, mind, space, or
//! motion. `klotho-runtime` supplies `&mut [&mut dyn SyncProposer]`.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Instant;

use klotho_commit::{CommitKernel, Proposal, SyncProposer};
use klotho_core::{Budget, KernelFault, Tick};
use klotho_trace::TraceDelta;

/// Metric name for snapshot blob size.
pub const METRIC_SNAP_BYTES: &str = "klotho.snap.bytes";
/// Metric name for projection/step wall time.
pub const METRIC_PROJ_US: &str = "klotho.proj.us";

/// One host-tick phase. Present is on the render thread (PR 12), not here.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Phase {
    /// PlayerIntent and polled InferIntent enter the heap.
    Ingest,
    /// `CommitKernel::step` (K21) then snapshot publish.
    Step,
    /// Kick async infer against the **previous** snapshot.
    /// `InferHost` lives in `klotho-runtime`; this phase is a marker.
    InferKick,
    /// Listen-server flush marker. Sim does not call net.
    NetFlush,
}

/// One completed sim tick.
#[derive(Clone, Debug)]
pub struct FrameReport {
    /// Phase that produced this report (`Step`).
    pub phase: Phase,
    /// Admitted events + legal rejects.
    pub delta: TraceDelta,
    /// [`METRIC_SNAP_BYTES`].
    pub snap_bytes: u32,
    /// [`METRIC_PROJ_US`].
    pub proj_us: u32,
    /// True when `proj_us` exceeded [`Budget::us_sim`].
    pub over_budget: bool,
}

/// Sim thread driver around a [`CommitKernel`].
pub struct Sim {
    kernel: CommitKernel,
    budget: Budget,
    last_snap_bytes: u32,
}

impl Sim {
    /// Wrap a kernel with the Hearth/Ash default budget (`eval_slo_ticks` = 12).
    #[must_use]
    pub fn new(kernel: CommitKernel) -> Self {
        Self::with_budget(kernel, Budget::HEARTH)
    }

    /// Wrap a kernel with an explicit budget.
    #[must_use]
    pub fn with_budget(kernel: CommitKernel, budget: Budget) -> Self {
        Self {
            kernel,
            budget,
            last_snap_bytes: 0,
        }
    }

    /// Live kernel (read).
    #[must_use]
    pub fn kernel(&self) -> &CommitKernel {
        &self.kernel
    }

    /// Live kernel (write): seed, bind, tests.
    pub fn kernel_mut(&mut self) -> &mut CommitKernel {
        &mut self.kernel
    }

    /// Budget in force (`eval_slo_ticks` default 12).
    #[must_use]
    pub fn budget(&self) -> Budget {
        self.budget
    }

    /// [`Phase::Ingest`]: enqueue a proposal for the next [`Self::tick`].
    pub fn ingest(&mut self, p: Proposal) {
        profile_enter("klotho.ingest");
        self.kernel.ingest(p);
    }

    /// One host tick: Step → InferKick → NetFlush. Returns the Step report.
    pub fn tick(
        &mut self,
        dt: Tick,
        sync: &mut [&mut dyn SyncProposer],
    ) -> Result<FrameReport, KernelFault> {
        let report = self.phase_step(dt, sync)?;
        self.phase_infer_kick();
        self.phase_net_flush();
        Ok(report)
    }

    /// [`Phase::Step`]: kernel `step` + snapshot publish + timers.
    pub fn phase_step(
        &mut self,
        dt: Tick,
        sync: &mut [&mut dyn SyncProposer],
    ) -> Result<FrameReport, KernelFault> {
        profile_enter("klotho.step");
        let t0 = Instant::now();
        let delta = self.kernel.step(dt, self.budget, sync)?;
        let proj_us = u32::try_from(t0.elapsed().as_micros()).unwrap_or(u32::MAX);
        let snap_bytes = delta.snap_bytes;
        self.last_snap_bytes = snap_bytes;
        let _ = self.kernel.snapshot();
        Ok(FrameReport {
            phase: Phase::Step,
            delta,
            snap_bytes,
            proj_us,
            over_budget: proj_us > self.budget.us_sim,
        })
    }

    fn phase_infer_kick(&self) {
        profile_enter("klotho.infer_kick");
        let _ = Phase::InferKick;
        let _ = self.budget.eval_slo_ticks;
    }

    fn phase_net_flush(&self) {
        profile_enter("klotho.net_flush");
        let _ = Phase::NetFlush;
    }

    /// Last published snapshot size.
    #[must_use]
    pub fn last_snap_bytes(&self) -> u32 {
        self.last_snap_bytes
    }
}

#[inline]
fn profile_enter(label: &'static str) {
    let _ = label;
    #[cfg(feature = "profile")]
    {
        // puffin / Tracy scopes wire here. The feature is the PR 08 hook;
        // the crates themselves land when a presenter is present.
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::Proposal;
    use klotho_core::{Hash, LocusKind, PlayerId, Sigil};
    use klotho_ir::{Analog, CanonDiff, IntentTarget, PlayerIntent, Verb, from_ron};
    use klotho_world::World;

    use super::*;

    fn sim_with(src: &str) -> Sim {
        let d: Vec<CanonDiff> = from_ron(src).unwrap();
        let canon = cook_diffs(&d).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let s = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        k.bind_player(PlayerId(0), s);
        k.world_mut().insert_locus(s, LocusKind::Actor).unwrap();
        Sim::new(k)
    }

    #[test]
    fn eval_slo_ticks_default_is_12() {
        assert_eq!(Budget::HEARTH.eval_slo_ticks, 12);
        let s = sim_with("[]");
        assert_eq!(s.budget().eval_slo_ticks, 12);
    }

    #[test]
    fn headless_script_reports_snap_bytes() {
        let src = r#"[
            AddRite(RiteGraph(id: "spend", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Spend("stamina", 10, 2),
                Complete(Success),
                Complete(Fail),
            ])),
        ]"#;
        let mut sim = sim_with(src);
        let stamina = sim.kernel().canon().resource_id("stamina").unwrap();
        let s = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        sim.kernel_mut()
            .world_mut()
            .set_qty(s, stamina, 10)
            .unwrap();
        sim.ingest(Proposal::Player(PlayerIntent {
            player: PlayerId(0),
            at: Tick(0),
            verb: Verb::Use,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: klotho_ir::Agency::none(),
        }));
        let r = sim.tick(Tick(1), &mut []).unwrap();
        assert_eq!(r.phase, Phase::Step);
        assert!(r.delta.rejects.is_empty(), "{r:?}");
        assert_eq!(sim.last_snap_bytes(), r.snap_bytes);
        assert_eq!(METRIC_SNAP_BYTES, "klotho.snap.bytes");
        assert_eq!(METRIC_PROJ_US, "klotho.proj.us");
    }

    #[test]
    fn phases_are_four_and_present_is_not_here() {
        assert_ne!(Phase::Ingest, Phase::Step);
        assert_ne!(Phase::InferKick, Phase::NetFlush);
    }
}
