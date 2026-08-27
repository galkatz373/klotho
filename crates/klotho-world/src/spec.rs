//! Speculative projection + event list (K21). Drop = rollback.

use klotho_canon::RiteId;
use klotho_core::{AabbMm, BlobId, PoseMm, ResourceId, Sigil, Tick, Vel3};
use klotho_ir::Rel;
use klotho_trace::TraceEvent;

use crate::error::WorldError;
use crate::proj::{Projection, RiteMachine};
use crate::view::WorldView;

/// Speculative overlay: CoW-cloned projection plus events not yet on Trace.
#[derive(Clone, Debug)]
pub struct SpecDelta {
    proj: Projection,
    events: Vec<TraceEvent>,
    tick: Tick,
}

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

    /// Write island/sleep on the spec.
    pub fn set_island(&mut self, s: Sigil, island: u16, sleep: u16) -> Result<(), WorldError> {
        self.proj.set_island(s, island, sleep)
    }

    /// Write a quantity without a Trace event (tests / Conserve pre-read).
    pub fn set_qty(&mut self, s: Sigil, r: ResourceId, v: i32) -> Result<(), WorldError> {
        self.proj.set_qty(s, r, v)
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
