//! Write path. Public via [`World::mutate`] when feature `mutate` is on
//! (`klotho-commit` only). Always compiled so projection writers stay linked.

use klotho_core::{
    AabbMm, AffordanceId, BlobId, LocusKind, PackedIx, PoseMm, ResourceId, Sigil, Tick, Vel3,
};
use klotho_ir::{PlayerIntent, Rel};
use klotho_trace::TraceEvent;

use crate::error::WorldError;
use crate::spec::SpecDelta;
use crate::world::World;

/// Exclusive write handle. Only `klotho-commit` should construct this at runtime.
#[cfg_attr(not(any(test, feature = "mutate")), allow(dead_code))]
pub struct WorldMut<'a> {
    world: &'a mut World,
}

impl World {
    /// Open the write path. Runtime: `klotho-commit` only (`mutate` feature).
    #[cfg(any(test, feature = "mutate"))]
    #[must_use]
    pub fn mutate(&mut self) -> WorldMut<'_> {
        WorldMut { world: self }
    }
}

#[cfg_attr(not(any(test, feature = "mutate")), allow(dead_code))]
impl WorldMut<'_> {
    /// Allocate a locus. Existing sigils are returned as-is.
    pub fn insert_locus(&mut self, s: Sigil, kind: LocusKind) -> Result<PackedIx, WorldError> {
        self.world.projection_mut().insert_locus(s, kind)
    }

    /// Set or clear an affordance bit. Reindexes `space_ix` if Opaque.
    pub fn set_affordance(
        &mut self,
        s: Sigil,
        a: AffordanceId,
        on: bool,
    ) -> Result<(), WorldError> {
        self.world.projection_mut().set_affordance(s, a, on)
    }

    /// Bind a local hull AABB and blob id. Reindexes `space_ix`.
    pub fn set_hull(&mut self, s: Sigil, local: AabbMm, id: BlobId) -> Result<(), WorldError> {
        self.world.projection_mut().set_hull(s, local, id)
    }

    /// Set pose. Reindexes `space_ix`.
    pub fn set_pose(&mut self, s: Sigil, p: PoseMm) -> Result<(), WorldError> {
        self.world.projection_mut().set_pose(s, p)
    }

    /// Set velocity columns.
    pub fn set_vel(&mut self, s: Sigil, vel: Vel3, yaw_rate: i32) -> Result<(), WorldError> {
        self.world.projection_mut().set_vel(s, vel, yaw_rate)
    }

    /// Set island id and sleep ticks.
    pub fn set_island(&mut self, s: Sigil, island: u16, sleep: u16) -> Result<(), WorldError> {
        self.world.projection_mut().set_island(s, island, sleep)
    }

    /// Set a quantity row.
    pub fn set_qty(&mut self, s: Sigil, r: ResourceId, v: i32) -> Result<(), WorldError> {
        self.world.projection_mut().set_qty(s, r, v)
    }

    /// Insert a relation. Reindexes `space_ix` when Place membership or
    /// OpaqueClosed changes (`Rel::In`, `Rel::LockedBy`).
    pub fn add_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        self.world.projection_mut().add_rel(a, r, b)
    }

    /// Delete a relation. Reindexes `space_ix` when Place membership or
    /// OpaqueClosed changes (`Rel::In`, `Rel::LockedBy`).
    pub fn del_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        self.world.projection_mut().del_rel(a, r, b)
    }

    /// Append an admitted event to Trace and apply it to the projection.
    /// Prefix hash changes.
    pub fn append(&mut self, e: TraceEvent) {
        if e.tick > self.world.tick() {
            self.world.set_tick(e.tick);
        }
        self.world.projection_mut().apply_event(&e);
        self.world.trace_mut().append(e);
    }

    /// Rebuild `space_ix` from hull, pose, OpaqueClosed, and Place membership.
    pub fn rebuild_space_ix(&mut self) {
        self.world.projection_mut().rebuild_space_ix();
    }

    /// Enqueue a player intent for this tick.
    pub fn push_intent(&mut self, p: PlayerIntent) {
        self.world.intents_mut().push_player(p);
    }

    /// Drain the Intent heap (end of tick).
    pub fn clear_intents(&mut self) {
        self.world.intents_mut().clear();
    }

    /// Set the global tick.
    pub fn set_tick(&mut self, t: Tick) {
        self.world.set_tick(t);
    }

    /// Fork projection for a proposal-local transaction (K21).
    #[must_use]
    pub fn begin_spec(&self) -> SpecDelta {
        SpecDelta::from_parts(self.world.projection().clone(), self.world.tick())
    }

    /// Atomic install of a successful spec: replace projection, append Trace
    /// (events already applied on the spec — do not apply twice).
    pub fn commit_spec(&mut self, spec: SpecDelta) {
        let (proj, events) = spec.into_parts();
        *self.world.projection_mut() = proj;
        for e in events {
            self.world.trace_mut().append(e);
        }
    }
}
