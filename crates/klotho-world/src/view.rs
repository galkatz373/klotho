//! Read path. Same API from live `World` and `WorldSnapshot`.

use klotho_canon::{PredStore, RiteId};
use klotho_core::{AabbMm, AffordanceId, PoseMm, ResourceId, Sigil, VelFx};
use klotho_ir::{Channel, Rel};

use crate::proj::Projection;

/// Borrowed projection queries. No writes.
#[derive(Copy, Clone, Debug)]
pub struct WorldView<'a> {
    pub(crate) proj: &'a Projection,
}

impl WorldView<'_> {
    /// Wrap a projection (speculative or live).
    #[must_use]
    pub fn of(proj: &Projection) -> WorldView<'_> {
        WorldView { proj }
    }

    /// Loci that currently hold `a`.
    pub fn with_affordance(&self, a: AffordanceId) -> impl Iterator<Item = Sigil> + '_ {
        self.proj.with_affordance(a)
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

    /// Pose.
    #[must_use]
    pub fn pose(&self, s: Sigil) -> Option<PoseMm> {
        self.proj.pose(s)
    }

    /// `(vel_x, vel_z, yaw_rate)`.
    #[must_use]
    pub fn vel(&self, s: Sigil) -> Option<(VelFx, VelFx, i32)> {
        self.proj.vel(s)
    }

    /// `(island_id, sleep_ticks)`.
    #[must_use]
    pub fn island(&self, s: Sigil) -> Option<(u16, u16)> {
        self.proj.island(s)
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

    /// `space_ix` candidates. Admission uses `opaque_closed_only = true`.
    #[must_use]
    pub fn space_candidates(&self, swept: AabbMm, opaque_closed_only: bool) -> Vec<Sigil> {
        self.proj
            .space_ix()
            .candidates(swept, opaque_closed_only)
            .into_iter()
            .filter_map(|slot| self.proj.sigil(slot))
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
        (0..self.proj.len() as u16).any(|slot| {
            self.proj
                .sigil(slot)
                .and_then(|s| self.proj.rite(s, rite))
                .is_some_and(|m| m.wait_left > 0)
        })
    }

    fn sleep_ticks(&self, s: Sigil) -> Option<u16> {
        self.proj.island(s).map(|(_, t)| t)
    }
}
