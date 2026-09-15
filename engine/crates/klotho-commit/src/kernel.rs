//! CommitKernel: ingest, K18 order, K21 speculate, admit or nack.

use std::collections::BTreeMap;
use std::sync::Arc;

use klotho_canon::{Canon, EpochMap};
use klotho_core::{
    Budget, ConstraintState, Epoch, Hash, IVec3, KernelFault, LocusKind, Mm, NO_ISLAND, PlayerId,
    PoseMm, RejectReason, Sigil, Tick, Vel3, YawMd, look_offset, rotate_xz,
};
use klotho_ir::{Channel, IntentTarget, PlayerIntent, Rel, SourceKind, Verb};
use klotho_trace::{
    ISLAND_SNAP_PERIOD_TICKS, IslandSnap, ProposalKind, RiteEnd, TraceBody, TraceDelta, TraceEvent,
};
use klotho_world::{
    HITSCAN_RANGE_MM, RewindRing, SpecDelta, World, WorldMut, WorldSnapshot, WorldView,
};

use crate::admit::{AdmitBuf, SyncProposer};
use crate::laws::{admit_laws, admit_phys_laws};
use crate::partition::partition_islands;
use crate::proposal::{
    BodyDelta, ContactClaim, MAX_PHYS_ISLAND_BODIES, MAX_PHYS_ISLAND_BREAKS,
    MAX_PHYS_ISLAND_CHILDREN, MAX_PHYS_ISLAND_CONSTRAINTS, MAX_PHYS_ISLAND_CONTACTS,
    MAX_PHYS_ISLAND_MEMBERS, MAX_PHYS_ISLAND_WRITE_LOCI, Proposal, ResidencyOp,
};
use crate::rite::drive_rite;
use crate::swept::{check_phys_body, check_space};

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

/// Why a halted Canon epoch transition was refused before any write.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum EpochApplyError {
    /// Pack ancestry does not name the live Canon hash.
    CanonMismatch,
    /// Pack ancestry or successor does not name the live/next epoch.
    EpochMismatch,
    /// A transition must mint a new Canon identity.
    UnchangedCanon,
}

impl core::fmt::Display for EpochApplyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CanonMismatch => write!(f, "CanonMismatch"),
            Self::EpochMismatch => write!(f, "EpochMismatch"),
            Self::UnchangedCanon => write!(f, "UnchangedCanon"),
        }
    }
}

impl core::error::Error for EpochApplyError {}

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

    /// Commit a pre-cooked Canon replacement while the runtime is halted.
    ///
    /// Pending proposals are from the old view and are discarded. Active
    /// `WAIT`s resume only when their stable Rite name and next `pc` exist in
    /// the replacement; all others end with [`RiteEnd::Evicted`].
    #[allow(clippy::too_many_arguments)]
    pub fn apply_canon_epoch(
        &mut self,
        from_canon_hash: Hash,
        from_epoch: Epoch,
        canon_hash: Hash,
        epoch: Epoch,
        canon: Arc<Canon>,
        map: &EpochMap,
    ) -> Result<TraceDelta, EpochApplyError> {
        if self.world.canon_hash() != from_canon_hash {
            return Err(EpochApplyError::CanonMismatch);
        }
        let expected = self
            .world
            .epoch()
            .0
            .checked_add(1)
            .map(Epoch)
            .ok_or(EpochApplyError::EpochMismatch)?;
        if self.world.epoch() != from_epoch || epoch != expected {
            return Err(EpochApplyError::EpochMismatch);
        }
        if canon_hash == from_canon_hash {
            return Err(EpochApplyError::UnchangedCanon);
        }

        self.heap.clear();
        self.partition_rejects.clear();
        let tick = self.world.tick();
        let evicted = self
            .world
            .mutate()
            .apply_canon_epoch(canon, canon_hash, epoch, map);
        let mut delta = TraceDelta::empty(tick);
        for (actor, rite) in evicted {
            let event = TraceEvent::new(
                tick,
                TraceBody::RiteEnded {
                    actor,
                    rite,
                    status: RiteEnd::Evicted,
                },
            );
            self.world.mutate().append(event.clone());
            delta.events.push(event);
        }
        let cap = self.ring.cap();
        self.ring = RewindRing::new(cap);
        let snap = self.world.snapshot();
        self.ring.push(Arc::clone(&snap));
        delta.snap_bytes = u32::try_from(snap.approx_bytes()).unwrap_or(u32::MAX);
        Ok(delta)
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
        if matches!(p, Proposal::PhysIsland { .. }) {
            return self.admit_phys_island(p, tick, pred_ops, rite_steps, written);
        }
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
            Proposal::PhysIsland { .. } => unreachable!("physical islands admit as a batch"),
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

    fn admit_phys_island(
        &mut self,
        p: Proposal,
        tick: Tick,
        pred_ops: &mut u32,
        rite_steps: &mut u32,
        written: &mut BTreeMap<(u128, u8), ()>,
    ) -> Result<Vec<TraceEvent>, RejectReason> {
        let Proposal::PhysIsland {
            epoch,
            tick: proposed_tick,
            island,
            members,
            bodies,
            contacts,
            motion_contacts,
            constraints,
            breaks,
        } = &p
        else {
            unreachable!("caller selected PhysIsland")
        };
        let swept_hits = self.validate_phys_island(
            *epoch,
            *proposed_tick,
            *island,
            members,
            bodies,
            contacts,
            constraints,
            breaks,
            tick,
        )?;
        let (mut cells, attachments) = phys_write_cells(bodies, constraints, &self.world.view())?;
        if cells.iter().any(|cell| written.contains_key(cell)) {
            return Err(RejectReason::Conflict);
        }

        crate::motion_contact::validate(&self.world.view(), bodies, motion_contacts)?;
        for body in bodies {
            if self.world.view().contact_window(body.mover).is_some() {
                cells.push((body.mover.raw(), 1));
            }
        }
        if cells.iter().any(|cell| written.contains_key(cell)) {
            return Err(RejectReason::Conflict);
        }
        let mut spec = self.world.mutate().begin_spec();
        for body in bodies {
            apply_body(&mut spec, *island, body)?;
        }
        for (child, parent, local) in attachments {
            let parent_pose = spec
                .view()
                .pose(parent)
                .ok_or(RejectReason::WitnessMismatch)?;
            spec.set_pose(child, compose_yaw_only(parent_pose, local))
                .map_err(|_| RejectReason::Budget)?;
            spec.clear_phys_req(child)
                .map_err(|_| RejectReason::Budget)?;
        }
        apply_constraints(&mut spec, constraints, breaks)?;
        crate::breakage::apply_constraint_breaks(&mut spec, self.world.canon(), breaks, tick)?;
        validate_contact_claims(&spec.view(), bodies, contacts)?;
        validate_character_geometry(&self.world.view(), &spec.view(), bodies)?;
        let law_bodies: Vec<(Sigil, bool)> = bodies
            .iter()
            .zip(swept_hits)
            .map(|(body, hits)| (body.mover, hits))
            .collect();

        let before_view = self.world.view();
        let contact_law_contexts: Vec<_> = bodies
            .iter()
            .filter_map(|b| {
                if let Some(claim) = motion_contacts.iter().find(|c| c.actor == b.mover) {
                    return Some((claim.actor, claim.target, claim.track.channel));
                }
                let track = before_view.contact_track(b.mover)?;
                let rite = self
                    .world
                    .canon()
                    .rites
                    .iter()
                    .find(|r| r.name.as_str() == track.rite)?;
                let machine = self.world.view().rite(b.mover, rite.id)?;
                machine
                    .contact_hit
                    .then_some((b.mover, machine.target?, machine.contact_agency))
            })
            .collect();
        crate::rite::admit_motion_windows(
            &mut spec,
            self.world.canon(),
            bodies,
            motion_contacts,
            rite_steps,
            pred_ops,
            tick,
        )?;
        admit_phys_laws(
            self.world.canon(),
            &spec.view(),
            &law_bodies,
            &contact_law_contexts,
            pred_ops,
        )?;
        // Capture every physical and semantic write before publication so a
        // Law, break, or spawn rejection leaves Projection byte-identical.
        // Lane 1 is action/constraint metadata, not every newly written locus.
        for locus in spec.write_loci() {
            cells.push((locus.raw(), 0));
        }
        if bodies
            .iter()
            .any(|b| before_view.contact_track(b.mover).is_some())
        {
            for locus in spec.write_loci() {
                cells.push((locus.raw(), 1));
            }
        }
        cells.sort_unstable();
        cells.dedup();
        if cells
            .iter()
            .map(|c| c.0)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > MAX_PHYS_ISLAND_WRITE_LOCI
        {
            return Err(RejectReason::IslandTooLarge);
        }
        if cells.iter().any(|cell| written.contains_key(cell)) {
            return Err(RejectReason::Conflict);
        }
        let events = spec.events().to_vec();
        self.world.mutate().commit_spec(spec);
        for cell in cells {
            written.insert(cell, ());
        }
        Ok(events)
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_phys_island(
        &self,
        epoch: Epoch,
        proposed_tick: Tick,
        island: u16,
        members: &[Sigil],
        bodies: &[BodyDelta],
        contacts: &[ContactClaim],
        constraints: &[crate::proposal::ConstraintRef],
        breaks: &[crate::proposal::ConstraintBreakClaim],
        tick: Tick,
    ) -> Result<Vec<bool>, RejectReason> {
        if epoch != self.world.epoch() || proposed_tick != tick {
            return Err(RejectReason::EpochMismatch);
        }
        if members.len() > MAX_PHYS_ISLAND_MEMBERS
            || bodies.len() > MAX_PHYS_ISLAND_BODIES
            || contacts.len() > MAX_PHYS_ISLAND_CONTACTS
            || constraints.len() > MAX_PHYS_ISLAND_CONSTRAINTS
            || breaks.len() > MAX_PHYS_ISLAND_BREAKS
        {
            return Err(RejectReason::IslandTooLarge);
        }
        if island == NO_ISLAND || members.is_empty() || bodies.is_empty() {
            return Err(RejectReason::WitnessMismatch);
        }
        if !strictly_increasing(members)
            || !strictly_increasing_by(bodies, |b| b.mover)
            || !strictly_increasing_by(contacts, ContactClaim::key)
            || !strictly_increasing_by(constraints, |c| c.constraint)
            || !strictly_increasing_by(breaks, |b| b.constraint)
        {
            return Err(RejectReason::WitnessMismatch);
        }
        let view = self.world.view();
        let mut expected: Vec<Sigil> = view
            .loci()
            .filter(|&s| view.island(s).map(|(id, _)| id) == Some(island))
            .collect();
        expected.sort_unstable();
        if expected != members {
            return Err(RejectReason::WitnessMismatch);
        }
        validate_constraint_headers(&view, members, constraints, breaks)?;

        let mut swept_hits = Vec::with_capacity(bodies.len());
        for body in bodies {
            if view.attach_parent(body.mover).is_some()
                || members.binary_search(&body.mover).is_err()
                || body.witness.mover != body.mover
                || body.witness.proposed != body.pose
            {
                return Err(RejectReason::WitnessMismatch);
            }
            swept_hits.push(check_phys_body(
                &view,
                body.mover,
                body.pose,
                body.hull,
                body.witness,
            )?);
        }
        // Every unattached Relic and explicitly driven Actor is required.
        // Attached rows derive from their parent in the same transaction.
        for member in members {
            if (view.kind(*member) == Some(LocusKind::Relic)
                || view.character_physics(*member).is_some())
                && view.attach_parent(*member).is_none()
                && bodies
                    .binary_search_by_key(member, |body| body.mover)
                    .is_err()
            {
                return Err(RejectReason::WitnessMismatch);
            }
        }
        validate_contact_headers(&view, members, bodies, contacts)?;
        Ok(swept_hits)
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
            Proposal::PhysIsland { .. } => unreachable!("physical islands preflight as a batch"),
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
                if self.world.view().character_physics(*mover).is_some() {
                    return Err(RejectReason::Conflict);
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
                if self.world.view().character_physics(*mover).is_some() {
                    return Err(RejectReason::Conflict);
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
        Proposal::PhysIsland { bodies, .. } => {
            let mut cells = Vec::new();
            for body in bodies {
                cells.push((body.mover.raw(), 0));
                for child in attached_children(view, body.mover) {
                    cells.push((child.raw(), 0));
                }
            }
            cells.sort_unstable();
            cells.dedup();
            cells
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

fn strictly_increasing<T: Ord>(items: &[T]) -> bool {
    items.windows(2).all(|pair| pair[0] < pair[1])
}

fn strictly_increasing_by<T, K: Ord>(items: &[T], key: impl Fn(&T) -> K) -> bool {
    items.windows(2).all(|pair| key(&pair[0]) < key(&pair[1]))
}

type WriteCell = (u128, u8);
type AttachmentWrite = (Sigil, Sigil, IVec3);
type PhysWriteSet = (Vec<WriteCell>, Vec<AttachmentWrite>);

fn validate_constraint_headers(
    view: &WorldView<'_>,
    members: &[Sigil],
    constraints: &[crate::proposal::ConstraintRef],
    breaks: &[crate::proposal::ConstraintBreakClaim],
) -> Result<(), RejectReason> {
    for claim in constraints {
        let Some(canon) = view.constraint(claim.constraint) else {
            return Err(RejectReason::WrongHull);
        };
        if canon.binding != claim.binding || claim.binding == klotho_core::BlobId::ZERO {
            return Err(RejectReason::WrongHull);
        }
        if !canon.is_valid() {
            return Err(RejectReason::WitnessMismatch);
        }
        if view
            .constraint_state(claim.constraint)
            .is_some_and(|s| s.broken)
        {
            return Err(RejectReason::WitnessMismatch);
        }
        let a_member = members.binary_search(&canon.a).is_ok();
        let b_member = members.binary_search(&canon.b).is_ok();
        if !a_member && !b_member {
            return Err(RejectReason::WitnessMismatch);
        }
        if !view.contains(canon.a) || !view.contains(canon.b) {
            return Err(RejectReason::WitnessMismatch);
        }
    }
    for brk in breaks {
        let Ok(idx) = constraints.binary_search_by_key(&brk.constraint, |c| c.constraint) else {
            return Err(RejectReason::WitnessMismatch);
        };
        let Some(canon) = view.constraint(brk.constraint) else {
            return Err(RejectReason::WrongHull);
        };
        if canon.break_impulse <= 0 || brk.impulse < canon.break_impulse {
            return Err(RejectReason::WitnessMismatch);
        }
        if constraints[idx].impulse != brk.impulse {
            return Err(RejectReason::WitnessMismatch);
        }
    }
    Ok(())
}

fn apply_constraints(
    spec: &mut SpecDelta,
    constraints: &[crate::proposal::ConstraintRef],
    breaks: &[crate::proposal::ConstraintBreakClaim],
) -> Result<(), RejectReason> {
    for claim in constraints {
        let broken = breaks.iter().any(|b| b.constraint == claim.constraint);
        spec.set_constraint_state(
            claim.constraint,
            ConstraintState {
                impulse: claim.impulse,
                broken,
            },
        );
    }
    Ok(())
}

fn phys_write_cells(
    bodies: &[BodyDelta],
    constraints: &[crate::proposal::ConstraintRef],
    view: &WorldView<'_>,
) -> Result<PhysWriteSet, RejectReason> {
    let body_ids: Vec<Sigil> = bodies.iter().map(|body| body.mover).collect();
    let mut children: BTreeMap<Sigil, (Sigil, IVec3)> = BTreeMap::new();
    for body in bodies {
        for child in attached_children(view, body.mover) {
            if body_ids.binary_search(&child).is_ok() || children.contains_key(&child) {
                return Err(RejectReason::WitnessMismatch);
            }
            let local = view
                .attach_local(child)
                .unwrap_or_else(|| default_attach_local(view, child, body.mover));
            children.insert(child, (body.mover, local));
        }
    }
    let written = bodies
        .len()
        .saturating_add(children.len())
        .saturating_add(constraints.len());
    if children.len() > MAX_PHYS_ISLAND_CHILDREN || written > MAX_PHYS_ISLAND_WRITE_LOCI {
        return Err(RejectReason::IslandTooLarge);
    }
    let mut cells = Vec::with_capacity(written);
    cells.extend(bodies.iter().map(|body| (body.mover.raw(), 0)));
    cells.extend(children.keys().map(|child| (child.raw(), 0)));
    cells.extend(constraints.iter().map(|c| (c.constraint.raw(), 1)));
    let attachments = children
        .into_iter()
        .map(|(child, (parent, local))| (child, parent, local))
        .collect();
    Ok((cells, attachments))
}

fn apply_body(spec: &mut SpecDelta, island: u16, body: &BodyDelta) -> Result<(), RejectReason> {
    spec.set_pose(body.mover, body.pose)
        .map_err(|_| RejectReason::Budget)?;
    spec.set_vel(body.mover, body.vel, body.yaw_rate)
        .map_err(|_| RejectReason::Budget)?;
    spec.set_rates(body.mover, body.yaw_rate, body.pitch_rate, body.roll_rate)
        .map_err(|_| RejectReason::Budget)?;
    spec.set_island(body.mover, island, body.sleep_ticks)
        .map_err(|_| RejectReason::Budget)?;
    spec.set_support(body.mover, body.support)
        .map_err(|_| RejectReason::Budget)?;
    spec.clear_phys_req(body.mover)
        .map_err(|_| RejectReason::Budget)
}

fn validate_contact_headers(
    view: &WorldView<'_>,
    members: &[Sigil],
    bodies: &[BodyDelta],
    contacts: &[ContactClaim],
) -> Result<(), RejectReason> {
    for claim in contacts {
        if claim.a >= claim.b
            || claim.witness.mover != claim.a
            || !view.contains(claim.a)
            || !view.contains(claim.b)
            || (members.binary_search(&claim.a).is_err()
                && members.binary_search(&claim.b).is_err())
        {
            return Err(RejectReason::WitnessMismatch);
        }
        if view.hull_id(claim.a) != Some(claim.shape_a)
            || view.hull_id(claim.b) != Some(claim.shape_b)
            || view.body_physics(claim.a).shape != claim.kind_a
            || view.body_physics(claim.b).shape != claim.kind_b
        {
            return Err(RejectReason::WrongHull);
        }
        let proposed = bodies
            .binary_search_by_key(&claim.a, |body| body.mover)
            .ok()
            .map(|i| bodies[i].pose)
            .or_else(|| view.pose(claim.a));
        if proposed != Some(claim.witness.proposed) {
            return Err(RejectReason::WitnessMismatch);
        }
    }
    Ok(())
}

fn validate_contact_claims(
    view: &WorldView<'_>,
    _bodies: &[BodyDelta],
    contacts: &[ContactClaim],
) -> Result<(), RejectReason> {
    for claim in contacts {
        if claim.witness.epoch != view.epoch() {
            return Err(RejectReason::StaleEpoch);
        }
        if claim.witness.shape != claim.kind_a
            || !claim.kind_a.is_dynamic()
            || !claim.kind_b.is_dynamic()
            || claim.witness.evidence.is_none()
        {
            return Err(RejectReason::WitnessMismatch);
        }
        let local_a = view.hull(claim.a).ok_or(RejectReason::WrongHull)?;
        let local_b = view.hull(claim.b).ok_or(RejectReason::WrongHull)?;
        let pose_a = view.pose(claim.a).ok_or(RejectReason::WitnessMismatch)?;
        let pose_b = view.pose(claim.b).ok_or(RejectReason::WitnessMismatch)?;
        klotho_geom::verify_cooked(
            claim.witness,
            view.epoch(),
            claim.kind_a,
            local_a,
            pose_a,
            claim.kind_b,
            local_b,
            pose_b,
        )
        .map_err(|_| RejectReason::WitnessMismatch)?;
    }
    Ok(())
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

// Canon bindings come from the live view; the speculative view carries Projection.
// Independently validate traversal and the complete final island geometry.
fn validate_character_geometry(
    before: &WorldView,
    view: &WorldView,
    bodies: &[BodyDelta],
) -> Result<(), RejectReason> {
    for body in bodies {
        if before.character_physics(body.mover).is_none() {
            continue;
        }
        let local = view.hull(body.mover).ok_or(RejectReason::WrongHull)?;
        let shape = klotho_geom::cooked_shape(body.witness.shape, local)
            .map_err(|_| RejectReason::WitnessMismatch)?;
        let previous = before
            .pose(body.mover)
            .ok_or(RejectReason::WitnessMismatch)?;
        let mut obstacles = Vec::new();
        let start_bounds =
            klotho_geom::bounds(shape, previous).map_err(|_| RejectReason::WitnessMismatch)?;
        let end_bounds =
            klotho_geom::bounds(shape, body.pose).map_err(|_| RejectReason::WitnessMismatch)?;
        let mut sweep = start_bounds.swept_union(end_bounds);
        sweep.min.y = sweep.min.y.saturating_sub(503);
        sweep.max.y = sweep.max.y.saturating_add(503);
        let mut static_loci = view.space_candidates(sweep, false);
        static_loci.sort_unstable();
        for s in static_loci {
            if s.kind() != Some(LocusKind::Place)
                && before.body_physics(s).mode != klotho_core::BodyMode::Static
                && !view.opaque_closed(s)
            {
                continue;
            }
            if s == body.mover {
                continue;
            }
            let (Some(local), Some(pose)) = (view.hull(s), view.pose(s)) else {
                continue;
            };
            let obstacle = klotho_geom::cooked_shape(before.body_physics(s).shape, local)
                .map_err(|_| RejectReason::WitnessMismatch)?;
            if !klotho_geom::bounds(obstacle, pose)
                .map_err(|_| RejectReason::WitnessMismatch)?
                .intersects(sweep)
            {
                continue;
            }
            obstacles.push(klotho_geom::CharacterObstacle {
                shape: obstacle,
                pose,
            });
        }
        let policy = before
            .character_physics(body.mover)
            .ok_or(RejectReason::WitnessMismatch)?;
        let desire = body.pose.translation().wrapping_sub(previous.translation());
        let reproduced =
            klotho_geom::resolve_character(shape, previous, desire, policy, &obstacles)
                .map_err(|_| RejectReason::WitnessMismatch)?;
        // Validate horizontal traversal. The coupled scalar solve may resolve
        // vertical contact or fall off a rounded ledge rather than take the
        // query's optional ground snap. Final penetration is checked below.
        if reproduced.pose.x.0.abs_diff(body.pose.x.0) > 2
            || reproduced.pose.z.0.abs_diff(body.pose.z.0) > 2
        {
            return Err(RejectReason::WitnessMismatch);
        }
        let query =
            klotho_geom::bounds(shape, body.pose).map_err(|_| RejectReason::WitnessMismatch)?;
        let mut candidates = view.space_candidates(query, false);
        candidates.sort_unstable();
        for other in candidates {
            if other == body.mover || view.attach_parent(other) == Some(body.mover) {
                continue;
            }
            let (Some(local), Some(pose)) = (view.hull(other), view.pose(other)) else {
                continue;
            };
            let obstacle = klotho_geom::cooked_shape(before.body_physics(other).shape, local)
                .map_err(|_| RejectReason::WitnessMismatch)?;
            if klotho_geom::penetration_mm(shape, body.pose, obstacle, pose)
                .map_err(|_| RejectReason::WitnessMismatch)?
                .unwrap_or(0)
                > klotho_geom::CONTACT_SLOP_MM
            {
                return Err(RejectReason::WitnessMismatch);
            }
        }
    }
    Ok(())
}
