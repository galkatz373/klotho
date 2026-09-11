//! Host pause gate (K17) and K19 save quadruple. Not a Place and not a Rite.

use std::sync::Arc;

use klotho_core::{Epoch, Hash, Tick};
use klotho_ir::PlayerIntent;
use klotho_save::{SaveBlob, pause_save};
use klotho_trace::TraceEvent;
use klotho_world::WorldSnapshot;

/// Local pause. Stops `step` and drops PlayerIntent when the host honors it.
#[derive(Clone, Debug, Default)]
pub struct Pause {
    paused: bool,
}

impl Pause {
    /// Unpaused.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the local pause flag.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// True while the host should stop `step`.
    #[must_use]
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Pass through a player packet, or `None` while paused (do not enqueue).
    #[must_use]
    pub fn accept_player(&self, pi: PlayerIntent) -> Option<PlayerIntent> {
        if self.paused { None } else { Some(pi) }
    }

    /// Whether the host should call kernel / sim `step`.
    #[must_use]
    pub fn should_step(&self) -> bool {
        !self.paused
    }
}

/// Pause gate plus the last published snapshot, for pause-menu save.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pause: Pause,
    last: Option<Arc<WorldSnapshot>>,
}

impl Session {
    /// Unpaused, no snapshot yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the local pause flag.
    pub fn set_paused(&mut self, paused: bool) {
        self.pause.set_paused(paused);
    }

    /// True while the host should stop `step`.
    #[must_use]
    pub fn paused(&self) -> bool {
        self.pause.paused()
    }

    /// Pass through a player packet, or `None` while paused (do not enqueue).
    #[must_use]
    pub fn accept_player(&self, pi: PlayerIntent) -> Option<PlayerIntent> {
        self.pause.accept_player(pi)
    }

    /// Whether the host should call kernel / sim `step`.
    #[must_use]
    pub fn should_step(&self) -> bool {
        self.pause.should_step()
    }

    /// Remember the last published snapshot.
    pub fn publish(&mut self, snap: Arc<WorldSnapshot>) {
        self.last = Some(snap);
    }

    /// Last snapshot passed to [`Self::publish`].
    #[must_use]
    pub fn last_snapshot(&self) -> Option<&Arc<WorldSnapshot>> {
        self.last.as_ref()
    }

    /// Checkpoint from the last published snapshot (empty suffix).
    #[must_use]
    pub fn save(&self) -> Option<SaveQuad> {
        self.last.as_ref().map(save_from_snapshot)
    }
}

/// Checkpoint: hashes + epoch + snapshot blob + suffix + tick.
#[derive(Clone, Debug)]
pub struct SaveQuad {
    /// Frozen Canon hash from the snapshot.
    pub canon_hash: Hash,
    /// Cook / hull epoch.
    pub epoch: Epoch,
    /// Trace prefix ancestry of this checkpoint.
    pub trace_prefix_hash: Hash,
    /// Projection blob.
    pub snapshot: Arc<WorldSnapshot>,
    /// Trace suffix after [`Self::trace_from_tick`]. Empty on pause save.
    pub suffix: Vec<TraceEvent>,
    /// Snapshot tick (`trace_from_tick`).
    pub trace_from_tick: Tick,
}

impl From<SaveBlob> for SaveQuad {
    fn from(blob: SaveBlob) -> Self {
        Self {
            canon_hash: blob.canon_hash,
            epoch: blob.epoch,
            trace_prefix_hash: blob.prefix,
            snapshot: blob.snap,
            suffix: blob.suffix,
            trace_from_tick: blob.trace_from_tick,
        }
    }
}

impl SaveQuad {
    fn as_blob(&self) -> SaveBlob {
        SaveBlob {
            canon_hash: self.canon_hash,
            epoch: self.epoch,
            prefix: self.trace_prefix_hash,
            snap: Arc::clone(&self.snapshot),
            suffix: self.suffix.clone(),
            trace_from_tick: self.trace_from_tick,
        }
    }
}

/// Copy the published snapshot through pause save. Does not step the kernel.
#[must_use]
pub fn save_from_snapshot(snap: &Arc<WorldSnapshot>) -> SaveQuad {
    SaveQuad::from(pause_save(snap).expect("published snapshot is a valid pause save"))
}

/// Hard refuse when a save quadruple cannot be loaded. Not a [`klotho_core::RejectReason`].
pub type LoadError = klotho_save::SaveError;

/// Refuse if `quad` ancestry does not match the live world.
pub fn check_load(
    quad: &SaveQuad,
    expected_prefix: Hash,
    expected_canon: Hash,
) -> Result<(), LoadError> {
    klotho_save::check_load(&quad.as_blob(), expected_prefix, expected_canon)
}

/// Restore the snapshot blob if ancestry matches.
pub fn load(
    quad: SaveQuad,
    expected_prefix: Hash,
    expected_canon: Hash,
) -> Result<Arc<WorldSnapshot>, LoadError> {
    klotho_save::restore(&quad.as_blob(), expected_prefix, expected_canon)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::{CommitKernel, Proposal};
    use klotho_core::{Budget, Hash, LocusKind, PlayerId, Sigil, Tick};
    use klotho_ir::{Agency, Analog, CanonDiff, IntentTarget, PlayerIntent, Verb, from_ron};
    use klotho_sim::Sim;
    use klotho_trace::TraceBody;
    use klotho_world::World;

    use super::*;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn player_intent() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(0),
            at: Tick(0),
            verb: Verb::Look,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: Agency::none(),
        }
    }

    fn kernel() -> CommitKernel {
        let d: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&d).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let s = actor(1);
        k.bind_player(PlayerId(0), s);
        k.world_mut().insert_locus(s, LocusKind::Actor).unwrap();
        k
    }

    fn host_tick(session: &Session, sim: &mut Sim, pi: PlayerIntent) {
        if let Some(pi) = session.accept_player(pi) {
            sim.ingest(Proposal::Player(pi));
        }
        if session.should_step() {
            sim.tick(Tick(1), &mut []).unwrap();
        }
    }

    #[test]
    fn paused_drops_player_intent() {
        let mut pause = Pause::new();
        let pi = player_intent();
        assert_eq!(pause.accept_player(pi.clone()), Some(pi.clone()));
        assert!(pause.should_step());
        pause.set_paused(true);
        assert!(pause.paused());
        assert!(pause.accept_player(pi).is_none());
        assert!(!pause.should_step());
        pause.set_paused(false);
        assert_eq!(pause.accept_player(player_intent()), Some(player_intent()));
        assert!(pause.should_step());
    }

    #[test]
    fn paused_host_does_not_step_or_enqueue() {
        let mut sim = Sim::new(kernel());
        let mut session = Session::new();
        session.set_paused(true);
        let tick0 = sim.kernel().world().tick();
        let n0 = sim.kernel().world().trace().len();
        host_tick(&session, &mut sim, player_intent());
        assert_eq!(sim.kernel().world().tick(), tick0);
        assert_eq!(sim.kernel().world().trace().len(), n0);
        assert!(sim.kernel().world().intents().is_empty());

        session.set_paused(false);
        host_tick(&session, &mut sim, player_intent());
        assert_eq!(sim.kernel().world().tick(), Tick(1));
    }

    #[test]
    fn save_from_last_snapshot_while_paused_is_not_a_rite() {
        let mut k = kernel();
        let snap = k.snapshot();
        let tick0 = k.world().tick();
        let n0 = k.world().trace().len();
        let mut session = Session::new();
        session.publish(Arc::clone(&snap));
        session.set_paused(true);
        assert!(session.accept_player(player_intent()).is_none());
        assert!(!session.should_step());

        let quad = session.save().expect("published snap");
        assert_eq!(quad.canon_hash, snap.canon_hash);
        assert_eq!(quad.epoch, snap.epoch);
        assert_eq!(quad.trace_prefix_hash, snap.trace_prefix_hash);
        assert_eq!(quad.trace_from_tick, snap.tick);
        assert!(quad.suffix.is_empty());
        assert!(Arc::ptr_eq(&quad.snapshot, &snap));
        assert_eq!(k.world().tick(), tick0);
        assert_eq!(k.world().trace().len(), n0);
        assert!(
            k.world()
                .trace()
                .events()
                .iter()
                .all(|e| !matches!(e.body, TraceBody::SaveRequested))
        );

        let from_fn = save_from_snapshot(&snap);
        assert_eq!(from_fn.trace_prefix_hash, quad.trace_prefix_hash);
        assert_eq!(from_fn.trace_from_tick, quad.trace_from_tick);
        assert_eq!(from_fn.epoch, quad.epoch);
        assert!(from_fn.suffix.is_empty());
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

    #[test]
    fn save_without_snapshot_is_none() {
        let session = Session::new();
        assert!(session.save().is_none());
        assert!(session.last_snapshot().is_none());
    }
}
