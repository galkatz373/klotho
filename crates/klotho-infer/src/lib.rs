//! Inference isolator. Returns [`InferIntent`]; never `&mut World`.
//!
//! Only `klotho-runtime` constructs and polls this host (CI allowlist). v1 is a
//! safe in-process stub: default zero weights, no model load. Jobs older than
//! `eval_slo_ticks` are dropped as [`RejectReason::StaleEpoch`]. Age equal to
//! the cap is still copied out.

#![allow(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use klotho_core::{RejectReason, Tick};
use klotho_ir::{InferIntent, IntentTarget, ModelId, Name, Verb};
use klotho_world::WorldSnapshot;

/// Handle returned by [`InferHost::submit`].
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct JobId(pub u64);

/// Snapshot-stamped eval request. Stale is a job property, not an IR field.
#[derive(Clone, Debug)]
pub struct InferJob {
    /// Previous published snapshot. Queried via [`WorldSnapshot::view`].
    pub snap: Arc<WorldSnapshot>,
    /// Tick the job was kicked. Cancel if `now - tick > eval_slo_ticks`.
    pub tick: Tick,
}

/// Non-blocking copy-out of a poll.
#[derive(Clone, Debug, Default)]
pub struct InferPoll {
    /// Fresh enough to ingest. Age **equal** to the SLO cap is included.
    pub intents: Vec<InferIntent>,
    /// Dropped jobs, each [`RejectReason::StaleEpoch`]. Not ingested.
    pub stale: Vec<RejectReason>,
}

/// Owns sessions/weights. Never holds `&mut World`. Default is infer-off.
pub struct InferHost {
    jobs: Mutex<BTreeMap<JobId, InferJob>>,
    next: AtomicU64,
    disabled: AtomicBool,
}

impl InferHost {
    /// Public so runtime can construct it. CI forbids other crates from calling this.
    #[must_use]
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(1),
            disabled: AtomicBool::new(false),
        }
    }

    /// Queue a job stamped with its kick tick. No-op if the host has disabled itself.
    pub fn submit(&self, job: InferJob) -> JobId {
        if self.disabled.load(Ordering::Relaxed) {
            return JobId(0);
        }
        let id = JobId(self.next.fetch_add(1, Ordering::Relaxed));
        match self.jobs.lock() {
            Ok(mut jobs) => {
                jobs.insert(id, job);
                id
            }
            Err(_) => {
                self.disabled.store(true, Ordering::Relaxed);
                JobId(0)
            }
        }
    }

    /// Copy-out ready intents. Never blocks. Drops jobs with `now - job.tick > slo`.
    #[must_use]
    pub fn poll(&self, now: Tick, eval_slo_ticks: u16) -> InferPoll {
        if self.disabled.load(Ordering::Relaxed) {
            return InferPoll::default();
        }
        let pending = match self.jobs.lock() {
            Ok(mut jobs) => core::mem::take(&mut *jobs),
            Err(_) => {
                self.disabled.store(true, Ordering::Relaxed);
                return InferPoll::default();
            }
        };
        let cap = u64::from(eval_slo_ticks);
        let mut out = InferPoll::default();
        for (_id, job) in pending {
            if now - job.tick > cap {
                out.stale.push(RejectReason::StaleEpoch);
            } else {
                out.intents.push(fill(&job));
            }
        }
        out
    }
}

impl Default for InferHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Stub fill: Look at the first locus, no claimed facts. Zero weights; no model.
fn fill(job: &InferJob) -> InferIntent {
    let locus = job.snap.view().loci().next();
    InferIntent {
        model: ModelId(Name::from("stub")),
        locus,
        verb: Verb::Look,
        target: IntentTarget::None,
        claimed_facts: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{Budget, Hash, Tick};
    use klotho_ir::{CanonDiff, from_ron};
    use klotho_world::World;

    use super::*;

    fn snap() -> Arc<WorldSnapshot> {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        let mut w = World::new(Arc::new(canon), Hash::ZERO);
        w.snapshot()
    }

    fn job_at(tick: Tick) -> InferJob {
        InferJob { snap: snap(), tick }
    }

    #[test]
    fn new_poll_is_empty() {
        let host = InferHost::new();
        let p = InferHost::poll(&host, Tick(0), Budget::HEARTH.eval_slo_ticks);
        assert!(p.intents.is_empty());
        assert!(p.stale.is_empty());
    }

    #[test]
    fn age_equal_to_eval_slo_ticks_is_ingested() {
        let host = InferHost::new();
        let _ = InferHost::submit(&host, job_at(Tick(0)));
        let slo = Budget::HEARTH.eval_slo_ticks;
        assert_eq!(slo, 12);
        let p = InferHost::poll(&host, Tick(12), slo);
        assert_eq!(p.intents.len(), 1, "{p:?}");
        assert!(p.stale.is_empty(), "{p:?}");
        assert!(p.intents[0].claimed_facts.is_empty());
        assert_eq!(p.intents[0].verb, Verb::Look);
    }

    #[test]
    fn age_greater_than_eval_slo_ticks_is_stale_epoch() {
        let host = InferHost::new();
        let _ = InferHost::submit(&host, job_at(Tick(0)));
        let p = InferHost::poll(&host, Tick(13), Budget::HEARTH.eval_slo_ticks);
        assert!(p.intents.is_empty(), "{p:?}");
        assert!(p.stale.contains(&RejectReason::StaleEpoch), "{p:?}");
    }

    #[test]
    fn job_carries_snapshot_not_mut_world() {
        let job = job_at(Tick(3));
        let _view = job.snap.view();
        assert_eq!(job.tick, Tick(3));
    }
}
