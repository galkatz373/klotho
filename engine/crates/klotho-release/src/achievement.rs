//! Achievements derived from committed Trace / Knows facts.
//!
//! Observation and platform submission never emit `Proposal`s or write
//! Projection. A sink failure stays in the offline queue.

use std::collections::BTreeSet;

use klotho_core::Tick;
use klotho_ir::Name;
use klotho_trace::{RelTag, TraceBody, TraceEvent};
use serde::{Deserialize, Serialize};

use crate::ReleaseError;

/// How a committed event unlocks an achievement.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AchievementKind {
    /// `TraceBody::Learned` with this interned fact.
    Learned {
        /// Interned Knows-fact id.
        fact: u16,
    },
    /// `TraceBody::RelAdd` of `Knows`.
    KnowsRel,
    /// `TraceBody::Emitted` with this interned kind.
    Emitted {
        /// Interned emit kind.
        kind: u16,
    },
}

/// Authoring declaration packed next to the ship warp.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AchievementDef {
    /// Stable achievement id.
    pub id: Name,
    /// Unlock rule.
    pub kind: AchievementKind,
}

/// One pending or submitted unlock.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct AchievementUnlock {
    /// Achievement id.
    pub id: Name,
    /// Tick the fact committed.
    pub at: Tick,
}

/// Offline queue plus the set already handed to the platform.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct AchievementQueue {
    unlocked: BTreeSet<Name>,
    pending: Vec<AchievementUnlock>,
    submitted: BTreeSet<Name>,
}

/// Platform achievement sink. Implementations must be idempotent.
pub trait AchievementSink {
    /// Submit one unlock. Duplicate ids must succeed without a second record.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Achievement`] when the platform is unavailable.
    fn submit(&mut self, id: &Name) -> Result<(), ReleaseError>;
}

/// In-memory sink used by CI and offline tests.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct MemoryAchievementSink {
    /// Distinct submitted ids, in first-submit order.
    pub records: Vec<Name>,
    fail: bool,
}

impl MemoryAchievementSink {
    /// Empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Subsequent submits fail. Gameplay must be unchanged by this.
    pub fn fail_next(&mut self) {
        self.fail = true;
    }
}

impl AchievementSink for MemoryAchievementSink {
    fn submit(&mut self, id: &Name) -> Result<(), ReleaseError> {
        if self.fail {
            return Err(ReleaseError::Achievement(format!(
                "sink unavailable for {}",
                id.as_str()
            )));
        }
        if !self.records.iter().any(|row| row == id) {
            self.records.push(id.clone());
        }
        Ok(())
    }
}

impl AchievementQueue {
    /// Empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Unlock matching defs from committed events. Already-unlocked ids are ignored.
    pub fn observe(&mut self, defs: &[AchievementDef], events: &[TraceEvent]) {
        for event in events {
            for def in defs {
                if self.unlocked.contains(&def.id) {
                    continue;
                }
                if matches_kind(&def.kind, &event.body) {
                    self.unlocked.insert(def.id.clone());
                    self.pending.push(AchievementUnlock {
                        id: def.id.clone(),
                        at: event.tick,
                    });
                }
            }
        }
    }

    /// Ids unlocked locally and not yet confirmed by the sink.
    #[must_use]
    pub fn pending(&self) -> &[AchievementUnlock] {
        &self.pending
    }

    /// Ids the sink has accepted.
    #[must_use]
    pub fn submitted(&self) -> &BTreeSet<Name> {
        &self.submitted
    }

    /// Drain the pending queue into `sink`. Failures stay pending; nothing is
    /// written back into the world.
    pub fn reconcile(&mut self, sink: &mut impl AchievementSink) -> Result<(), ReleaseError> {
        let mut still = Vec::new();
        let mut first_err = None;
        for unlock in self.pending.drain(..) {
            if self.submitted.contains(&unlock.id) {
                continue;
            }
            match sink.submit(&unlock.id) {
                Ok(()) => {
                    self.submitted.insert(unlock.id);
                }
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                    still.push(unlock);
                }
            }
        }
        self.pending = still;
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

fn matches_kind(kind: &AchievementKind, body: &TraceBody) -> bool {
    match (kind, body) {
        (AchievementKind::Learned { fact }, TraceBody::Learned { fact: got, .. }) => fact == got,
        (AchievementKind::KnowsRel, TraceBody::RelAdd { rel, .. }) => *rel == RelTag::KNOWS,
        (AchievementKind::Emitted { kind }, TraceBody::Emitted { kind: got, .. }) => kind == got,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, Sigil, Tick};
    use klotho_trace::TraceEvent;

    use super::*;

    fn actor() -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, 1).unwrap()
    }

    fn learned(tick: u64, fact: u16) -> TraceEvent {
        TraceEvent::new(
            Tick(tick),
            TraceBody::Learned {
                mind: actor(),
                fact,
            },
        )
    }

    fn def(id: &str, fact: u16) -> AchievementDef {
        AchievementDef {
            id: Name::from(id),
            kind: AchievementKind::Learned { fact },
        }
    }

    #[test]
    fn observe_is_idempotent_and_reconcile_dedups() {
        let defs = [def("opened", 3)];
        let mut queue = AchievementQueue::new();
        queue.observe(&defs, &[learned(1, 3), learned(2, 3), learned(3, 9)]);
        assert_eq!(queue.pending().len(), 1);
        let mut sink = MemoryAchievementSink::new();
        queue.reconcile(&mut sink).unwrap();
        queue.observe(&defs, &[learned(4, 3)]);
        queue.reconcile(&mut sink).unwrap();
        assert_eq!(sink.records, vec![Name::from("opened")]);
        assert!(queue.pending().is_empty());
    }

    #[test]
    fn sink_failure_does_not_drop_pending_or_invent_gameplay() {
        let defs = [def("opened", 1)];
        let mut queue = AchievementQueue::new();
        queue.observe(&defs, &[learned(1, 1)]);
        let mut sink = MemoryAchievementSink::new();
        sink.fail_next();
        assert!(queue.reconcile(&mut sink).is_err());
        assert_eq!(queue.pending().len(), 1);
        assert!(sink.records.is_empty());
        sink.fail = false;
        queue.reconcile(&mut sink).unwrap();
        assert_eq!(sink.records.len(), 1);
    }
}
