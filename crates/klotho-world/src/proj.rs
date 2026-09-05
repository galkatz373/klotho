//! Projection columns. Rebuildable from snapshot + Trace suffix.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use klotho_canon::RiteId;
use klotho_core::{
    AabbMm, AffordanceId, BlobId, Hash, IVec3, LocusKind, PackedIx, PhysRequest, PoseMm,
    ResourceId, Sigil, SimLod, Support, Vel3, rotate_xz,
};
use klotho_ir::{Channel, Rel};
use klotho_trace::{RelTag, TraceBody, TraceEvent};

use crate::cow::CowCol;
use crate::error::{SnapError, WorldError};
use crate::grid::{PlaceIndex, world_aabb};
use crate::snap::{MAX_PLACE_ROWS, PlaceRow, PlaceSnap};
use crate::snap_blob::{MAX_SNAP_ROWS, SnapRow, check_row_caps};

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
    pitch_rate: CowCol<i32>,
    roll_rate: CowCol<i32>,
    island_id: CowCol<u16>,
    sleep_ticks: CowCol<u16>,
    sim_lod: CowCol<SimLod>,
    /// Place membership (`Rel::In`). Column so a 10k-row load is not a BTree insert per row.
    in_place: CowCol<Option<Sigil>>,
    support: CowCol<Option<Support>>,
    attach_local: CowCol<Option<IVec3>>,
    /// `(packed, rel_u8)` → neighbors. One neighbor is inline (no heap).
    rels: Arc<BTreeMap<(PackedIx, u8), Neighbors>>,
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

/// Rows to drop and rites to end before packed-index remap.
pub(crate) struct PlaceEvictPlan {
    pub drop: Vec<Sigil>,
    pub rites: Vec<(Sigil, u16)>,
}

/// Zero or more neighbors. A single edge does not heap-allocate.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Neighbors {
    one: Option<Sigil>,
    more: Vec<Sigil>,
}

impl Neighbors {
    fn as_slice(&self) -> &[Sigil] {
        if self.more.is_empty() {
            self.one.as_slice()
        } else {
            &self.more
        }
    }

    fn len(&self) -> usize {
        if self.more.is_empty() {
            usize::from(self.one.is_some())
        } else {
            self.more.len()
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn push(&mut self, s: Sigil) {
        if !self.more.is_empty() {
            self.more.push(s);
            return;
        }
        match self.one {
            None => self.one = Some(s),
            Some(first) => {
                self.more.reserve_exact(2);
                self.more.push(first);
                self.more.push(s);
                self.one = None;
            }
        }
    }

    fn retain<F: FnMut(&Sigil) -> bool>(&mut self, mut f: F) {
        if !self.more.is_empty() {
            self.more.retain(f);
            if self.more.len() == 1 {
                self.one = self.more.pop();
                self.more = Vec::new();
            }
            return;
        }
        if let Some(s) = self.one {
            if !f(&s) {
                self.one = None;
            }
        }
    }

    fn iter(&self) -> impl Iterator<Item = Sigil> + '_ {
        self.as_slice().iter().copied()
    }
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
            pitch_rate: CowCol::default(),
            roll_rate: CowCol::default(),
            island_id: CowCol::default(),
            sleep_ticks: CowCol::default(),
            sim_lod: CowCol::default(),
            in_place: CowCol::default(),
            support: CowCol::default(),
            attach_local: CowCol::default(),
            rels: Arc::new(BTreeMap::new()),
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

    /// Cooked `Opaque` id captured with this projection, if any.
    #[must_use]
    pub fn opaque_id(&self) -> Option<AffordanceId> {
        self.opaque_id
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
        let i = self.insert_locus_at_end(s, kind)?;
        Arc::make_mut(&mut self.space_ix).ensure(i);
        Ok(i)
    }

    fn insert_locus_at_end(&mut self, s: Sigil, kind: LocusKind) -> Result<PackedIx, WorldError> {
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
        self.pitch_rate.push(0);
        self.roll_rate.push(0);
        self.island_id.push(0);
        self.sleep_ticks.push(0);
        self.sim_lod.push(SimLod::Full);
        self.in_place.push(None);
        self.support.push(None);
        self.attach_local.push(None);
        Ok(i)
    }

    /// Insert every snap row or none. Rebuilds `space_ix` once.
    pub(crate) fn apply_place_snap(&mut self, snap: &PlaceSnap) -> Result<u32, WorldError> {
        if snap.len() > MAX_PLACE_ROWS {
            return Err(WorldError::PlaceSnap);
        }
        let mut ids: Vec<Sigil> = snap.rows().iter().map(|r| r.sigil).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(WorldError::PlaceSnap);
        }
        let mut new_rows = 0usize;
        let mut packed = Vec::with_capacity(snap.len());
        for row in snap.rows() {
            row_in_place(&row.rels)?;
            let existing = self.packed(row.sigil);
            if existing.is_none() {
                new_rows += 1;
            }
            packed.push(existing);
        }
        if self.sigils.len().saturating_add(new_rows) > self.locus_cap {
            return Err(WorldError::LocusCap);
        }
        self.append_snap_rows(snap, &packed);
        for row in snap.rows() {
            for &(r, b) in &row.rels {
                if r == Rel::In {
                    continue;
                }
                self.add_rel_raw(row.sigil, r, b)?;
            }
        }
        self.rebuild_space_ix();
        Ok(u32::try_from(snap.len()).unwrap_or(u32::MAX))
    }

    fn append_snap_rows(&mut self, snap: &PlaceSnap, packed: &[Option<PackedIx>]) {
        for (row, existing) in snap.rows().iter().zip(packed) {
            if let Some(i) = *existing {
                self.write_snap_row(i, row);
                continue;
            }
            let i = self.sigils.len() as PackedIx;
            Arc::make_mut(&mut self.by_sigil).insert(row.sigil, i);
            self.sigils.push(row.sigil);
            self.kinds.push(row.kind);
            self.afford.push(row.afford);
            self.hull_local.push(row.hull);
            self.hull_id.push(row.hull_id);
            self.pose.push(row.pose);
            self.vel.push(row.vel);
            self.yaw_rate.push(row.yaw_rate);
            self.pitch_rate.push(row.pitch_rate);
            self.roll_rate.push(row.roll_rate);
            self.island_id.push(row.island);
            self.sleep_ticks.push(row.sleep);
            self.sim_lod.push(row.sim_lod);
            self.in_place
                .push(row_in_place(&row.rels).expect("validated"));
            self.support.push(row.support);
            self.attach_local.push(row.attach_local);
            for &(res, v) in &row.qty {
                Arc::make_mut(&mut self.qty).insert((i, res), v);
            }
            if let Some(req) = row.phys_req {
                Arc::make_mut(&mut self.phys_req).insert(i, req);
            }
            for &fact in &row.knows {
                Arc::make_mut(&mut self.knows).insert((i, fact));
            }
            for &(rite, machine) in &row.rites {
                Arc::make_mut(&mut self.rites).insert((i, rite), machine);
            }
        }
    }

    fn write_snap_row(&mut self, i: PackedIx, row: &PlaceRow) {
        self.drop_packed_maps(i);
        let ix = i as usize;
        self.kinds.set(ix, row.kind);
        self.afford.set(ix, row.afford);
        self.vel.set(ix, row.vel);
        self.yaw_rate.set(ix, row.yaw_rate);
        self.pitch_rate.set(ix, row.pitch_rate);
        self.roll_rate.set(ix, row.roll_rate);
        self.support.set(ix, row.support);
        self.attach_local.set(ix, row.attach_local);
        self.island_id.set(ix, row.island);
        self.sleep_ticks.set(ix, row.sleep);
        self.sim_lod.set(ix, row.sim_lod);
        self.pose.set(ix, row.pose);
        if let Some(local) = row.hull {
            self.hull_local.set(ix, Some(local));
            self.hull_id.set(ix, row.hull_id);
        } else {
            self.hull_local.set(ix, None);
            self.hull_id.set(ix, row.hull_id);
        }
        self.in_place
            .set(ix, row_in_place(&row.rels).expect("validated"));
        for &(res, v) in &row.qty {
            Arc::make_mut(&mut self.qty).insert((i, res), v);
        }
        if let Some(req) = row.phys_req {
            Arc::make_mut(&mut self.phys_req).insert(i, req);
        }
        if !row.knows.is_empty() {
            let knows = Arc::make_mut(&mut self.knows);
            for fact in &row.knows {
                knows.insert((i, *fact));
            }
        }
        if !row.rites.is_empty() {
            let rites = Arc::make_mut(&mut self.rites);
            for &(rite, machine) in &row.rites {
                rites.insert((i, rite), machine);
            }
        }
    }

    /// Members with `Rel::In` to `place`, excluding migrating attach/pilot rows.
    pub(crate) fn plan_place_evict(&self, place: Sigil) -> Result<PlaceEvictPlan, WorldError> {
        if self.packed(place).is_none() {
            return Err(WorldError::UnknownLocus);
        }
        let members = self.place_members(place);
        let mut in_place: BTreeSet<Sigil> = members.iter().copied().collect();
        in_place.insert(place);
        let mut drop = Vec::new();
        for s in members {
            if self.is_migrating(s, &in_place) {
                continue;
            }
            drop.push(s);
        }
        let mut rites = Vec::new();
        for s in &drop {
            for (rite, _) in self.rites_of(*s) {
                rites.push((*s, rite));
            }
        }
        Ok(PlaceEvictPlan { drop, rites })
    }

    pub(crate) fn drop_loci(&mut self, drop: &[Sigil]) -> Result<(), WorldError> {
        let mut drop_ix = BTreeSet::new();
        let mut drop_sig = BTreeSet::new();
        for &s in drop {
            let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
            drop_ix.insert(i);
            drop_sig.insert(s);
        }
        self.strip_packed_maps(&drop_ix);
        self.scrub_incoming_set(&drop_sig);
        let mut ordered: Vec<(PackedIx, Sigil)> = drop_sig
            .iter()
            .copied()
            .filter_map(|s| self.packed(s).map(|i| (i, s)))
            .collect();
        ordered.sort_by_key(|(i, _)| core::cmp::Reverse(*i));
        for (_, s) in ordered {
            self.swap_out_locus(s)?;
        }
        Ok(())
    }

    pub(crate) fn remove_locus(&mut self, s: Sigil) -> Result<(), WorldError> {
        self.drop_loci(&[s])
    }

    fn swap_out_locus(&mut self, s: Sigil) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        let last = (self.sigils.len() - 1) as PackedIx;
        if i != last {
            self.swap_packed(i, last);
            self.remap_packed(last, i);
            let moved = self.sigils.get(i as usize).copied().expect("swapped row");
            Arc::make_mut(&mut self.by_sigil).insert(moved, i);
        }
        self.pop_packed();
        Arc::make_mut(&mut self.by_sigil).remove(&s);
        Ok(())
    }

    fn drop_packed_maps(&mut self, ix: PackedIx) {
        let mut one = BTreeSet::new();
        one.insert(ix);
        self.strip_packed_maps(&one);
    }

    fn strip_packed_maps(&mut self, drop_ix: &BTreeSet<PackedIx>) {
        let rels = Arc::make_mut(&mut self.rels);
        rels.retain(|(p, _), _| !drop_ix.contains(p));
        let qty = Arc::make_mut(&mut self.qty);
        qty.retain(|(p, _), _| !drop_ix.contains(p));
        let phys = Arc::make_mut(&mut self.phys_req);
        phys.retain(|p, _| !drop_ix.contains(p));
        let rites = Arc::make_mut(&mut self.rites);
        rites.retain(|(p, _), _| !drop_ix.contains(p));
        let knows = Arc::make_mut(&mut self.knows);
        knows.retain(|(p, _)| !drop_ix.contains(p));
    }

    fn scrub_incoming_set(&mut self, drop_sig: &BTreeSet<Sigil>) {
        let rels = Arc::make_mut(&mut self.rels);
        for v in rels.values_mut() {
            v.retain(|x| !drop_sig.contains(x));
        }
        for i in 0..self.in_place.len() {
            if let Some(s) = self.in_place.get(i).copied().flatten() {
                if drop_sig.contains(&s) {
                    self.in_place.set(i, None);
                }
            }
        }
    }

    fn swap_packed(&mut self, a: PackedIx, b: PackedIx) {
        let a = a as usize;
        let b = b as usize;
        self.sigils.swap(a, b);
        self.kinds.swap(a, b);
        self.afford.swap(a, b);
        self.hull_local.swap(a, b);
        self.hull_id.swap(a, b);
        self.pose.swap(a, b);
        self.vel.swap(a, b);
        self.yaw_rate.swap(a, b);
        self.pitch_rate.swap(a, b);
        self.roll_rate.swap(a, b);
        self.island_id.swap(a, b);
        self.sleep_ticks.swap(a, b);
        self.sim_lod.swap(a, b);
        self.in_place.swap(a, b);
        self.support.swap(a, b);
        self.attach_local.swap(a, b);
    }

    fn remap_packed(&mut self, from: PackedIx, to: PackedIx) {
        let rels = Arc::make_mut(&mut self.rels);
        let rel_keys: Vec<_> = rels.keys().filter(|(p, _)| *p == from).copied().collect();
        for k in rel_keys {
            if let Some(v) = rels.remove(&k) {
                rels.insert((to, k.1), v);
            }
        }
        let qty = Arc::make_mut(&mut self.qty);
        let qty_keys: Vec<_> = qty.keys().filter(|(p, _)| *p == from).copied().collect();
        for k in qty_keys {
            if let Some(v) = qty.remove(&k) {
                qty.insert((to, k.1), v);
            }
        }
        let phys = Arc::make_mut(&mut self.phys_req);
        if let Some(req) = phys.remove(&from) {
            phys.insert(to, req);
        }
        let rites = Arc::make_mut(&mut self.rites);
        let rite_keys: Vec<_> = rites.keys().filter(|(p, _)| *p == from).copied().collect();
        for k in rite_keys {
            if let Some(m) = rites.remove(&k) {
                rites.insert((to, k.1), m);
            }
        }
        let knows = Arc::make_mut(&mut self.knows);
        let know_move: Vec<_> = knows.iter().filter(|(p, _)| *p == from).copied().collect();
        for (p, f) in know_move {
            knows.remove(&(p, f));
            knows.insert((to, f));
        }
    }

    fn pop_packed(&mut self) {
        let _ = self.sigils.pop();
        let _ = self.kinds.pop();
        let _ = self.afford.pop();
        let _ = self.hull_local.pop();
        let _ = self.hull_id.pop();
        let _ = self.pose.pop();
        let _ = self.vel.pop();
        let _ = self.yaw_rate.pop();
        let _ = self.pitch_rate.pop();
        let _ = self.roll_rate.pop();
        let _ = self.island_id.pop();
        let _ = self.sleep_ticks.pop();
        let _ = self.sim_lod.pop();
        let _ = self.in_place.pop();
        let _ = self.support.pop();
        let _ = self.attach_local.pop();
    }

    fn place_members(&self, place: Sigil) -> Vec<Sigil> {
        (0..self.sigils.len() as PackedIx)
            .filter_map(|i| {
                let s = self.sigils.get(i as usize).copied()?;
                (s != place && self.has_rel(s, Rel::In, place)).then_some(s)
            })
            .collect()
    }

    fn is_migrating(&self, s: Sigil, in_place: &BTreeSet<Sigil>) -> bool {
        self.related_slice(s, Rel::AttachedTo)
            .iter()
            .chain(self.related_slice(s, Rel::PilotedBy))
            .any(|host| !in_place.contains(host))
    }

    fn rites_of(&self, actor: Sigil) -> Vec<(u16, RiteMachine)> {
        let Some(i) = self.packed(actor) else {
            return Vec::new();
        };
        self.rites
            .iter()
            .filter(|((ix, _), _)| *ix == i)
            .map(|((_, rite), m)| (*rite, *m))
            .collect()
    }

    pub(crate) fn capture_place(
        &self,
        place: Sigil,
        canon_hash: Hash,
        prefix: Hash,
    ) -> Option<PlaceSnap> {
        self.packed(place)?;
        let mut rows = Vec::new();
        rows.push(self.capture_row(place)?);
        for s in self.place_members(place) {
            rows.push(self.capture_row(s)?);
        }
        Some(PlaceSnap::new(place, canon_hash, prefix, rows))
    }

    pub(crate) fn capture_snap_rows(&self) -> Vec<SnapRow> {
        (0..self.len() as PackedIx)
            .filter_map(|i| {
                let s = self.sigil(i)?;
                self.capture_snap_row(s)
            })
            .collect()
    }

    fn capture_snap_row(&self, s: Sigil) -> Option<SnapRow> {
        let i = self.packed(s)?;
        let ix = i as usize;
        let kind = self.kinds.get(ix).copied()?;
        let qty = self
            .qty
            .iter()
            .filter(|((p, _), _)| *p == i)
            .map(|((_, r), v)| (*r, *v))
            .collect();
        let mut rels: Vec<(Rel, Sigil)> = self
            .rels
            .iter()
            .filter(|((p, _), _)| *p == i)
            .flat_map(|((_, k), n)| {
                Rel::from_u8(*k)
                    .into_iter()
                    .flat_map(move |r| n.iter().map(move |b| (r, b)))
            })
            .collect();
        if let Some(p) = self.in_place.get(i as usize).copied().flatten() {
            rels.push((Rel::In, p));
        }
        let knows = self
            .knows
            .iter()
            .filter(|(p, _)| *p == i)
            .map(|(_, f)| *f)
            .collect();
        Some(SnapRow {
            sigil: s,
            kind,
            pose: self.pose.get(ix).copied().flatten(),
            vel: self.vel.get(ix).copied().unwrap_or(Vel3::ZERO),
            yaw_rate: self.yaw_rate.get(ix).copied().unwrap_or(0),
            pitch_rate: self.pitch_rate.get(ix).copied().unwrap_or(0),
            roll_rate: self.roll_rate.get(ix).copied().unwrap_or(0),
            sleep_ticks: self.sleep_ticks.get(ix).copied().unwrap_or(0),
            island: self.island_id.get(ix).copied().unwrap_or(0),
            support: self.support.get(ix).copied().flatten(),
            phys_req: self.phys_req.get(&i).copied(),
            attach_local: self.attach_local.get(ix).copied().flatten(),
            rites: self.rites_of(s),
            rels,
            qty,
            knows,
            hull: self.hull_local.get(ix).copied().flatten(),
            hull_id: self.hull_id.get(ix).copied().unwrap_or(BlobId::ZERO),
            afford: self.afford.get(ix).copied().unwrap_or(0),
            sim_lod: self.sim_lod.get(ix).copied().unwrap_or(SimLod::Full),
        })
    }

    pub(crate) fn from_snap_rows(
        opaque: Option<AffordanceId>,
        rows: &[SnapRow],
    ) -> Result<Projection, SnapError> {
        if rows.len() > MAX_SNAP_ROWS {
            return Err(SnapError::Oversize {
                size: rows.len(),
                cap: MAX_SNAP_ROWS,
            });
        }
        let mut ids: Vec<Sigil> = rows.iter().map(|r| r.sigil).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(SnapError::Duplicate);
        }
        for row in rows {
            check_row_caps(row)?;
            snap_row_in_place(&row.rels)?;
        }
        let mut p = Projection::with_cap(opaque, klotho_core::MAX_LOCI_PROCESS);
        for row in rows {
            p.push_snap_row(row);
        }
        for row in rows {
            for &(r, b) in &row.rels {
                if r == Rel::In {
                    continue;
                }
                p.add_rel_raw(row.sigil, r, b)
                    .map_err(|_| SnapError::Kind)?;
            }
        }
        for row in rows {
            for &(rite, m) in &row.rites {
                p.put_rite(row.sigil, RiteId(rite), m);
            }
        }
        p.rebuild_space_ix();
        Ok(p)
    }

    fn push_snap_row(&mut self, row: &SnapRow) {
        let i = self.sigils.len() as PackedIx;
        Arc::make_mut(&mut self.by_sigil).insert(row.sigil, i);
        self.sigils.push(row.sigil);
        self.kinds.push(row.kind);
        self.afford.push(row.afford);
        self.hull_local.push(row.hull);
        self.hull_id.push(row.hull_id);
        self.pose.push(row.pose);
        self.vel.push(row.vel);
        self.yaw_rate.push(row.yaw_rate);
        self.pitch_rate.push(row.pitch_rate);
        self.roll_rate.push(row.roll_rate);
        self.island_id.push(row.island);
        self.sleep_ticks.push(row.sleep_ticks);
        self.sim_lod.push(row.sim_lod);
        self.in_place
            .push(snap_row_in_place(&row.rels).expect("validated"));
        self.support.push(row.support);
        self.attach_local.push(row.attach_local);
        for &(res, v) in &row.qty {
            Arc::make_mut(&mut self.qty).insert((i, res), v);
        }
        if let Some(req) = row.phys_req {
            Arc::make_mut(&mut self.phys_req).insert(i, req);
        }
        for &fact in &row.knows {
            Arc::make_mut(&mut self.knows).insert((i, fact));
        }
    }

    fn capture_row(&self, s: Sigil) -> Option<PlaceRow> {
        let i = self.packed(s)?;
        let ix = i as usize;
        let kind = self.kinds.get(ix).copied()?;
        let qty = self
            .qty
            .iter()
            .filter(|((p, _), _)| *p == i)
            .map(|((_, r), v)| (*r, *v))
            .collect();
        let mut rels: Vec<(Rel, Sigil)> = self
            .rels
            .iter()
            .filter(|((p, _), _)| *p == i)
            .flat_map(|((_, k), n)| {
                Rel::from_u8(*k)
                    .into_iter()
                    .flat_map(move |r| n.iter().map(move |b| (r, b)))
            })
            .collect();
        if let Some(p) = self.in_place.get(i as usize).copied().flatten() {
            rels.push((Rel::In, p));
        }
        let knows = self
            .knows
            .iter()
            .filter(|(p, _)| *p == i)
            .map(|(_, f)| *f)
            .collect();
        Some(PlaceRow {
            sigil: s,
            kind,
            pose: self.pose.get(ix).copied().flatten(),
            vel: self.vel.get(ix).copied().unwrap_or(Vel3::ZERO),
            yaw_rate: self.yaw_rate.get(ix).copied().unwrap_or(0),
            pitch_rate: self.pitch_rate.get(ix).copied().unwrap_or(0),
            roll_rate: self.roll_rate.get(ix).copied().unwrap_or(0),
            hull: self.hull_local.get(ix).copied().flatten(),
            hull_id: self.hull_id.get(ix).copied().unwrap_or(BlobId::ZERO),
            afford: self.afford.get(ix).copied().unwrap_or(0),
            qty,
            rels,
            island: self.island_id.get(ix).copied().unwrap_or(0),
            sleep: self.sleep_ticks.get(ix).copied().unwrap_or(0),
            sim_lod: self.sim_lod.get(ix).copied().unwrap_or(SimLod::Full),
            phys_req: self.phys_req.get(&i).copied(),
            support: self.support.get(ix).copied().flatten(),
            attach_local: self.attach_local.get(ix).copied().flatten(),
            rites: self.rites_of(s),
            knows,
        })
    }

    /// Packed kind, if the locus exists.
    #[must_use]
    pub fn kind(&self, s: Sigil) -> Option<LocusKind> {
        let i = self.packed(s)?;
        self.kinds.get(i as usize).copied()
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

    pub(crate) fn set_rates(
        &mut self,
        s: Sigil,
        yaw_rate: i32,
        pitch_rate: i32,
        roll_rate: i32,
    ) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        self.yaw_rate.set(i as usize, yaw_rate);
        self.pitch_rate.set(i as usize, pitch_rate);
        self.roll_rate.set(i as usize, roll_rate);
        Ok(())
    }

    pub(crate) fn set_support(
        &mut self,
        s: Sigil,
        support: Option<Support>,
    ) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        self.support.set(i as usize, support);
        Ok(())
    }

    pub(crate) fn set_attach_local(
        &mut self,
        s: Sigil,
        local: Option<IVec3>,
    ) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        self.attach_local.set(i as usize, local);
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

    pub(crate) fn set_sim_lod(&mut self, s: Sigil, lod: SimLod) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        self.sim_lod.set(i as usize, lod);
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

    pub(crate) fn clear_phys_req(&mut self, s: Sigil) -> Result<(), WorldError> {
        let i = self.packed(s).ok_or(WorldError::UnknownLocus)?;
        Arc::make_mut(&mut self.phys_req).remove(&i);
        Ok(())
    }

    /// Current `PHYS_REQ` write, if any.
    #[must_use]
    pub fn phys_req(&self, s: Sigil) -> Option<PhysRequest> {
        let i = self.packed(s)?;
        self.phys_req.get(&i).copied()
    }

    pub(crate) fn add_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        let ia = self.add_rel_raw(a, r, b)?;
        if (r == Rel::AttachedTo || r == Rel::PilotedBy)
            && self
                .attach_local
                .get(ia as usize)
                .copied()
                .flatten()
                .is_none()
        {
            if let Some(local) = default_attach_local(self.pose(a), self.pose(b)) {
                self.attach_local.set(ia as usize, Some(local));
            }
        }
        if r == Rel::LockedBy || r == Rel::In {
            self.reindex_ix(ia);
        }
        Ok(())
    }

    fn add_rel_raw(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<PackedIx, WorldError> {
        let ia = self.packed(a).ok_or(WorldError::UnknownLocus)?;
        if r == Rel::In {
            self.in_place.set(ia as usize, Some(b));
            return Ok(ia);
        }
        let key = rel_key(r);
        let n = Arc::make_mut(&mut self.rels).entry((ia, key)).or_default();
        if !n.as_slice().contains(&b) {
            n.push(b);
        }
        Ok(ia)
    }

    pub(crate) fn del_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        let ia = self.packed(a).ok_or(WorldError::UnknownLocus)?;
        if r == Rel::In {
            if self.in_place.get(ia as usize).copied().flatten() == Some(b) {
                self.in_place.set(ia as usize, None);
            }
            self.reindex_ix(ia);
            return Ok(());
        }
        let key = rel_key(r);
        if let Some(v) = Arc::make_mut(&mut self.rels).get_mut(&(ia, key)) {
            v.retain(|&x| x != b);
        }
        if r == Rel::AttachedTo || r == Rel::PilotedBy {
            let still = self
                .related_slice(a, Rel::AttachedTo)
                .iter()
                .chain(self.related_slice(a, Rel::PilotedBy))
                .next()
                .is_some();
            if !still {
                self.attach_local.set(ia as usize, None);
            }
        }
        if r == Rel::LockedBy {
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
            TraceBody::Spawned { sigil, at, .. } => {
                if self.insert_locus(*sigil, LocusKind::Relic).is_ok() {
                    let _ = self.set_pose(*sigil, *at);
                }
            }
            TraceBody::SaveRequested
            | TraceBody::Emitted { .. }
            | TraceBody::Uttered { .. }
            | TraceBody::PlaceLoaded { .. }
            | TraceBody::PlaceEvicted { .. }
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
        self.in_place.get(ix as usize).copied().flatten()
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
        self.related_slice(a, r).contains(&b)
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
        if r == Rel::In {
            return match self.in_place.get(i as usize) {
                Some(Some(p)) => std::slice::from_ref(p),
                _ => &[],
            };
        }
        self.rels
            .get(&(i, rel_key(r)))
            .map(Neighbors::as_slice)
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

    /// `(yaw_rate, pitch_rate, roll_rate)`.
    #[must_use]
    pub fn rates(&self, s: Sigil) -> Option<(i32, i32, i32)> {
        let i = self.packed(s)?;
        Some((
            self.yaw_rate.get(i as usize).copied().unwrap_or(0),
            self.pitch_rate.get(i as usize).copied().unwrap_or(0),
            self.roll_rate.get(i as usize).copied().unwrap_or(0),
        ))
    }

    /// Last admitted PhysDelta support, if any.
    #[must_use]
    pub fn support(&self, s: Sigil) -> Option<Support> {
        let i = self.packed(s)?;
        self.support.get(i as usize).copied().flatten()
    }

    /// Seat offset in the parent's yaw frame. Missing until Rel add or a write.
    #[must_use]
    pub fn attach_local(&self, s: Sigil) -> Option<IVec3> {
        let i = self.packed(s)?;
        self.attach_local.get(i as usize).copied().flatten()
    }

    /// Parent of `PilotedBy` / `AttachedTo`, if any.
    #[must_use]
    pub fn attach_parent(&self, s: Sigil) -> Option<Sigil> {
        self.related_slice(s, Rel::PilotedBy)
            .iter()
            .copied()
            .chain(self.related_slice(s, Rel::AttachedTo).iter().copied())
            .next()
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

    /// Simulation LOD. Unknown locus is [`SimLod::Full`].
    #[must_use]
    pub fn sim_lod(&self, s: Sigil) -> SimLod {
        let Some(i) = self.packed(s) else {
            return SimLod::Full;
        };
        self.sim_lod
            .get(i as usize)
            .copied()
            .unwrap_or(SimLod::Full)
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
        bytes += self.pitch_rate.approx_bytes();
        bytes += self.roll_rate.approx_bytes();
        bytes += self.support.approx_bytes();
        bytes += self.attach_local.approx_bytes();
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

fn row_in_place(rels: &[(Rel, Sigil)]) -> Result<Option<Sigil>, WorldError> {
    let mut found = None;
    for &(r, b) in rels {
        if r != Rel::In {
            continue;
        }
        if found.is_some() {
            return Err(WorldError::PlaceSnap);
        }
        found = Some(b);
    }
    Ok(found)
}

fn snap_row_in_place(rels: &[(Rel, Sigil)]) -> Result<Option<Sigil>, SnapError> {
    let mut found = None;
    for &(r, b) in rels {
        if r != Rel::In {
            continue;
        }
        if found.is_some() {
            return Err(SnapError::Duplicate);
        }
        found = Some(b);
    }
    Ok(found)
}

fn rel_from_tag(t: RelTag) -> Option<Rel> {
    Rel::from_u8(t.0)
}

fn default_attach_local(child: Option<PoseMm>, parent: Option<PoseMm>) -> Option<IVec3> {
    let child = child?;
    let parent = parent?;
    let delta = IVec3 {
        x: child.x.0.wrapping_sub(parent.x.0),
        y: child.y.0.wrapping_sub(parent.y.0),
        z: child.z.0.wrapping_sub(parent.z.0),
    };
    Some(rotate_xz(delta, -parent.yaw))
}
