//! Read path. Same API from live `World` and `WorldSnapshot`.

use klotho_canon::{PredStore, RiteId};
use klotho_core::{
    AabbMm, AffordanceId, PackedIx, PhysRequest, PoseMm, ResourceId, Sigil, SimLod, Tick, Vel3,
};
use klotho_ir::{Channel, Rel};

use crate::proj::Projection;

/// Borrowed projection queries. No writes.
#[derive(Copy, Clone, Debug)]
pub struct WorldView<'a> {
    pub(crate) proj: &'a Projection,
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
        WorldView { proj, tick }
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
