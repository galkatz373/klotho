//! CommitKernel: ingest, K18 order, K21 speculate, admit or nack.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use klotho_canon::Canon;
use klotho_core::{Budget, KernelFault, NO_ISLAND, PlayerId, RejectReason, Sigil, Tick, Vel3};
use klotho_ir::{Channel, IntentTarget, Rel, SourceKind, Verb};
use klotho_trace::{
    ISLAND_SNAP_PERIOD_TICKS, IslandSnap, ProposalKind, TraceBody, TraceDelta, TraceEvent,
};
use klotho_world::{World, WorldMut, WorldSnapshot, WorldView};

use crate::admit::{AdmitBuf, SyncProposer};
use crate::laws::admit_laws;
use crate::partition::partition_islands;
use crate::proposal::{Proposal, ResidencyOp};
use crate::rite::drive_rite;
use crate::swept::check_space;

/// Only this type commits. Everyone else proposes.
pub struct CommitKernel {
    world: World,
    heap: Vec<(Proposal, u8)>,
    players: BTreeMap<PlayerId, Sigil>,
    /// Fail-closed partition rejects, flushed on the next [`Self::step`].
    partition_rejects: Vec<(ProposalKind, RejectReason)>,
}

impl CommitKernel {
    /// Wrap a world. Bind players with [`Self::bind_player`].
    #[must_use]
    pub fn new(world: World) -> Self {
        Self {
            world,
            heap: Vec::new(),
            players: BTreeMap::new(),
            partition_rejects: Vec::new(),
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
        self.ingest_from(p, 0);
    }

    /// Enqueue a proposal tagged with its static proposer registration index.
    pub fn ingest_from(&mut self, p: Proposal, proposer_reg_ix: u8) {
        self.heap.push((p, proposer_reg_ix));
    }

    /// K58: rewrite this-tick `island` ids from the live view. Not an admit.
    ///
    /// Non-members get [`NO_ISLAND`]. Oversize / too-many groups are omitted
    /// and recorded as legal rejects on the next [`Self::step`].
    pub fn partition(&mut self) -> Vec<(u16, Vec<Sigil>)> {
        let part = partition_islands(&self.world.view());
        self.partition_rejects.clear();
        if part.omitted_too_large > 0 {
            self.partition_rejects
                .push((ProposalKind::Phys, RejectReason::IslandTooLarge));
        }
        if part.omitted_too_many > 0 {
            self.partition_rejects
                .push((ProposalKind::Phys, RejectReason::TooManyIslands));
        }
        let islands = part.islands;
        let writes: Vec<(Sigil, u16, u16)> = {
            let view = self.world.view();
            let mut assigned: BTreeMap<Sigil, u16> = BTreeMap::new();
            for (id, members) in &islands {
                for s in members {
                    assigned.insert(*s, *id);
                }
            }
            view.loci()
                .map(|s| {
                    let sleep = view.island(s).map(|(_, t)| t).unwrap_or(0);
                    let island = assigned.get(&s).copied().unwrap_or(NO_ISLAND);
                    (s, island, sleep)
                })
                .collect()
        };
        let mut w = self.world.mutate();
        for (s, island, sleep) in writes {
            w.set_island(s, island, sleep)
                .expect("partition: packed locus vanished");
        }
        islands
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

        let mut batch = core::mem::take(&mut self.heap);
        {
            let view = self.world.view();
            for (ix, p) in sync.iter_mut().enumerate() {
                let mut buf = AdmitBuf::new();
                p.propose(&view, dt, &mut buf);
                let Some(reg) = u8::try_from(ix).ok() else {
                    break;
                };
                batch.extend(buf.drain().into_iter().map(|prop| (prop, reg)));
            }
        }
        batch.sort_by_key(|(p, ix)| p.admit_key(*ix));

        let mut delta = TraceDelta::empty(tick);
        delta
            .rejects
            .extend(core::mem::take(&mut self.partition_rejects));
        let mut written: BTreeMap<(u128, u8), ()> = BTreeMap::new();
        let mut pred_ops = budget.pred_ops;
        let mut rite_steps = budget.rite_steps;
        let t_admit = Instant::now();

        for (p, _) in batch {
            let kind = p.kind();
            if t_admit.elapsed().as_micros() >= u128::from(budget.us_sim) {
                delta.rejects.push((kind, RejectReason::Budget));
                continue;
            }
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

        let cells = write_cells(&p, actor, &self.world.view());
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
            Proposal::Residency {
                place, op, snap, ..
            } => match op {
                ResidencyOp::Load => {
                    spec.apply_place_snap(snap)
                        .map_err(|_| RejectReason::Residency)?;
                }
                ResidencyOp::Evict => {
                    spec.evict_place(*place)
                        .map_err(|_| RejectReason::Residency)?;
                }
            },
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
            Proposal::Residency {
                place,
                op,
                prefix,
                canon_hash,
                snap,
            } => {
                if *canon_hash != self.world.canon_hash()
                    || *prefix != self.world.trace_prefix_hash()
                {
                    return Err(RejectReason::EpochMismatch);
                }
                // Snap prefix is capture identity, not the live world's prefix.
                if snap.place != *place || snap.canon_hash != *canon_hash {
                    return Err(RejectReason::Residency);
                }
                if snap.len() > klotho_world::MAX_PLACE_ROWS {
                    return Err(RejectReason::Residency);
                }
                if *op == ResidencyOp::Evict && !self.world.view().contains(*place) {
                    return Err(RejectReason::Residency);
                }
                Ok((
                    *place,
                    None,
                    Verb::Look,
                    SourceKind::Residency,
                    Vec::new(),
                    false,
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

fn write_cells(p: &Proposal, actor: Sigil, view: &WorldView<'_>) -> Vec<(u128, u8)> {
    match p {
        Proposal::Player(_) | Proposal::Mind(_) | Proposal::Infer(_) => {
            vec![(actor.raw(), 1)]
        }
        Proposal::SpaceDelta { mover, .. } | Proposal::MotionDelta { mover, .. } => {
            let mut cells = vec![(mover.raw(), 0)];
            for child in attached_children(view, *mover) {
                cells.push((child.raw(), 0));
            }
            cells
        }
        Proposal::Residency {
            place, op, snap, ..
        } => {
            let mut cells = vec![(place.raw(), 0)];
            for row in snap.rows() {
                cells.push((row.sigil.raw(), 0));
            }
            if *op == ResidencyOp::Evict {
                for s in view.loci() {
                    if view.has_rel(s, Rel::In, *place) {
                        cells.push((s.raw(), 0));
                    }
                }
            }
            cells
        }
    }
}

fn attached_children(view: &WorldView<'_>, parent: Sigil) -> Vec<Sigil> {
    let mut out: Vec<Sigil> = view
        .loci()
        .filter(|&s| {
            s != parent
                && (view.has_rel(s, Rel::PilotedBy, parent)
                    || view.has_rel(s, Rel::AttachedTo, parent))
        })
        .collect();
    out.sort_unstable();
    out
}

fn island_snaps(view: WorldView<'_>, tick: Tick) -> Vec<TraceEvent> {
    let mut by_island: BTreeMap<u16, IslandSnap> = BTreeMap::new();
    for s in view.loci() {
        let Some((island, sleep)) = view.island(s) else {
            continue;
        };
        if island == NO_ISLAND || sleep != 0 {
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
