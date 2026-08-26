//! Replay PlayerIntent scripts through CommitKernel.

use std::time::Instant;

use klotho_commit::{CommitKernel, Proposal, SyncProposer};
use klotho_core::{Budget, Hash, KernelFault, Tick};
use klotho_ir::PlayerIntent;
use klotho_trace::{TraceEvent, fold_prefix, genesis_hash};

use crate::event::DebugEvent;

/// Prefix of a recorded event list. Same events ⇒ same hash.
#[must_use]
pub fn prefix_of_events(events: &[TraceEvent]) -> Hash {
    fold_prefix(genesis_hash(), events)
}

/// Kernel wrapper that records per-tick [`DebugEvent`]s.
pub struct TracePlayer {
    kernel: CommitKernel,
    budget: Budget,
}

/// Result of playing a script: per-tick events plus the final prefix.
#[derive(Clone, Debug)]
pub struct Played {
    /// One [`DebugEvent`] per ingested intent / step.
    pub events: Vec<DebugEvent>,
    /// Prefix after the last step.
    pub trace_prefix_hash: Hash,
}

impl TracePlayer {
    /// Wrap a kernel with the Hearth/Ash default budget.
    #[must_use]
    pub fn new(kernel: CommitKernel) -> Self {
        Self {
            kernel,
            budget: Budget::HEARTH,
        }
    }

    /// Live kernel (read).
    #[must_use]
    pub fn kernel(&self) -> &CommitKernel {
        &self.kernel
    }

    /// Live kernel (write): seed, bind, snapshot.
    pub fn kernel_mut(&mut self) -> &mut CommitKernel {
        &mut self.kernel
    }

    /// Current Trace prefix.
    #[must_use]
    pub fn prefix_hash(&self) -> Hash {
        self.kernel.world().trace_prefix_hash()
    }

    /// Ingest each intent (stamped to the live tick) and step once per packet.
    pub fn play(&mut self, intents: &[PlayerIntent]) -> Result<Played, KernelFault> {
        self.play_with(intents, &mut [])
    }

    /// [`Self::play`] with sync proposers (space / motion / mind).
    pub fn play_with(
        &mut self,
        intents: &[PlayerIntent],
        sync: &mut [&mut dyn SyncProposer],
    ) -> Result<Played, KernelFault> {
        let mut events = Vec::with_capacity(intents.len());
        for pi in intents {
            let mut p = pi.clone();
            p.at = self.kernel.world().tick();
            self.kernel.ingest(Proposal::Player(p));
            events.push(self.step(sync)?);
        }
        Ok(Played {
            events,
            trace_prefix_hash: self.prefix_hash(),
        })
    }

    /// One host tick with no new player packet.
    pub fn step(&mut self, sync: &mut [&mut dyn SyncProposer]) -> Result<DebugEvent, KernelFault> {
        let t0 = Instant::now();
        let delta = self.kernel.step(Tick(1), self.budget, sync)?;
        let proj_us = u32::try_from(t0.elapsed().as_micros()).unwrap_or(u32::MAX);
        Ok(DebugEvent::from_delta(delta, proj_us))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::CommitKernel;
    use klotho_core::{Hash, LocusKind, PlayerId, Sigil, Tick};
    use klotho_ir::{Agency, Analog, CanonDiff, IntentTarget, PlayerIntent, Verb, from_ron};
    use klotho_trace::{TraceBody, TraceEvent, TraceLog};
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

    fn look() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(0),
            at: Tick(0),
            verb: Verb::Look,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: Agency::none(),
        }
    }

    #[test]
    fn play_records_debug_events_and_prefix() {
        let mut player = TracePlayer::new(kernel());
        let played = player.play(&[look(), look()]).unwrap();
        assert_eq!(played.events.len(), 2);
        assert_eq!(played.events[0].tick, Tick(1));
        assert_eq!(played.events[1].tick, Tick(2));
        assert_eq!(played.trace_prefix_hash, player.prefix_hash());
        assert!(played.events.iter().all(|e| e.snap_bytes > 0));
    }

    #[test]
    fn prefix_of_events_matches_log() {
        let e = TraceEvent::new(Tick(1), TraceBody::SaveRequested);
        let log = TraceLog::from_events(vec![e.clone()]);
        assert_eq!(prefix_of_events(&[e]), log.prefix_hash());
        assert_eq!(prefix_of_events(&[]), genesis_hash());
    }
}
