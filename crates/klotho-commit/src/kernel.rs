//! CommitKernel: ingest, K18 order, K21 speculate, admit or nack.

use std::collections::BTreeMap;
use std::sync::Arc;

use klotho_canon::Canon;
use klotho_core::{
    Budget, IVec3, KernelFault, Mm, NO_ISLAND, PlayerId, PoseMm, RejectReason, Sigil, Tick, Vel3,
    YawMd, look_offset, rotate_xz,
};
use klotho_ir::{Channel, IntentTarget, PlayerIntent, Rel, SourceKind, Verb};
use klotho_trace::{
    ISLAND_SNAP_PERIOD_TICKS, IslandSnap, ProposalKind, TraceBody, TraceDelta, TraceEvent,
};
use klotho_world::{HITSCAN_RANGE_MM, RewindRing, World, WorldMut, WorldSnapshot, WorldView};

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
    ring: RewindRing,
    last_rewind_ticks_used: u16,
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
            ring: RewindRing::new(0),
            last_rewind_ticks_used: 0,
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

    /// Last `rewind_ticks` published snapshots. Unhashed, not in Trace.
    #[must_use]
    pub fn rewind_ring(&self) -> &RewindRing {
        &self.ring
    }

    /// [`crate::METRIC_REWIND_TICKS_USED`] from the last [`Self::step`].
    #[must_use]
    pub fn last_rewind_ticks_used(&self) -> u16 {
        self.last_rewind_ticks_used
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
        self.ring.set_cap(budget.rewind_ticks);
        self.last_rewind_ticks_used = 0;

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
        // Single left-to-right pass over the key-sorted batch. Consecutive
        // equal keys form one producer-equivalence class (the sort above
        // groups them). No arrival-order fallback is permitted on the commit
        // path. A class of distinct grains is ambiguous: every member loses
        // with Conflict, deterministically. A class of identical grains is
        // an idempotent retry, not ambiguity: the first admits and the rest
        // drop silently, so a retried ingest has exactly-once effect.
        let mut rest = batch.into_iter().peekable();
        while let Some((first, first_ix)) = rest.next() {
            let key = first.admit_key(first_ix);
            let mut class = vec![(first, first_ix)];
            while rest.peek().is_some_and(|(q, qix)| q.admit_key(*qix) == key) {
                class.push(rest.next().expect("peeked class member"));
            }
            if class[1..].iter().any(|(q, _)| *q != class[0].0) {
                for (p, _) in &class {
                    delta.rejects.push((p.kind(), RejectReason::Conflict));
                }
                continue;
            }
            // Identical retries collapse beyond the first element.
            let (p, _) = class.into_iter().next().expect("nonempty class");
            let kind = p.kind();
            match self.admit_one(
                p,
                tick,
                budget,
                &mut pred_ops,
                &mut rite_steps,
                &mut written,
            ) {
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
        self.ring.push(Arc::clone(&snap));
        delta.snap_bytes = u32::try_from(snap.approx_bytes()).unwrap_or(u32::MAX);
        Ok(delta)
    }

    fn admit_one(
        &mut self,
        p: Proposal,
        tick: Tick,
        budget: Budget,
        pred_ops: &mut u32,
        rite_steps: &mut u32,
        written: &mut BTreeMap<(u128, u8), ()>,
    ) -> Result<Vec<TraceEvent>, RejectReason> {
        let (actor, mut target, verb, source, claimed, swept_hits) =
            self.preflight(&p, tick, budget)?;
        if let Proposal::Player(pi) = &p {
            if budget.rewind_ticks > 0 && pi.verb == Verb::Fire {
                target = self.rewind_hitscan(pi, actor, tick)?;
                self.last_rewind_ticks_used =
                    u16::try_from(tick.0.saturating_sub(pi.at.0)).unwrap_or(u16::MAX);
            }
        }

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
            Proposal::PhysDelta {
                mover,
                pose,
                vel,
                yaw_rate,
                pitch_rate,
                roll_rate,
                island,
                sleep_ticks,
                support,
                ..
            } => {
                let children = attached_children(&spec.view(), *mover);
                let locals: Vec<(Sigil, IVec3)> = children
                    .iter()
                    .map(|&child| {
                        let local = spec
                            .view()
                            .attach_local(child)
                            .unwrap_or_else(|| default_attach_local(&spec.view(), child, *mover));
                        (child, local)
                    })
                    .collect();
                spec.set_pose(*mover, *pose)
                    .map_err(|_| RejectReason::Budget)?;
                spec.set_vel(*mover, *vel, *yaw_rate)
                    .map_err(|_| RejectReason::Budget)?;
                spec.set_rates(*mover, *yaw_rate, *pitch_rate, *roll_rate)
                    .map_err(|_| RejectReason::Budget)?;
                spec.set_island(*mover, *island, *sleep_ticks)
                    .map_err(|_| RejectReason::Budget)?;
                spec.set_support(*mover, *support)
                    .map_err(|_| RejectReason::Budget)?;
                spec.clear_phys_req(*mover)
                    .map_err(|_| RejectReason::Budget)?;
                for (child, local) in locals {
                    spec.set_pose(child, compose_yaw_only(*pose, local))
                        .map_err(|_| RejectReason::Budget)?;
                    spec.clear_phys_req(child)
                        .map_err(|_| RejectReason::Budget)?;
                }
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
        budget: Budget,
    ) -> Result<(Sigil, Option<Sigil>, Verb, SourceKind, Vec<Channel>, bool), RejectReason> {
        match p {
            Proposal::Player(pi) => {
                let actor = *self.players.get(&pi.player).ok_or(RejectReason::Budget)?;
                let target = resolve_target(&pi.target, self.world.canon());
                let age = tick.0.saturating_sub(pi.at.0);
                let rewind = u64::from(budget.rewind_ticks);
                let combat = pi.verb == Verb::Fire
                    || (pi.verb == Verb::Use
                        && target
                            .is_some_and(|t| hittable_target(&self.world.view(), self.canon(), t)));
                if rewind > 0 && combat {
                    if age > rewind {
                        return Err(RejectReason::StaleEpoch);
                    }
                } else if age > u64::from(budget.eval_slo_ticks) {
                    return Err(RejectReason::StaleEpoch);
                }
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
            Proposal::PhysDelta {
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
                Ok((*mover, None, Verb::Move, SourceKind::Phys, Vec::new(), hits))
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

    fn rewind_hitscan(
        &self,
        pi: &PlayerIntent,
        actor: Sigil,
        tick: Tick,
    ) -> Result<Option<Sigil>, RejectReason> {
        let age = tick.0.saturating_sub(pi.at.0);
        if age == 0 {
            return Ok(hitscan_fire(&self.world.view(), self.canon(), pi, actor));
        }
        let snap = self
            .ring
            .lookup(tick, pi.at)
            .ok_or(RejectReason::StaleEpoch)?;
        Ok(hitscan_fire(&snap.view(), self.canon(), pi, actor))
    }
}

fn hitscan_fire(
    view: &WorldView<'_>,
    canon: &Canon,
    pi: &PlayerIntent,
    actor: Sigil,
) -> Option<Sigil> {
    let hittable = canon.affordance_id("Hittable")?;
    let pose = view.pose(actor).unwrap_or_default();
    let yaw = pose.yaw.wrapping_add(pi.analog.look_yaw);
    let pitch = YawMd(pose.pitch.0.saturating_add(pi.analog.look_pitch));
    view.hitscan(
        pose.translation(),
        look_offset(yaw, pitch, HITSCAN_RANGE_MM),
        actor,
        hittable,
    )
}

fn hittable_target(view: &WorldView<'_>, canon: &Canon, t: Sigil) -> bool {
    canon
        .affordance_id("Hittable")
        .is_some_and(|id| view.has_affordance(t, id))
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
        Proposal::PhysDelta { mover, .. }
        | Proposal::SpaceDelta { mover, .. }
        | Proposal::MotionDelta { mover, .. } => {
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

fn compose_yaw_only(parent: PoseMm, local: IVec3) -> PoseMm {
    let r = rotate_xz(local, parent.yaw);
    PoseMm {
        x: Mm(parent.x.0.wrapping_add(r.x)),
        y: Mm(parent.y.0.wrapping_add(r.y)),
        z: Mm(parent.z.0.wrapping_add(r.z)),
        yaw: parent.yaw,
        pitch: parent.pitch,
        roll: parent.roll,
    }
}

fn default_attach_local(view: &WorldView<'_>, child: Sigil, parent: Sigil) -> IVec3 {
    let child_p = view.pose(child).unwrap_or_default();
    let parent_p = view.pose(parent).unwrap_or_default();
    let delta = IVec3 {
        x: child_p.x.0.wrapping_sub(parent_p.x.0),
        y: child_p.y.0.wrapping_sub(parent_p.y.0),
        z: child_p.z.0.wrapping_sub(parent_p.z.0),
    };
    rotate_xz(delta, -parent_p.yaw)
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
