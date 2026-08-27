//! CommitKernel: ingest, K18 order, K21 speculate, admit or nack.

use std::collections::BTreeMap;
use std::sync::Arc;

use klotho_canon::Canon;
use klotho_core::{Budget, KernelFault, PlayerId, RejectReason, Sigil, Tick, Vel3};
use klotho_ir::{Channel, IntentTarget, SourceKind, Verb};
use klotho_trace::{ISLAND_SNAP_PERIOD_TICKS, IslandSnap, TraceBody, TraceDelta, TraceEvent};
use klotho_world::{World, WorldMut, WorldSnapshot, WorldView};

use crate::admit::{AdmitBuf, SyncProposer};
use crate::laws::admit_laws;
use crate::proposal::Proposal;
use crate::rite::drive_rite;
use crate::swept::check_space;

/// Only this type commits. Everyone else proposes.
pub struct CommitKernel {
    world: World,
    heap: Vec<Proposal>,
    players: BTreeMap<PlayerId, Sigil>,
}

impl CommitKernel {
    /// Wrap a world. Bind players with [`Self::bind_player`].
    #[must_use]
    pub fn new(world: World) -> Self {
        Self {
            world,
            heap: Vec::new(),
            players: BTreeMap::new(),
        }
    }

    /// Map a local player slot to a locus.
    pub fn bind_player(&mut self, id: PlayerId, s: Sigil) {
        self.players.insert(id, s);
    }

    /// Live world (read).
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Write path (seed, tests). Runtime tick writes go through [`Self::step`].
    pub fn world_mut(&mut self) -> WorldMut<'_> {
        self.world.mutate()
    }

    /// Frozen Canon.
    #[must_use]
    pub fn canon(&self) -> &Canon {
        self.world.canon()
    }

    /// Enqueue a proposal for the next [`Self::step`].
    pub fn ingest(&mut self, p: Proposal) {
        self.heap.push(p);
    }

    /// Publish a snapshot.
    #[must_use]
    pub fn snapshot(&mut self) -> Arc<WorldSnapshot> {
        self.world.snapshot()
    }

    /// One tick. Legal rejects are in the delta. `Err` is a kernel bug.
    pub fn step(
        &mut self,
        dt: Tick,
        budget: Budget,
        sync: &mut [&mut dyn SyncProposer],
    ) -> Result<TraceDelta, KernelFault> {
        let tick = self.world.tick().saturating_add(dt.0.max(1));
        self.world.mutate().set_tick(tick);

        let mut buf = AdmitBuf::new();
        {
            let view = self.world.view();
            for p in sync.iter_mut() {
                p.propose(&view, dt, &mut buf);
            }
        }
        let mut batch = core::mem::take(&mut self.heap);
        batch.extend(buf.drain());
        batch.sort_by_key(|p| p.order_key());

        let mut delta = TraceDelta::empty(tick);
        let mut written: BTreeMap<(u128, u8), ()> = BTreeMap::new();
        let mut pred_ops = budget.pred_ops;
        let mut rite_steps = budget.rite_steps;

        for p in batch {
            let kind = p.kind();
            match self.admit_one(p, tick, &mut pred_ops, &mut rite_steps, &mut written) {
                Ok(events) => delta.events.extend(events),
                Err(r) => delta.rejects.push((kind, r)),
            }
        }

        if tick.0 % ISLAND_SNAP_PERIOD_TICKS == 0 {
            for ev in island_snaps(self.world.view(), tick) {
                self.world.mutate().append(ev.clone());
                delta.events.push(ev);
            }
        }

        let snap = self.world.snapshot();
        delta.snap_bytes = u32::try_from(snap.approx_bytes()).unwrap_or(u32::MAX);
        let _ = budget.us_sim;
        Ok(delta)
    }

    fn admit_one(
        &mut self,
        p: Proposal,
        tick: Tick,
        pred_ops: &mut u32,
        rite_steps: &mut u32,
        written: &mut BTreeMap<(u128, u8), ()>,
    ) -> Result<Vec<TraceEvent>, RejectReason> {
        let (actor, target, verb, source, claimed, swept_hits) = self.preflight(&p, tick)?;

        let cells = write_cells(&p, actor);
        for c in &cells {
            if written.contains_key(c) {
                return Err(RejectReason::Conflict);
            }
        }

        let mut spec = self.world.mutate().begin_spec();
        match &p {
            Proposal::Player(_) | Proposal::Mind(_) | Proposal::Infer(_) => {
                drive_rite(
                    &mut spec,
                    self.world.canon(),
                    actor,
                    target,
                    verb,
                    source,
                    claimed.as_slice(),
                    rite_steps,
                    pred_ops,
                    tick,
                )?;
            }
            Proposal::SpaceDelta {
                mover,
                pose,
                vel,
                yaw_rate,
                island,
                sleep_ticks,
                ..
            }
            | Proposal::MotionDelta {
                mover,
                pose,
                vel,
                yaw_rate,
                island,
                sleep_ticks,
                ..
            } => {
                spec.set_pose(*mover, *pose)
                    .map_err(|_| RejectReason::Budget)?;
                spec.set_vel(*mover, *vel, *yaw_rate)
                    .map_err(|_| RejectReason::Budget)?;
                spec.set_island(*mover, *island, *sleep_ticks)
                    .map_err(|_| RejectReason::Budget)?;
            }
        }

        admit_laws(
            self.world.canon(),
            &spec.view(),
            actor,
            target,
            verb,
            source,
            claimed.as_slice(),
            swept_hits,
            pred_ops,
        )?;

        let events: Vec<TraceEvent> = spec.events().to_vec();
        self.world.mutate().commit_spec(spec);
        for c in cells {
            written.insert(c, ());
        }
        Ok(events)
    }

    #[allow(clippy::type_complexity)]
    fn preflight(
        &self,
        p: &Proposal,
        tick: Tick,
    ) -> Result<(Sigil, Option<Sigil>, Verb, SourceKind, Vec<Channel>, bool), RejectReason> {
        match p {
            Proposal::Player(pi) => {
                if tick.0.saturating_sub(pi.at.0) > self.slo() {
                    return Err(RejectReason::StaleEpoch);
                }
                let actor = *self.players.get(&pi.player).ok_or(RejectReason::Budget)?;
                let target = resolve_target(&pi.target, self.world.canon());
                if pi.verb == Verb::Time && pi.agency.claimed.is_empty() {
                    return Err(RejectReason::UnclaimedAgency);
                }
                Ok((
                    actor,
                    target,
                    pi.verb,
                    SourceKind::Player,
                    pi.agency.claimed.clone(),
                    false,
                ))
            }
            Proposal::Mind(m) => {
                if m.verb == Verb::Time {
                    return Err(RejectReason::UnclaimedAgency);
                }
                let target = resolve_target(&m.target, self.world.canon());
                Ok((m.locus, target, m.verb, SourceKind::Mind, Vec::new(), false))
            }
            Proposal::Infer(inf) => {
                if inf.verb == Verb::Time {
                    return Err(RejectReason::UnclaimedAgency);
                }
                let actor = inf.locus.ok_or(RejectReason::HallucinatedFact)?;
                if !inf.claimed_facts.is_empty() {
                    return Err(RejectReason::HallucinatedFact);
                }
                let target = resolve_target(&inf.target, self.world.canon());
                Ok((
                    actor,
                    target,
                    inf.verb,
                    SourceKind::Infer,
                    Vec::new(),
                    false,
                ))
            }
            Proposal::SpaceDelta {
                mover,
                pose,
                hull,
                witness,
                ..
            } => {
                if witness.mover != *mover {
                    return Err(RejectReason::WrongHull);
                }
                let hits = check_space(
                    &self.world.view(),
                    *mover,
                    *pose,
                    *hull,
                    witness.overlaps_closed_opaque,
                )?;
                Ok((
                    *mover,
                    None,
                    Verb::Move,
                    SourceKind::Space,
                    Vec::new(),
                    hits,
                ))
            }
            Proposal::MotionDelta {
                mover,
                pose,
                hull,
                witness,
                ..
            } => {
                if witness.mover != *mover {
                    return Err(RejectReason::WrongHull);
                }
                let hits = check_space(
                    &self.world.view(),
                    *mover,
                    *pose,
                    *hull,
                    witness.overlaps_closed_opaque,
                )?;
                Ok((
                    *mover,
                    None,
                    Verb::Move,
                    SourceKind::Motion,
                    Vec::new(),
                    hits,
                ))
            }
        }
    }

    fn slo(&self) -> u64 {
        // Bound from last snapshot budget is per-step; default 12.
        u64::from(Budget::HEARTH.eval_slo_ticks)
    }
}

fn resolve_target(t: &IntentTarget, canon: &Canon) -> Option<Sigil> {
    match t {
        IntentTarget::None => None,
        IntentTarget::Sigil(s) => Some(*s),
        IntentTarget::Name(n) => canon.pin(n.as_str()),
    }
}

fn write_cells(p: &Proposal, actor: Sigil) -> Vec<(u128, u8)> {
    match p {
        Proposal::Player(_) | Proposal::Mind(_) | Proposal::Infer(_) => {
            vec![(actor.raw(), 1)]
        }
        Proposal::SpaceDelta { mover, .. } | Proposal::MotionDelta { mover, .. } => {
            vec![(mover.raw(), 0)]
        }
    }
}

fn island_snaps(view: WorldView<'_>, tick: Tick) -> Vec<TraceEvent> {
    let mut by_island: BTreeMap<u16, IslandSnap> = BTreeMap::new();
    for s in view.loci() {
        let Some((island, sleep)) = view.island(s) else {
            continue;
        };
        if sleep != 0 {
            continue;
        }
        let Some(pose) = view.pose(s) else {
            continue;
        };
        let (vel, yaw_rate) = view.vel(s).unwrap_or((Vel3::ZERO, 0));
        let snap = by_island.entry(island).or_insert_with(|| IslandSnap {
            island,
            members: Vec::new(),
            poses: Vec::new(),
            vels: Vec::new(),
            yaw_rates: Vec::new(),
            sleep_ticks: Vec::new(),
        });
        snap.members.push(s);
        snap.poses.push(pose);
        snap.vels.push(vel);
        snap.yaw_rates.push(yaw_rate);
        snap.sleep_ticks.push(0);
    }
    by_island
        .into_values()
        .map(|snap| TraceEvent::new(tick, TraceBody::IslandSnap(snap)))
        .collect()
}
