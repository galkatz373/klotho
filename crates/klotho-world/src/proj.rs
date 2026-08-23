//! Projection columns. Rebuildable from snapshot + Trace suffix.

use std::collections::{BTreeMap, BTreeSet};

use klotho_canon::RiteId;
use klotho_core::{AabbMm, AffordanceId, BlobId, LocusKind, PoseMm, ResourceId, Sigil, VelFx};
use klotho_ir::{Channel, Rel};
use klotho_trace::{RelTag, TraceBody, TraceEvent};

use crate::MAX_LOCI;
use crate::error::WorldError;
use crate::grid::{GridIndex, world_aabb};

/// Derived SoA. Not a source.
#[derive(Clone, Debug, Default)]
pub struct Projection {
    by_sigil: BTreeMap<Sigil, u16>,
    sigils: Vec<Sigil>,
    kinds: Vec<LocusKind>,
    /// Affordance bits 0..63 per slot.
    afford: Vec<u64>,
    hull_local: Vec<Option<AabbMm>>,
    hull_id: Vec<BlobId>,
    pose: Vec<Option<PoseMm>>,
    vel_x: Vec<VelFx>,
    vel_z: Vec<VelFx>,
    yaw_rate: Vec<i32>,
    island_id: Vec<u16>,
    sleep_ticks: Vec<u16>,
    /// `(slot, rel_u8)` → neighbors.
    rels: BTreeMap<(u16, u8), Vec<Sigil>>,
    rel_triples: BTreeSet<(u16, u8, Sigil)>,
    qty: BTreeMap<(u16, ResourceId), i32>,
    rites: BTreeMap<(u16, u16), RiteMachine>,
    knows: BTreeSet<(u16, u16)>,
    space_ix: GridIndex,
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
        Self {
            opaque_id: opaque,
            ..Self::default()
        }
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

    /// Packed slot for `s`.
    #[must_use]
    pub fn slot(&self, s: Sigil) -> Option<u16> {
        self.by_sigil.get(&s).copied()
    }

    /// Sigil for a packed slot.
    #[must_use]
    pub fn sigil(&self, slot: u16) -> Option<Sigil> {
        self.sigils.get(slot as usize).copied()
    }

    pub(crate) fn insert_locus(&mut self, s: Sigil, kind: LocusKind) -> Result<u16, WorldError> {
        if let Some(&i) = self.by_sigil.get(&s) {
            return Ok(i);
        }
        if self.sigils.len() >= MAX_LOCI {
            return Err(WorldError::LocusCap);
        }
        let i = self.sigils.len() as u16;
        self.by_sigil.insert(s, i);
        self.sigils.push(s);
        self.kinds.push(kind);
        self.afford.push(0);
        self.hull_local.push(None);
        self.hull_id.push(BlobId::ZERO);
        self.pose.push(None);
        self.vel_x.push(VelFx::ZERO);
        self.vel_z.push(VelFx::ZERO);
        self.yaw_rate.push(0);
        self.island_id.push(0);
        self.sleep_ticks.push(0);
        Ok(i)
    }

    pub(crate) fn set_affordance(
        &mut self,
        s: Sigil,
        a: AffordanceId,
        on: bool,
    ) -> Result<(), WorldError> {
        let i = self.slot(s).ok_or(WorldError::UnknownLocus)?;
        if a.0 < 64 {
            let bit = 1u64 << a.0;
            if on {
                self.afford[i as usize] |= bit;
            } else {
                self.afford[i as usize] &= !bit;
            }
        }
        self.reindex_slot(i);
        Ok(())
    }

    pub(crate) fn set_hull(
        &mut self,
        s: Sigil,
        local: AabbMm,
        id: BlobId,
    ) -> Result<(), WorldError> {
        let i = self.slot(s).ok_or(WorldError::UnknownLocus)?;
        self.hull_local[i as usize] = Some(local);
        self.hull_id[i as usize] = id;
        self.reindex_slot(i);
        Ok(())
    }

    pub(crate) fn set_pose(&mut self, s: Sigil, p: PoseMm) -> Result<(), WorldError> {
        let i = self.slot(s).ok_or(WorldError::UnknownLocus)?;
        self.pose[i as usize] = Some(p);
        self.reindex_slot(i);
        Ok(())
    }

    pub(crate) fn set_vel(
        &mut self,
        s: Sigil,
        vx: VelFx,
        vz: VelFx,
        yaw_rate: i32,
    ) -> Result<(), WorldError> {
        let i = self.slot(s).ok_or(WorldError::UnknownLocus)?;
        self.vel_x[i as usize] = vx;
        self.vel_z[i as usize] = vz;
        self.yaw_rate[i as usize] = yaw_rate;
        Ok(())
    }

    pub(crate) fn set_island(
        &mut self,
        s: Sigil,
        island: u16,
        sleep: u16,
    ) -> Result<(), WorldError> {
        let i = self.slot(s).ok_or(WorldError::UnknownLocus)?;
        self.island_id[i as usize] = island;
        self.sleep_ticks[i as usize] = sleep;
        Ok(())
    }

    pub(crate) fn set_qty(&mut self, s: Sigil, r: ResourceId, v: i32) -> Result<(), WorldError> {
        let i = self.slot(s).ok_or(WorldError::UnknownLocus)?;
        self.qty.insert((i, r), v);
        Ok(())
    }

    pub(crate) fn add_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        let ia = self.slot(a).ok_or(WorldError::UnknownLocus)?;
        let key = rel_key(r);
        if self.rel_triples.insert((ia, key, b)) {
            self.rels.entry((ia, key)).or_default().push(b);
        }
        if r == Rel::LockedBy {
            self.reindex_slot(ia);
        }
        Ok(())
    }

    pub(crate) fn del_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        let ia = self.slot(a).ok_or(WorldError::UnknownLocus)?;
        let key = rel_key(r);
        self.rel_triples.remove(&(ia, key, b));
        if let Some(v) = self.rels.get_mut(&(ia, key)) {
            v.retain(|&x| x != b);
        }
        if r == Rel::LockedBy {
            self.reindex_slot(ia);
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
                if let Some(i) = self.slot(*actor) {
                    self.rites.insert(
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
                if let Some(i) = self.slot(*actor) {
                    if let Some(m) = self.rites.get_mut(&(i, *rite)) {
                        m.pc = *pc;
                        m.wait_left = *wait_left;
                    }
                }
            }
            TraceBody::RiteEnded { actor, rite, .. } => {
                if let Some(i) = self.slot(*actor) {
                    self.rites.remove(&(i, *rite));
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
                        self.pose[i as usize] = Some(*p);
                    }
                    if let Some(&(vx, vz)) = snap.vels.get(k) {
                        self.vel_x[i as usize] = vx;
                        self.vel_z[i as usize] = vz;
                    }
                    if let Some(&y) = snap.yaw_rates.get(k) {
                        self.yaw_rate[i as usize] = y;
                    }
                    if let Some(&t) = snap.sleep_ticks.get(k) {
                        self.sleep_ticks[i as usize] = t;
                    }
                    self.island_id[i as usize] = snap.island;
                    self.reindex_slot(i);
                }
            }
            TraceBody::PoseCommitted { s, xz, yaw, .. } => {
                if let Some(i) = self.slot(*s) {
                    let y = self.pose[i as usize]
                        .map(|p| p.y)
                        .unwrap_or(klotho_core::Mm::ZERO);
                    self.pose[i as usize] = Some(PoseMm {
                        x: xz.0,
                        z: xz.1,
                        y,
                        yaw: *yaw,
                    });
                    self.reindex_slot(i);
                }
            }
            TraceBody::SaveRequested | TraceBody::Emitted { .. } | TraceBody::Uttered { .. } => {}
            TraceBody::Learned { mind, fact } => {
                if let Some(i) = self.slot(*mind) {
                    self.knows.insert((i, *fact));
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

    fn ensure(&mut self, s: Sigil) -> Result<u16, WorldError> {
        if let Some(i) = self.slot(s) {
            Ok(i)
        } else {
            self.insert_locus(s, LocusKind::Relic)
        }
    }

    /// Drop `space_ix` and rebuild from hull+pose+LockedBy. Legal any time.
    pub fn rebuild_space_ix(&mut self) {
        let items: Vec<(u16, AabbMm, bool)> = (0..self.sigils.len() as u16)
            .filter_map(|i| {
                self.world_hull(i)
                    .map(|aabb| (i, aabb, self.opaque_closed_slot(i)))
            })
            .collect();
        self.space_ix.rebuild(items);
    }

    fn reindex_slot(&mut self, slot: u16) {
        match self.world_hull(slot) {
            Some(aabb) => self
                .space_ix
                .index(slot, aabb, self.opaque_closed_slot(slot)),
            None => self.space_ix.unindex(slot),
        }
    }

    fn world_hull(&self, slot: u16) -> Option<AabbMm> {
        let local = self.hull_local[slot as usize]?;
        let pose = self.pose[slot as usize]?;
        Some(world_aabb(local, pose.translation()))
    }

    fn opaque_closed_slot(&self, slot: u16) -> bool {
        let opaque = match self.opaque_id {
            Some(id) if id.0 < 64 => (self.afford[slot as usize] & (1u64 << id.0)) != 0,
            _ => false,
        };
        if !opaque {
            return false;
        }
        self.rels
            .get(&(slot, RelTag::LOCKED_BY.0))
            .is_some_and(|v| !v.is_empty())
    }

    /// Affordance bit.
    #[must_use]
    pub fn has_affordance(&self, s: Sigil, a: AffordanceId) -> bool {
        let Some(i) = self.slot(s) else {
            return false;
        };
        if a.0 >= 64 {
            return false;
        }
        (self.afford[i as usize] & (1u64 << a.0)) != 0
    }

    /// Relation triple.
    #[must_use]
    pub fn has_rel(&self, a: Sigil, r: Rel, b: Sigil) -> bool {
        let Some(i) = self.slot(a) else {
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
        let Some(i) = self.slot(a) else {
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
        let Some(i) = self.slot(s) else {
            return 0;
        };
        self.qty.get(&(i, r)).copied().unwrap_or(0)
    }

    /// Pose.
    #[must_use]
    pub fn pose(&self, s: Sigil) -> Option<PoseMm> {
        let i = self.slot(s)?;
        self.pose[i as usize]
    }

    /// `(vel_x, vel_z, yaw_rate)`.
    #[must_use]
    pub fn vel(&self, s: Sigil) -> Option<(VelFx, VelFx, i32)> {
        let i = self.slot(s)?;
        Some((
            self.vel_x[i as usize],
            self.vel_z[i as usize],
            self.yaw_rate[i as usize],
        ))
    }

    /// `(island_id, sleep_ticks)`.
    #[must_use]
    pub fn island(&self, s: Sigil) -> Option<(u16, u16)> {
        let i = self.slot(s)?;
        Some((self.island_id[i as usize], self.sleep_ticks[i as usize]))
    }

    /// Local hull AABB (unposed).
    #[must_use]
    pub fn hull(&self, s: Sigil) -> Option<AabbMm> {
        let i = self.slot(s)?;
        self.hull_local[i as usize]
    }

    /// Hull blob id. [`BlobId::ZERO`] if unbound.
    #[must_use]
    pub fn hull_id(&self, s: Sigil) -> Option<BlobId> {
        let i = self.slot(s)?;
        Some(self.hull_id[i as usize])
    }

    /// Knows table.
    #[must_use]
    pub fn knows(&self, mind: Sigil, fact: u16) -> bool {
        let Some(i) = self.slot(mind) else {
            return false;
        };
        self.knows.contains(&(i, fact))
    }

    /// Active rite row.
    #[must_use]
    pub fn rite(&self, actor: Sigil, rite: RiteId) -> Option<RiteMachine> {
        let i = self.slot(actor)?;
        self.rites.get(&(i, rite.0)).copied()
    }

    /// First active rite for `actor`, if any (lowest rite id).
    #[must_use]
    pub fn first_rite(&self, actor: Sigil) -> Option<(RiteId, RiteMachine)> {
        let i = self.slot(actor)?;
        self.rites
            .iter()
            .filter(|((slot, _), _)| *slot == i)
            .min_by_key(|((_, rite), _)| *rite)
            .map(|((_, rite), m)| (RiteId(*rite), *m))
    }

    pub(crate) fn put_rite(&mut self, actor: Sigil, rite: RiteId, m: RiteMachine) {
        if let Some(i) = self.slot(actor) {
            self.rites.insert((i, rite.0), m);
        }
    }

    /// `Opaque ∧ LockedBy`.
    #[must_use]
    pub fn opaque_closed(&self, s: Sigil) -> bool {
        let Some(i) = self.slot(s) else {
            return false;
        };
        self.opaque_closed_slot(i)
    }

    /// Posed hull AABB.
    #[must_use]
    pub fn posed_hull(&self, s: Sigil) -> Option<AabbMm> {
        let i = self.slot(s)?;
        self.world_hull(i)
    }

    /// Kernel spatial index.
    #[must_use]
    pub fn space_ix(&self) -> &GridIndex {
        &self.space_ix
    }

    /// Loci that have `a`.
    pub fn with_affordance(&self, a: AffordanceId) -> impl Iterator<Item = Sigil> + '_ {
        self.sigils
            .iter()
            .copied()
            .filter(move |s| self.has_affordance(*s, a))
    }

    pub(crate) fn approx_bytes(&self) -> usize {
        let n = self.sigils.len();
        let mut bytes = n * 64;
        bytes += self.rels.len() * 16;
        for v in self.rels.values() {
            bytes += v.len() * 16;
        }
        bytes += self.qty.len() * 8;
        bytes += self.rites.len() * 16;
        bytes += self.knows.len() * 4;
        bytes += self.space_ix.approx_bytes();
        bytes
    }
}

pub(crate) fn rel_key(r: Rel) -> u8 {
    match r {
        Rel::In => RelTag::IN.0,
        Rel::OwnedBy => RelTag::OWNED_BY.0,
        Rel::WieldedBy => RelTag::WIELDED_BY.0,
        Rel::KeyedBy => RelTag::KEYED_BY.0,
        Rel::Knows => RelTag::KNOWS.0,
        Rel::Owes => RelTag::OWES.0,
        Rel::Fears => RelTag::FEARS.0,
        Rel::PartOf => RelTag::PART_OF.0,
        Rel::DerivedFrom => RelTag::DERIVED_FROM.0,
        Rel::LockedBy => RelTag::LOCKED_BY.0,
        Rel::Dead => RelTag::DEAD.0,
    }
}

fn rel_from_tag(t: RelTag) -> Option<Rel> {
    Some(match t.0 {
        0 => Rel::In,
        1 => Rel::OwnedBy,
        2 => Rel::WieldedBy,
        3 => Rel::KeyedBy,
        4 => Rel::Knows,
        5 => Rel::Owes,
        6 => Rel::Fears,
        7 => Rel::PartOf,
        8 => Rel::DerivedFrom,
        9 => Rel::LockedBy,
        10 => Rel::Dead,
        _ => return None,
    })
}
