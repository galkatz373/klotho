//! Read path. Same API from live `World` and `WorldSnapshot`.

use klotho_canon::{PredStore, RiteId};
use klotho_core::{
    AabbMm, AffordanceId, Epoch, Hash, IVec3, LocusKind, PackedIx, PhysRequest, PoseMm, ResourceId,
    Sigil, SimLod, Support, Tick, Vel3, frac_cmp,
};
use klotho_ir::{Channel, Rel};

use crate::proj::Projection;

/// Engine Fire range cap (50 m). Rewind hitscan only; not a Law.
pub const HITSCAN_RANGE_MM: i32 = 50_000;

/// Borrowed projection queries. No writes.
#[derive(Copy, Clone, Debug)]
pub struct WorldView<'a> {
    pub(crate) proj: &'a Projection,
    pub(crate) epoch: Epoch,
    pub(crate) tick: Tick,
}

impl WorldView<'_> {
    /// Wrap a projection at tick 0 (tests). Prefer [`Self::at`].
    #[must_use]
    pub fn of(proj: &Projection) -> WorldView<'_> {
        Self::at(proj, Tick::ZERO)
    }

    /// Wrap a projection at `tick`. Motion derives clip time from this (K22).
    #[must_use]
    pub fn at(proj: &Projection, tick: Tick) -> WorldView<'_> {
        Self::at_epoch(proj, Epoch::ZERO, tick)
    }

    /// Wrap a projection at its Canon epoch and tick.
    #[must_use]
    pub fn at_epoch(proj: &Projection, epoch: Epoch, tick: Tick) -> WorldView<'_> {
        WorldView { proj, epoch, tick }
    }

    /// Canon epoch this view was taken at.
    #[must_use]
    pub fn epoch(self) -> Epoch {
        self.epoch
    }

    /// World tick this view was taken at.
    #[must_use]
    pub fn tick(self) -> Tick {
        self.tick
    }

    /// Loci that currently hold `a`.
    pub fn with_affordance(&self, a: AffordanceId) -> impl Iterator<Item = Sigil> + '_ {
        self.proj.with_affordance(a)
    }

    /// Every packed locus, packed-index order (dense, deterministic).
    pub fn loci(&self) -> impl Iterator<Item = Sigil> + '_ {
        (0..self.proj.len() as PackedIx).filter_map(|i| self.proj.sigil(i))
    }

    /// True if `s` is in the identity table.
    #[must_use]
    pub fn contains(self, s: Sigil) -> bool {
        self.proj.packed(s).is_some()
    }

    /// Packed kind, if present.
    #[must_use]
    pub fn kind(self, s: Sigil) -> Option<LocusKind> {
        self.proj.kind(s)
    }

    /// Capture Place + `Rel::In` members. `None` if `place` is unknown.
    #[must_use]
    pub fn capture_place(
        self,
        place: Sigil,
        canon_hash: Hash,
        prefix: Hash,
    ) -> Option<crate::PlaceSnap> {
        self.proj.capture_place(place, canon_hash, prefix)
    }

    /// Affordance bit.
    #[must_use]
    pub fn has_affordance(&self, s: Sigil, a: AffordanceId) -> bool {
        self.proj.has_affordance(s, a)
    }

    /// Relation triple.
    #[must_use]
    pub fn has_rel(&self, a: Sigil, r: Rel, b: Sigil) -> bool {
        self.proj.has_rel(a, r, b)
    }

    /// Neighbors of `a` along `r`.
    pub fn related(&self, a: Sigil, r: Rel) -> impl Iterator<Item = Sigil> + '_ {
        self.proj.related_slice(a, r).iter().copied()
    }

    /// Quantity; missing is 0.
    #[must_use]
    pub fn qty(&self, s: Sigil, r: ResourceId) -> i32 {
        self.proj.qty(s, r)
    }

    /// Current `PHYS_REQ` write, if any.
    #[must_use]
    pub fn phys_req(&self, s: Sigil) -> Option<PhysRequest> {
        self.proj.phys_req(s)
    }

    /// Pose.
    #[must_use]
    pub fn pose(&self, s: Sigil) -> Option<PoseMm> {
        self.proj.pose(s)
    }

    /// `(vel, yaw_rate)`.
    #[must_use]
    pub fn vel(&self, s: Sigil) -> Option<(Vel3, i32)> {
        self.proj.vel(s)
    }

    /// `(yaw_rate, pitch_rate, roll_rate)`.
    #[must_use]
    pub fn rates(&self, s: Sigil) -> Option<(i32, i32, i32)> {
        self.proj.rates(s)
    }

    /// Last admitted physical-island support, if any.
    #[must_use]
    pub fn support(&self, s: Sigil) -> Option<Support> {
        self.proj.support(s)
    }

    /// Seat offset in the parent's yaw frame.
    #[must_use]
    pub fn attach_local(&self, s: Sigil) -> Option<IVec3> {
        self.proj.attach_local(s)
    }

    /// Parent of `PilotedBy` / `AttachedTo`, if any.
    #[must_use]
    pub fn attach_parent(&self, s: Sigil) -> Option<Sigil> {
        self.proj.attach_parent(s)
    }

    /// `(island_id, sleep_ticks)`.
    #[must_use]
    pub fn island(&self, s: Sigil) -> Option<(u16, u16)> {
        self.proj.island(s)
    }

    /// Simulation LOD. Unknown locus is [`SimLod::Full`].
    #[must_use]
    pub fn sim_lod(&self, s: Sigil) -> SimLod {
        self.proj.sim_lod(s)
    }

    /// `Opaque ∧ LockedBy`.
    #[must_use]
    pub fn opaque_closed(&self, s: Sigil) -> bool {
        self.proj.opaque_closed(s)
    }

    /// Posed hull.
    #[must_use]
    pub fn posed_hull(&self, s: Sigil) -> Option<AabbMm> {
        self.proj.posed_hull(s)
    }

    /// `space_ix` candidates (unplaced ∪ overlapping Places).
    /// Admission uses `opaque_closed_only = true`. Per-Place isolation is
    /// [`PlaceIndex::grid`](crate::PlaceIndex::grid).
    #[must_use]
    pub fn space_candidates(&self, swept: AabbMm, opaque_closed_only: bool) -> Vec<Sigil> {
        self.proj
            .space_ix()
            .candidates(swept, opaque_closed_only)
            .into_iter()
            .filter_map(|ix| self.proj.sigil(ix))
            .collect()
    }

    /// First `hittable` locus along the closed segment, excluding `skip`.
    /// Ties break by packed-index order from [`Self::space_candidates`].
    #[must_use]
    pub fn hitscan(
        self,
        origin: IVec3,
        dir: IVec3,
        skip: Sigil,
        hittable: AffordanceId,
    ) -> Option<Sigil> {
        let end = IVec3 {
            x: origin.x.wrapping_add(dir.x),
            y: origin.y.wrapping_add(dir.y),
            z: origin.z.wrapping_add(dir.z),
        };
        let swept = AabbMm::from_point(origin).swept_union(AabbMm::from_point(end));
        let mut best: Option<((i64, i64), Sigil)> = None;
        for s in self.space_candidates(swept, false) {
            if s == skip || !self.has_affordance(s, hittable) {
                continue;
            }
            let Some(hull) = self.posed_hull(s) else {
                continue;
            };
            let Some(t) = hull.segment_hit(origin, dir) else {
                continue;
            };
            match best {
                None => best = Some((t, s)),
                Some((bt, _)) if frac_cmp(t.0, t.1, bt.0, bt.1) == core::cmp::Ordering::Less => {
                    best = Some((t, s));
                }
                _ => {}
            }
        }
        best.map(|(_, s)| s)
    }

    /// Canonical hull blob. [`klotho_core::BlobId::ZERO`] if unbound.
    #[must_use]
    pub fn hull_id(&self, s: Sigil) -> Option<klotho_core::BlobId> {
        self.proj.hull_id(s)
    }

    /// Local (unposed) hull AABB.
    #[must_use]
    pub fn hull(&self, s: Sigil) -> Option<AabbMm> {
        self.proj.hull(s)
    }

    /// Active rite, if any.
    #[must_use]
    pub fn first_rite(&self, s: Sigil) -> Option<(RiteId, crate::RiteMachine)> {
        self.proj.first_rite(s)
    }

    /// Named rite row.
    #[must_use]
    pub fn rite(&self, actor: Sigil, rite: RiteId) -> Option<crate::RiteMachine> {
        self.proj.rite(actor, rite)
    }

    /// Knows bit. Missing mind or fact is `false`.
    #[must_use]
    pub fn knows(&self, mind: Sigil, fact: u16) -> bool {
        self.proj.knows(mind, fact)
    }
}

impl PredStore for WorldView<'_> {
    fn has_affordance(&self, s: Sigil, a: AffordanceId) -> bool {
        self.proj.has_affordance(s, a)
    }

    fn has_rel(&self, a: Sigil, r: Rel, b: Sigil) -> bool {
        self.proj.has_rel(a, r, b)
    }

    fn related(&self, a: Sigil, r: Rel, out: &mut Vec<Sigil>) {
        self.proj.related(a, r, out);
    }

    fn qty(&self, s: Sigil, r: ResourceId) -> i32 {
        self.proj.qty(s, r)
    }

    fn aabb(&self, s: Sigil) -> Option<AabbMm> {
        self.proj.posed_hull(s)
    }

    fn knows(&self, mind: Sigil, fact: u16) -> bool {
        self.proj.knows(mind, fact)
    }

    fn rite_active(&self, actor: Sigil, rite: RiteId) -> bool {
        self.proj.rite(actor, rite).is_some()
    }

    fn in_window(&self, rite: RiteId, _ch: Channel) -> bool {
        (0..self.proj.len() as PackedIx).any(|ix| {
            self.proj
                .sigil(ix)
                .and_then(|s| self.proj.rite(s, rite))
                .is_some_and(|m| m.wait_left > 0)
        })
    }

    fn sleep_ticks(&self, s: Sigil) -> Option<u16> {
        self.proj.island(s).map(|(_, t)| t)
    }

    fn sim_lod(&self, s: Sigil) -> SimLod {
        self.proj.sim_lod(s)
    }
}
