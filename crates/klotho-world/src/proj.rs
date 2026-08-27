//! Projection columns. Rebuildable from snapshot + Trace suffix.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use klotho_canon::RiteId;
use klotho_core::{
    AabbMm, AffordanceId, BlobId, LocusKind, PackedIx, PhysRequest, PoseMm, ResourceId, Sigil, Vel3,
};
use klotho_ir::{Channel, Rel};
use klotho_trace::{RelTag, TraceBody, TraceEvent};

use crate::cow::CowCol;
use crate::error::WorldError;
use crate::grid::{PlaceIndex, world_aabb};

/// Derived SoA. Not a source.
#[derive(Clone, Debug)]
pub struct Projection {
    locus_cap: usize,
    by_sigil: Arc<BTreeMap<Sigil, PackedIx>>,
    sigils: CowCol<Sigil>,
    kinds: CowCol<LocusKind>,
    /// Affordance bits 0..63 per packed index.
    afford: CowCol<u64>,
    hull_local: CowCol<Option<AabbMm>>,
    hull_id: CowCol<BlobId>,
    pose: CowCol<Option<PoseMm>>,
    vel: CowCol<Vel3>,
    yaw_rate: CowCol<i32>,
    island_id: CowCol<u16>,
    sleep_ticks: CowCol<u16>,
    /// `(packed, rel_u8)` → neighbors.
    rels: Arc<BTreeMap<(PackedIx, u8), Vec<Sigil>>>,
    rel_triples: Arc<BTreeSet<(PackedIx, u8, Sigil)>>,
    qty: Arc<BTreeMap<(PackedIx, ResourceId), i32>>,
    phys_req: Arc<BTreeMap<PackedIx, PhysRequest>>,
    rites: Arc<BTreeMap<(PackedIx, u16), RiteMachine>>,
    knows: Arc<BTreeSet<(PackedIx, u16)>>,
    space_ix: Arc<PlaceIndex>,
    opaque_id: Option<AffordanceId>,
}

/// In-progress rite row.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct RiteMachine {
    /// Program counter.
    pub pc: u16,
    /// Remaining WAIT ticks.
    pub wait_left: u16,
    /// Bound target.
    pub target: Option<Sigil>,
    /// Channel that may resume this WAIT (`None` = any).
    pub wait_ch: Option<Channel>,
}

impl Projection {
    /// Empty projection. `opaque` is the cooked `Opaque` id, if declared.
    #[must_use]
    pub fn new(opaque: Option<AffordanceId>) -> Self {
        Self::with_cap(opaque, crate::MAX_LOCI)
    }

    /// Empty projection with a packed-row cap (clamped to [`klotho_core::MAX_LOCI_PROCESS`]).
    #[must_use]
    pub fn with_cap(opaque: Option<AffordanceId>, cap: usize) -> Self {
        Self {
            locus_cap: cap.min(klotho_core::MAX_LOCI_PROCESS),
            by_sigil: Arc::new(BTreeMap::new()),
            sigils: CowCol::default(),
            kinds: CowCol::default(),
            afford: CowCol::default(),
            hull_local: CowCol::default(),
            hull_id: CowCol::default(),
            pose: CowCol::default(),
            vel: CowCol::default(),
            yaw_rate: CowCol::default(),
            island_id: CowCol::default(),
            sleep_ticks: CowCol::default(),
            rels: Arc::new(BTreeMap::new()),
            rel_triples: Arc::new(BTreeSet::new()),
            qty: Arc::new(BTreeMap::new()),
            phys_req: Arc::new(BTreeMap::new()),
            rites: Arc::new(BTreeMap::new()),
            knows: Arc::new(BTreeSet::new()),
            space_ix: Arc::new(PlaceIndex::new()),
            opaque_id: opaque,
        }
    }

    /// Packed-row cap for this projection.
    #[must_use]
    pub fn locus_cap(&self) -> usize {
        self.locus_cap
    }

    /// Locus count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sigils.len()
    }

    /// No loci.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sigils.is_empty()
    }

    /// Packed index for `s`.
    #[must_use]
    pub fn packed(&self, s: Sigil) -> Option<PackedIx> {
        self.by_sigil.get(&s).copied()
    }

    /// Sigil for a packed index.
    #[must_use]
    pub fn sigil(&self, ix: PackedIx) -> Option<Sigil> {
        self.sigils.get(ix as usize).copied()
    }

    pub(crate) fn insert_locus(
        &mut self,
        s: Sigil,
        kind: LocusKind,
    ) -> Result<PackedIx, WorldError> {
        if let Some(&i) = self.by_sigil.get(&s) {
            return Ok(i);
        }
        if self.sigils.len() >= self.locus_cap {
            return Err(WorldError::LocusCap);
        }
        let i = self.sigils.len() as PackedIx;
        Arc::make_mut(&mut self.by_sigil).insert(s, i);
        self.sigils.push(s);
        self.kinds.push(kind);
        self.afford.push(0);
        self.hull_local.push(None);
        self.hull_id.push(BlobId::ZERO);
        self.pose.push(None);
        self.vel.push(Vel3::ZERO);
        self.yaw_rate.push(0);
        self.island_id.push(0);
        self.sleep_ticks.push(0);
        Arc::make_mut(&mut self.space_ix).ensure(i);
        Ok(i)
    }

    pub(crate) fn set_affordance(
        &mut self,
        s: Sigil,
        a: AffordanceId,
        on: bool,
    ) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        if a.0 < 64 {
            let bit = 1u64 << a.0;
            let row = self
                .afford
                .get_mut(i as usize)
                .expect("packed index in range");
            if on {
                *row |= bit;
            } else {
                *row &= !bit;
            }
        }
        self.reindex_ix(i);
        Ok(())
    }

    pub(crate) fn set_hull(
        &mut self,
        s: Sigil,
        local: AabbMm,
        id: BlobId,
    ) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        self.hull_local.set(i as usize, Some(local));
        self.hull_id.set(i as usize, id);
        self.reindex_ix(i);
        Ok(())
    }

    pub(crate) fn set_pose(&mut self, s: Sigil, p: PoseMm) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        self.pose.set(i as usize, Some(p));
        self.reindex_ix(i);
        Ok(())
    }

    pub(crate) fn set_vel(&mut self, s: Sigil, vel: Vel3, yaw_rate: i32) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        self.vel.set(i as usize, vel);
        self.yaw_rate.set(i as usize, yaw_rate);
        Ok(())
    }

    pub(crate) fn set_island(
        &mut self,
        s: Sigil,
        island: u16,
        sleep: u16,
    ) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        self.island_id.set(i as usize, island);
        self.sleep_ticks.set(i as usize, sleep);
        Ok(())
    }

    pub(crate) fn set_qty(&mut self, s: Sigil, r: ResourceId, v: i32) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        Arc::make_mut(&mut self.qty).insert((i, r), v);
        Ok(())
    }

    pub(crate) fn set_phys_req(&mut self, s: Sigil, req: PhysRequest) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        Arc::make_mut(&mut self.phys_req).insert(i, req);
        Ok(())
    }

    /// Current `PHYS_REQ` write, if any.
    #[must_use]
    pub fn phys_req(&self, s: Sigil) -> Option<PhysRequest> {
        let i = self.packed(s)?;
        self.phys_req.get(&i).copied()
    }

    pub(crate) fn add_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        let ia = self.packed(a).ok_or(WorldError::UnknownLocus)?;
        let key = rel_key(r);
        if Arc::make_mut(&mut self.rel_triples).insert((ia, key, b)) {
            Arc::make_mut(&mut self.rels)
                .entry((ia, key))
                .or_default()
                .push(b);
        }
        if r == Rel::LockedBy || r == Rel::In {
            self.reindex_ix(ia);
        }
        Ok(())
    }

    pub(crate) fn del_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        let ia = self.packed(a).ok_or(WorldError::UnknownLocus)?;
        let key = rel_key(r);
        Arc::make_mut(&mut self.rel_triples).remove(&(ia, key, b));
        if let Some(v) = Arc::make_mut(&mut self.rels).get_mut(&(ia, key)) {
            v.retain(|&x| x != b);
        }
        if r == Rel::LockedBy || r == Rel::In {
            self.reindex_ix(ia);
        }
        Ok(())
    }

    /// Apply an admitted event to the columns. Unknown loci are ignored
    /// (the event still belongs on the log).
    pub(crate) fn apply_event(&mut self, e: &TraceEvent) {
        match &e.body {
            TraceBody::RiteBegan {
                actor,
                rite,
                target,
            } => {
                if let Some(i) = self.packed(*actor) {
                    Arc::make_mut(&mut self.rites).insert(
                        (i, *rite),
                        RiteMachine {
                            pc: 0,
                            wait_left: 0,
                            target: *target,
                            wait_ch: None,
                        },
                    );
                }
            }
            TraceBody::RiteAdvanced {
                actor,
                rite,
                pc,
                wait_left,
            } => {
                if let Some(i) = self.packed(*actor) {
                    if let Some(m) = Arc::make_mut(&mut self.rites).get_mut(&(i, *rite)) {
                        m.pc = *pc;
                        m.wait_left = *wait_left;
                    }
                }
            }
            TraceBody::RiteEnded { actor, rite, .. } => {
                if let Some(i) = self.packed(*actor) {
                    Arc::make_mut(&mut self.rites).remove(&(i, *rite));
                }
            }
            TraceBody::QtyChanged { id, res, to, .. } => {
                let _ = self.set_qty(*id, *res, *to);
            }
            TraceBody::IslandSnap(snap) => {
                for (k, &member) in snap.members.iter().enumerate() {
                    let Ok(i) = self.ensure(member) else {
                        continue;
                    };
                    if let Some(p) = snap.poses.get(k) {
                        self.pose.set(i as usize, Some(*p));
                    }
                    if let Some(&v) = snap.vels.get(k) {
                        self.vel.set(i as usize, v);
                    }
                    if let Some(&y) = snap.yaw_rates.get(k) {
                        self.yaw_rate.set(i as usize, y);
                    }
                    if let Some(&t) = snap.sleep_ticks.get(k) {
                        self.sleep_ticks.set(i as usize, t);
                    }
                    self.island_id.set(i as usize, snap.island);
                    self.reindex_ix(i);
                }
            }
            TraceBody::PoseCommitted { s, pose, .. } => {
                if let Some(i) = self.packed(*s) {
                    self.pose.set(i as usize, Some(*pose));
                    self.reindex_ix(i);
                }
            }
            TraceBody::SaveRequested
            | TraceBody::Emitted { .. }
            | TraceBody::Uttered { .. }
            | TraceBody::PlaceLoaded { .. }
            | TraceBody::PlaceEvicted { .. }
            | TraceBody::Spawned { .. }
            | TraceBody::Despawned { .. } => {}
            TraceBody::Learned { mind, fact } => {
                if let Some(i) = self.packed(*mind) {
                    Arc::make_mut(&mut self.knows).insert((i, *fact));
                }
            }
            TraceBody::RelAdd { a, rel, b } => {
                if let Some(r) = rel_from_tag(*rel) {
                    let _ = self.add_rel(*a, r, *b);
                }
            }
            TraceBody::RelDel { a, rel, b } => {
                if let Some(r) = rel_from_tag(*rel) {
                    let _ = self.del_rel(*a, r, *b);
                }
            }
        }
    }

    fn ensure(&mut self, s: Sigil) -> Result<PackedIx, WorldError> {
        if let Some(i) = self.packed(s) {
            Ok(i)
        } else {
            self.insert_locus(s, LocusKind::Relic)
        }
    }

    /// Drop `space_ix` and rebuild from hull, pose, OpaqueClosed, and Place
    /// membership. Legal any time.
    pub fn rebuild_space_ix(&mut self) {
        let items: Vec<(PackedIx, AabbMm, bool, Option<Sigil>)> = (0..self.sigils.len()
            as PackedIx)
            .filter_map(|i| {
                self.world_hull(i)
                    .map(|aabb| (i, aabb, self.opaque_closed_ix(i), self.place_for(i)))
            })
            .collect();
        Arc::make_mut(&mut self.space_ix).rebuild(items);
    }

    fn reindex_ix(&mut self, ix: PackedIx) {
        let place = self.place_for(ix);
        let oc = self.opaque_closed_ix(ix);
        match self.world_hull(ix) {
            Some(aabb) => Arc::make_mut(&mut self.space_ix).index(ix, aabb, oc, place),
            None => Arc::make_mut(&mut self.space_ix).unindex(ix),
        }
    }

    fn place_for(&self, ix: PackedIx) -> Option<Sigil> {
        let s = self.sigils.get(ix as usize).copied()?;
        if self.kinds.get(ix as usize).copied() == Some(LocusKind::Place) {
            return Some(s);
        }
        self.rels.get(&(ix, RelTag::IN.0)).and_then(|v| {
            v.iter()
                .copied()
                .find(|n| n.kind() == Some(LocusKind::Place))
        })
    }

    fn world_hull(&self, ix: PackedIx) -> Option<AabbMm> {
        let local = (*self.hull_local.get(ix as usize)?)?;
        let pose = (*self.pose.get(ix as usize)?)?;
        Some(world_aabb(local, pose.translation()))
    }

    fn opaque_closed_ix(&self, ix: PackedIx) -> bool {
        let opaque = match self.opaque_id {
            Some(id) if id.0 < 64 => {
                (self.afford.get(ix as usize).copied().unwrap_or(0) & (1u64 << id.0)) != 0
            }
            _ => false,
        };
        if !opaque {
            return false;
        }
        self.rels
            .get(&(ix, RelTag::LOCKED_BY.0))
            .is_some_and(|v| !v.is_empty())
    }

    /// Affordance bit.
    #[must_use]
    pub fn has_affordance(&self, s: Sigil, a: AffordanceId) -> bool {
        let Some(i) = self.packed(s) else {
            return false;
        };
        if a.0 >= 64 {
            return false;
        }
        (self.afford.get(i as usize).copied().unwrap_or(0) & (1u64 << a.0)) != 0
    }

    /// Relation triple.
    #[must_use]
    pub fn has_rel(&self, a: Sigil, r: Rel, b: Sigil) -> bool {
        let Some(i) = self.packed(a) else {
            return false;
        };
        self.rel_triples.contains(&(i, rel_key(r), b))
    }

    /// Neighbors along `r`, insert order.
    pub fn related(&self, a: Sigil, r: Rel, out: &mut Vec<Sigil>) {
        out.clear();
        out.extend_from_slice(self.related_slice(a, r));
    }

    /// Neighbors along `r`.
    #[must_use]
    pub fn related_slice(&self, a: Sigil, r: Rel) -> &[Sigil] {
        let Some(i) = self.packed(a) else {
            return &[];
        };
        self.rels
            .get(&(i, rel_key(r)))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Quantity; missing is 0.
    #[must_use]
    pub fn qty(&self, s: Sigil, r: ResourceId) -> i32 {
        let Some(i) = self.packed(s) else {
            return 0;
        };
        self.qty.get(&(i, r)).copied().unwrap_or(0)
    }

    /// Pose.
    #[must_use]
    pub fn pose(&self, s: Sigil) -> Option<PoseMm> {
        let i = self.packed(s)?;
        self.pose.get(i as usize).copied().flatten()
    }

    /// `(vel, yaw_rate)`.
    #[must_use]
    pub fn vel(&self, s: Sigil) -> Option<(Vel3, i32)> {
        let i = self.packed(s)?;
        Some((
            self.vel.get(i as usize).copied().unwrap_or(Vel3::ZERO),
            self.yaw_rate.get(i as usize).copied().unwrap_or(0),
        ))
    }

    /// `(island_id, sleep_ticks)`.
    #[must_use]
    pub fn island(&self, s: Sigil) -> Option<(u16, u16)> {
        let i = self.packed(s)?;
        Some((
            self.island_id.get(i as usize).copied().unwrap_or(0),
            self.sleep_ticks.get(i as usize).copied().unwrap_or(0),
        ))
    }

    /// Local hull AABB (unposed).
    #[must_use]
    pub fn hull(&self, s: Sigil) -> Option<AabbMm> {
        let i = self.packed(s)?;
        self.hull_local.get(i as usize).copied().flatten()
    }

    /// Hull blob id. [`BlobId::ZERO`] if unbound.
    #[must_use]
    pub fn hull_id(&self, s: Sigil) -> Option<BlobId> {
        let i = self.packed(s)?;
        self.hull_id.get(i as usize).copied()
    }

    /// Knows table.
    #[must_use]
    pub fn knows(&self, mind: Sigil, fact: u16) -> bool {
        let Some(i) = self.packed(mind) else {
            return false;
        };
        self.knows.contains(&(i, fact))
    }

    /// Active rite row.
    #[must_use]
    pub fn rite(&self, actor: Sigil, rite: RiteId) -> Option<RiteMachine> {
        let i = self.packed(actor)?;
        self.rites.get(&(i, rite.0)).copied()
    }

    /// First active rite for `actor`, if any (lowest rite id).
    #[must_use]
    pub fn first_rite(&self, actor: Sigil) -> Option<(RiteId, RiteMachine)> {
        let i = self.packed(actor)?;
        self.rites
            .iter()
            .filter(|((ix, _), _)| *ix == i)
            .min_by_key(|((_, rite), _)| *rite)
            .map(|((_, rite), m)| (RiteId(*rite), *m))
    }

    pub(crate) fn put_rite(&mut self, actor: Sigil, rite: RiteId, m: RiteMachine) {
        if let Some(i) = self.packed(actor) {
            Arc::make_mut(&mut self.rites).insert((i, rite.0), m);
        }
    }

    /// `Opaque ∧ LockedBy`.
    #[must_use]
    pub fn opaque_closed(&self, s: Sigil) -> bool {
        let Some(i) = self.packed(s) else {
            return false;
        };
        self.opaque_closed_ix(i)
    }

    /// Posed hull AABB.
    #[must_use]
    pub fn posed_hull(&self, s: Sigil) -> Option<AabbMm> {
        let i = self.packed(s)?;
        self.world_hull(i)
    }

    /// Kernel spatial index (per-Place grids + unplaced).
    #[must_use]
    pub fn space_ix(&self) -> &PlaceIndex {
        &self.space_ix
    }

    /// Loci that have `a`.
    pub fn with_affordance(&self, a: AffordanceId) -> impl Iterator<Item = Sigil> + '_ {
        (0..self.sigils.len() as PackedIx).filter_map(move |i| {
            let s = self.sigils.get(i as usize).copied()?;
            self.has_affordance(s, a).then_some(s)
        })
    }

    #[cfg(test)]
    pub(crate) fn shares_pose_chunk(&self, other: &Self, ix: usize) -> bool {
        self.pose.shares_chunk(&other.pose, ix)
    }

    pub(crate) fn approx_bytes(&self) -> usize {
        let n = self.sigils.len();
        let mut bytes = n * 64;
        bytes += self.pose.approx_bytes();
        bytes += self.vel.approx_bytes();
        bytes += self.yaw_rate.approx_bytes();
        bytes += self.rels.len() * 16;
        for v in self.rels.values() {
            bytes += v.len() * 16;
        }
        bytes += self.qty.len() * 8;
        bytes += self.phys_req.len() * 24;
        bytes += self.rites.len() * 16;
        bytes += self.knows.len() * 4;
        bytes += self.space_ix.approx_bytes();
        bytes
    }
}

impl Default for Projection {
    fn default() -> Self {
        Self::new(None)
    }
}

pub(crate) fn rel_key(r: Rel) -> u8 {
    r.as_u8()
}

fn rel_from_tag(t: RelTag) -> Option<Rel> {
    Rel::from_u8(t.0)
}
