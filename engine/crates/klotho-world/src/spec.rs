//! Speculative projection + event list (K21). Drop = rollback.

use klotho_canon::RiteId;
use klotho_core::{
    AabbMm, AffordanceId, BlobId, IVec3, PhysRequest, PoseMm, ResourceId, Sigil, Support, Tick,
    Vel3,
};
use klotho_ir::Rel;
use klotho_trace::{RiteEnd, TraceBody, TraceEvent};

use crate::error::WorldError;
use crate::proj::{Projection, RiteMachine};
use crate::snap::PlaceSnap;
use crate::view::WorldView;

/// Speculative overlay: CoW-cloned projection plus events not yet on Trace.
#[cfg_attr(not(any(test, feature = "mutate")), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct SpecDelta {
    proj: Projection,
    events: Vec<TraceEvent>,
    tick: Tick,
}

#[cfg_attr(not(any(test, feature = "mutate")), allow(dead_code))]
impl SpecDelta {
    /// Read the would-be post-state.
    #[must_use]
    pub fn view(&self) -> WorldView<'_> {
        WorldView::at(&self.proj, self.tick)
    }

    /// Tick these events are stamped with.
    #[must_use]
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// Events collected for atomic commit.
    #[must_use]
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Apply an event to the spec projection and queue it.
    pub fn push(&mut self, e: TraceEvent) {
        self.proj.apply_event(&e);
        self.events.push(e);
    }

    /// Write pose on the spec (then typically [`Self::push`] a `PoseCommitted`).
    pub fn set_pose(&mut self, s: Sigil, p: PoseMm) -> Result<(), WorldError> {
        self.proj.set_pose(s, p)
    }

    /// Write vel columns on the spec.
    pub fn set_vel(&mut self, s: Sigil, vel: Vel3, yaw_rate: i32) -> Result<(), WorldError> {
        self.proj.set_vel(s, vel, yaw_rate)
    }

    /// Write yaw/pitch/roll rates on the spec.
    pub fn set_rates(
        &mut self,
        s: Sigil,
        yaw_rate: i32,
        pitch_rate: i32,
        roll_rate: i32,
    ) -> Result<(), WorldError> {
        self.proj.set_rates(s, yaw_rate, pitch_rate, roll_rate)
    }

    /// Write support on the spec.
    pub fn set_support(&mut self, s: Sigil, support: Option<Support>) -> Result<(), WorldError> {
        self.proj.set_support(s, support)
    }

    /// Write seat offset on the spec.
    pub fn set_attach_local(&mut self, s: Sigil, local: Option<IVec3>) -> Result<(), WorldError> {
        self.proj.set_attach_local(s, local)
    }

    /// Write island/sleep on the spec.
    pub fn set_island(&mut self, s: Sigil, island: u16, sleep: u16) -> Result<(), WorldError> {
        self.proj.set_island(s, island, sleep)
    }

    /// Write a quantity without a Trace event (tests / Conserve pre-read).
    pub fn set_qty(&mut self, s: Sigil, r: ResourceId, v: i32) -> Result<(), WorldError> {
        self.proj.set_qty(s, r, v)
    }

    /// Affordance bit. Spawn uses this so Cap sees the mark this tick.
    pub fn set_affordance(
        &mut self,
        s: Sigil,
        a: AffordanceId,
        on: bool,
    ) -> Result<(), WorldError> {
        self.proj.set_affordance(s, a, on)
    }

    /// Write a `PHYS_REQ` column. Not a quantity.
    pub fn set_phys_req(&mut self, s: Sigil, req: PhysRequest) -> Result<(), WorldError> {
        self.proj.set_phys_req(s, req)
    }

    /// Drop a consumed `PHYS_REQ` row.
    pub fn clear_phys_req(&mut self, s: Sigil) -> Result<(), WorldError> {
        self.proj.clear_phys_req(s)
    }

    /// Relation write.
    pub fn add_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        self.proj.add_rel(a, r, b)
    }

    /// Relation delete.
    pub fn del_rel(&mut self, a: Sigil, r: Rel, b: Sigil) -> Result<(), WorldError> {
        self.proj.del_rel(a, r, b)
    }

    /// Local hull.
    pub fn set_hull(&mut self, s: Sigil, local: AabbMm, id: BlobId) -> Result<(), WorldError> {
        self.proj.set_hull(s, local, id)
    }

    /// Patch a rite machine (WAIT channel, pc).
    pub fn put_rite(&mut self, actor: Sigil, rite: RiteId, m: RiteMachine) {
        self.proj.put_rite(actor, rite, m);
    }

    /// Insert every snap row or none, then queue `PlaceLoaded`.
    pub fn apply_place_snap(&mut self, snap: &PlaceSnap) -> Result<u32, WorldError> {
        let n = self.proj.apply_place_snap(snap)?;
        self.push(TraceEvent::new(
            self.tick,
            TraceBody::PlaceLoaded {
                place: snap.place,
                n,
            },
        ));
        Ok(n)
    }

    /// Drop place-owned rows, end rites as `Evicted`, queue `PlaceEvicted`.
    pub fn evict_place(&mut self, place: Sigil) -> Result<(), WorldError> {
        let plan = self.proj.plan_place_evict(place)?;
        for (actor, rite) in plan.rites {
            self.push(TraceEvent::new(
                self.tick,
                TraceBody::RiteEnded {
                    actor,
                    rite,
                    status: RiteEnd::Evicted,
                },
            ));
        }
        self.proj.drop_loci(&plan.drop)?;
        self.proj.rebuild_space_ix();
        self.push(TraceEvent::new(
            self.tick,
            TraceBody::PlaceEvicted { place },
        ));
        Ok(())
    }

    pub(crate) fn from_parts(proj: Projection, tick: Tick) -> Self {
        Self {
            proj,
            events: Vec::new(),
            tick,
        }
    }

    pub(crate) fn into_parts(self) -> (Projection, Vec<TraceEvent>) {
        (self.proj, self.events)
    }
}
