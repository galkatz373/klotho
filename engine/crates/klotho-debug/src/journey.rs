//! Public-input journey kernel. No [`klotho_world::WorldMut`] on this type.

use std::collections::BTreeMap;
use std::sync::Arc;

use klotho_canon::cook_diffs;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{Hash, KernelFault, LocusKind, PlayerId, Sigil, Tick};
use klotho_input::{DeviceSample, TestInputAdapter};
use klotho_ir::{Analog, CanonDiff, IntentTarget, Name, PlayerIntent, Rel, Verb, from_ron};
use klotho_ui::{SaveQuad, check_load, load, save_from_snapshot};
use klotho_world::{World, WorldSnapshot, WorldView};

use crate::event::DebugEvent;
use crate::player::{Played, TracePlayer};

/// Named capture taken from a snapshot. Not a Trace event.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CaptureMark {
    /// Capture point name.
    pub name: Name,
    /// Tick the snapshot was published.
    pub tick: Tick,
    /// Canon hash of the snapshot.
    pub canon_hash: Hash,
    /// Trace prefix of the snapshot.
    pub prefix: Hash,
}

/// Read-only journey host over [`TracePlayer`].
///
/// Inputs enter only as device samples or verb fixtures. Agency is stamped by
/// [`TestInputAdapter`]. Projection writes happen only inside `CommitKernel::step`.
pub struct JourneyKernel {
    player: TracePlayer,
    adapter: TestInputAdapter,
    names: BTreeMap<Name, Sigil>,
    saves: BTreeMap<Name, SaveQuad>,
    captures: Vec<CaptureMark>,
}

impl JourneyKernel {
    /// Wrap an already-seeded kernel. Does not expose `world_mut`.
    #[must_use]
    pub fn wrap(kernel: CommitKernel, adapter: TestInputAdapter) -> Self {
        Self {
            player: TracePlayer::new(kernel),
            adapter,
            names: BTreeMap::new(),
            saves: BTreeMap::new(),
            captures: Vec::new(),
        }
    }

    /// Empty Canon, one bound actor named `player`.
    #[must_use]
    pub fn seeded() -> Self {
        let d: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&d).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let s = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        k.bind_player(PlayerId(0), s);
        k.world_mut().insert_locus(s, LocusKind::Actor).unwrap();
        let mut host = Self::wrap(k, TestInputAdapter::hearth());
        host.bind_name(Name::from("player"), s);
        host
    }

    /// Record a name → packed id mapping for assertions.
    pub fn bind_name(&mut self, name: Name, sigil: Sigil) {
        self.names.insert(name, sigil);
    }

    /// Packed id for an authoring name.
    #[must_use]
    pub fn sigil(&self, name: &Name) -> Option<Sigil> {
        self.names.get(name).copied()
    }

    /// Live read view.
    #[must_use]
    pub fn view(&self) -> WorldView<'_> {
        self.player.kernel().world().view()
    }

    /// Current prefix.
    #[must_use]
    pub fn prefix_hash(&self) -> Hash {
        self.player.prefix_hash()
    }

    /// Capture marks recorded so far.
    #[must_use]
    pub fn captures(&self) -> &[CaptureMark] {
        &self.captures
    }

    /// Map a device sample and ingest it as `Proposal::Player`.
    pub fn apply_device(&mut self, sample: &DeviceSample) -> Result<DebugEvent, KernelFault> {
        let intent = self.adapter.map(sample);
        self.play_intent(intent)
    }

    /// Stamp a fixture (no Agency field) and ingest it.
    pub fn apply_fixture(
        &mut self,
        player: PlayerId,
        verb: Verb,
        target: IntentTarget,
        analog: Analog,
    ) -> Result<DebugEvent, KernelFault> {
        let tick = self.player.kernel().world().tick();
        let intent = self.adapter.stamp(player, tick, verb, target, analog);
        self.play_intent(intent)
    }

    /// Step `ticks` with no new player packet.
    pub fn wait(&mut self, ticks: u32) -> Result<Vec<DebugEvent>, KernelFault> {
        let mut out = Vec::with_capacity(ticks as usize);
        for _ in 0..ticks {
            out.push(self.player.step(&mut [])?);
        }
        Ok(out)
    }

    /// Publish a snapshot and record a capture mark. Does not append Trace.
    pub fn capture(&mut self, name: Name) -> CaptureMark {
        let snap = self.snapshot();
        let mark = CaptureMark {
            name,
            tick: snap.tick,
            canon_hash: snap.canon_hash,
            prefix: snap.trace_prefix_hash,
        };
        self.captures.push(mark.clone());
        mark
    }

    /// Save the current snapshot under `slot`.
    pub fn save(&mut self, slot: Name) {
        let snap = self.snapshot();
        self.saves.insert(slot, save_from_snapshot(&snap));
    }

    /// Load a previously saved slot. Prefix/canon must match the save.
    pub fn load(&mut self, slot: &Name) -> Result<Arc<WorldSnapshot>, String> {
        let quad = self
            .saves
            .get(slot)
            .cloned()
            .ok_or_else(|| format!("unknown save {}", slot.as_str()))?;
        let prefix = quad.trace_prefix_hash;
        let canon = quad.canon_hash;
        check_load(&quad, prefix, canon).map_err(|e| e.to_string())?;
        load(quad, prefix, canon).map_err(|e| e.to_string())
    }

    /// Rel fact by authoring names.
    #[must_use]
    pub fn has_rel(&self, a: &Name, rel: Rel, b: &Name) -> bool {
        let (Some(sa), Some(sb)) = (self.sigil(a), self.sigil(b)) else {
            return false;
        };
        self.view().has_rel(sa, rel, sb)
    }

    /// Knows bit by authoring mind name.
    #[must_use]
    pub fn knows(&self, mind: &Name, fact: u16) -> bool {
        self.sigil(mind).is_some_and(|s| self.view().knows(s, fact))
    }

    /// Minimize a failing intent script. Replay of the result still fails `pred`.
    pub fn minimize(
        seed: impl Fn() -> Self,
        intents: &[PlayerIntent],
        pred: impl Fn(&Played) -> bool,
    ) -> Result<Vec<PlayerIntent>, KernelFault> {
        let mut kept = intents.to_vec();
        if !fails(&seed, &kept, &pred)? {
            return Ok(kept);
        }
        let mut i = 0;
        while i < kept.len() {
            let mut trial = kept.clone();
            trial.remove(i);
            if fails(&seed, &trial, &pred)? {
                kept = trial;
            } else {
                i += 1;
            }
        }
        Ok(kept)
    }

    fn snapshot(&mut self) -> Arc<WorldSnapshot> {
        self.player.kernel_mut().snapshot()
    }

    fn play_intent(&mut self, intent: PlayerIntent) -> Result<DebugEvent, KernelFault> {
        debug_assert!(
            matches!(Proposal::Player(intent.clone()), Proposal::Player(_)),
            "journey kernel admits PlayerIntent only"
        );
        let played = self.player.play(&[intent])?;
        Ok(played.events.into_iter().next().expect("one step"))
    }
}

fn fails(
    seed: &impl Fn() -> JourneyKernel,
    intents: &[PlayerIntent],
    pred: &impl Fn(&Played) -> bool,
) -> Result<bool, KernelFault> {
    let mut host = seed();
    let played = host.player.play(intents)?;
    Ok(pred(&played))
}

#[cfg(test)]
mod tests {
    use klotho_core::{PlayerId, Tick};
    use klotho_input::Button;
    use klotho_ir::{Agency, Analog, IntentTarget, Verb};

    use super::*;

    #[test]
    fn device_path_does_not_accept_agency() {
        let mut host = JourneyKernel::seeded();
        let mut sample = DeviceSample::new(PlayerId(0), Tick(0));
        sample.buttons.insert(Button::KeyE);
        let ev = host.apply_device(&sample).unwrap();
        assert_eq!(ev.tick, Tick(1));
        assert_eq!(host.prefix_hash(), host.player.prefix_hash());
    }

    #[test]
    fn fixture_agency_is_stamped_not_supplied() {
        let mut host = JourneyKernel::seeded();
        host.apply_fixture(
            PlayerId(0),
            Verb::Time,
            IntentTarget::None,
            Analog::default(),
        )
        .unwrap();
        // Type-level: apply_fixture has no Agency argument.
        let _: fn(&mut JourneyKernel, PlayerId, Verb, IntentTarget, Analog) -> _ =
            JourneyKernel::apply_fixture;
    }

    #[test]
    fn capture_does_not_append_trace() {
        let mut host = JourneyKernel::seeded();
        let before = host.player.kernel().world().trace().events().len();
        let mark = host.capture(Name::from("checkpoint"));
        let after = host.player.kernel().world().trace().events().len();
        assert_eq!(before, after);
        assert_eq!(mark.name.as_str(), "checkpoint");
        assert_eq!(host.captures().len(), 1);
    }

    #[test]
    fn minimized_failure_replays() {
        let look = PlayerIntent {
            player: PlayerId(0),
            at: Tick(0),
            verb: Verb::Look,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: Agency::none(),
        };
        let script = vec![look.clone(), look.clone(), look];
        let pred = |played: &Played| played.events.len() >= 2;
        let mini = JourneyKernel::minimize(JourneyKernel::seeded, &script, pred).unwrap();
        assert_eq!(mini.len(), 2);
        let mut host = JourneyKernel::seeded();
        let replayed = host.player.play(&mini).unwrap();
        assert!(pred(&replayed));
        assert_eq!(replayed.events.len(), mini.len());
    }
}
