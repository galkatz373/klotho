//! Read-only island capture and deterministic proposal replay for Distaff/CI.
use std::sync::Arc;

use klotho_commit::Proposal;
use klotho_core::{
    AabbMm, BlobId, Epoch, Hash, IVec3, PoseMm, ShapeKind, Sigil, Support, Tick, Vel3,
};
use klotho_world::WorldSnapshot;

use crate::{SolveTimings, solve_island};

/// Frozen input and exact output of one pure island solve. The snapshot owns Canon
/// and Projection; no mutable solver cache or renderer state is captured.
#[derive(Clone, Debug)]
pub struct IslandCapture {
    snapshot: Arc<WorldSnapshot>,
    canon_hash: Hash,
    epoch: Epoch,
    tick: Tick,
    island: u16,
    proposal: Proposal,
    timings: SolveTimings,
}

impl IslandCapture {
    /// Frozen Canon identity.
    #[must_use]
    pub const fn canon_hash(&self) -> Hash {
        self.canon_hash
    }
    /// Frozen Canon epoch.
    #[must_use]
    pub const fn epoch(&self) -> Epoch {
        self.epoch
    }
    /// Authoritative boundary used to solve.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }
    /// This-tick partition identity.
    #[must_use]
    pub const fn island(&self) -> u16 {
        self.island
    }
    /// Captured proposal payload. It may be inspected, never applied by this API.
    #[must_use]
    pub const fn proposal(&self) -> &Proposal {
        &self.proposal
    }
    /// Disposable per-stage timing and bounded workload counts for this solve.
    #[must_use]
    pub const fn timings(&self) -> SolveTimings {
        self.timings
    }
    /// Proposed versus admitted data for drawing a disposable overlay.
    #[must_use]
    pub fn bodies(&self) -> Vec<BodyOverlay> {
        let Proposal::PhysIsland { bodies, .. } = &self.proposal else {
            return Vec::new();
        };
        let view = self.snapshot.view();
        bodies
            .iter()
            .filter_map(|b| {
                Some(BodyOverlay {
                    sigil: b.mover,
                    hull: view.hull_id(b.mover)?,
                    local_hull: view.hull(b.mover)?,
                    shape: view.body_physics(b.mover).shape,
                    admitted_pose: view.pose(b.mover)?,
                    proposed_pose: b.pose,
                    proposed_velocity: b.vel,
                    admitted_support: view.support(b.mover),
                    proposed_support: b.support,
                    sleep_ticks: view.island(b.mover)?.1,
                    desired_root: view
                        .at_tick(self.tick)
                        .character_drive(b.mover)
                        .map(|d| d.root),
                    resolved_displacement: b
                        .pose
                        .translation()
                        .wrapping_sub(view.pose(b.mover)?.translation()),
                    step_mm: view.character_physics(b.mover).map(|p| p.step_mm),
                    slope_min_y: view.character_physics(b.mover).map(|p| p.slope_min_y),
                })
            })
            .collect()
    }

    /// Semantic weapon volumes in the active WAIT, in proposal order.
    #[must_use]
    pub fn sweeps(&self) -> Vec<SweepOverlay> {
        let Proposal::PhysIsland {
            motion_contacts, ..
        } = &self.proposal
        else {
            return Vec::new();
        };
        motion_contacts
            .iter()
            .map(|claim| {
                let channel = &claim.track.sweeps[usize::from(claim.sweep)];
                let i = usize::from(claim.boundary);
                SweepOverlay {
                    actor: claim.actor,
                    target: claim.target,
                    channel: channel.name.clone(),
                    endpoints: [channel.samples[i], channel.samples[i + 1]],
                    radius_mm: channel.radius_mm,
                    wait_boundary: claim.boundary,
                }
            })
            .collect()
    }

    /// Canon threshold and proposed impulse for each participating constraint.
    #[must_use]
    pub fn constraints(&self) -> Vec<ConstraintOverlay> {
        let Proposal::PhysIsland {
            constraints,
            breaks,
            ..
        } = &self.proposal
        else {
            return Vec::new();
        };
        let view = self.snapshot.view();
        constraints
            .iter()
            .filter_map(|claim| {
                let canon = view.constraint(claim.constraint)?;
                Some(ConstraintOverlay {
                    constraint: claim.constraint,
                    a: canon.a,
                    b: canon.b,
                    impulse: claim.impulse,
                    threshold: canon.break_impulse,
                    break_proposed: breaks.iter().any(|b| b.constraint == claim.constraint),
                    broken: view
                        .constraint_state(claim.constraint)
                        .is_some_and(|s| s.broken),
                })
            })
            .collect()
    }
}

/// One read-only overlay row. `admitted_pose` is the captured pre-solve pose;
/// the live post-admission pose can be inspected from the next snapshot.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BodyOverlay {
    /// Canon locus.
    pub sigil: Sigil,
    /// Canon hull identity.
    pub hull: BlobId,
    /// Local collision shape bounds.
    pub local_hull: AabbMm,
    /// Canon collision shape kind.
    pub shape: ShapeKind,
    /// Snapshot pose before the proposal.
    pub admitted_pose: PoseMm,
    /// Proposed resolved pose.
    pub proposed_pose: PoseMm,
    /// Proposed authoritative velocity.
    pub proposed_velocity: Vel3,
    /// Existing contact support.
    pub admitted_support: Option<Support>,
    /// Proposed contact support.
    pub proposed_support: Option<Support>,
    /// Existing sleep counter.
    pub sleep_ticks: u16,
    /// Canon root desire at this authoritative boundary, for driven actors.
    pub desired_root: Option<IVec3>,
    /// Proposed displacement relative to the captured Projection.
    pub resolved_displacement: IVec3,
    /// Canon step envelope for characters.
    pub step_mm: Option<i32>,
    /// Canon minimum upward slope normal, scaled by 32767.
    pub slope_min_y: Option<i16>,
}

/// Read-only motion-contact capsule at an authoritative interval.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SweepOverlay {
    /// Driven actor.
    pub actor: Sigil,
    /// Canonical target.
    pub target: Sigil,
    /// Semantic channel.
    pub channel: String,
    /// Root-local capsule endpoints at both boundaries.
    pub endpoints: [[IVec3; 2]; 2],
    /// Capsule radius in millimetres.
    pub radius_mm: i32,
    /// Active WAIT boundary.
    pub wait_boundary: u16,
}

/// Read-only constraint force and break policy.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ConstraintOverlay {
    /// Canon constraint identity.
    pub constraint: Sigil,
    /// First endpoint.
    pub a: Sigil,
    /// Second endpoint.
    pub b: Sigil,
    /// Proposed quantized impulse.
    pub impulse: i32,
    /// Canon break threshold; zero means unbreakable.
    pub threshold: i32,
    /// Whether this proposal claims the semantic break.
    pub break_proposed: bool,
    /// Previously admitted break state.
    pub broken: bool,
}

/// Why an island capture cannot be replayed exactly.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ReplayMismatch {
    /// Snapshot identity or epoch was altered.
    Identity,
    /// The island no longer yields one proposal.
    MissingProposal,
    /// Pure solver output differs from the captured payload.
    Proposal,
}

/// Capture a single island from a published, already partitioned snapshot.
/// The proposer samples the next authoritative boundary as runtime jobs do.
#[must_use]
pub fn capture_island(snapshot: Arc<WorldSnapshot>, island: u16) -> Option<IslandCapture> {
    let tick = snapshot.tick.saturating_add(1);
    let view = snapshot.view().at_tick(tick);
    let result = solve_island(island, &view);
    let mut solved = result.proposals;
    if solved.len() != 1 {
        return None;
    }
    Some(IslandCapture {
        canon_hash: snapshot.canon_hash,
        epoch: snapshot.epoch,
        tick,
        island,
        proposal: solved.remove(0),
        timings: result.timings,
        snapshot,
    })
}

/// Recompute the proposal from frozen Canon, Projection, epoch and tick.
/// Kernel admission remains an explicit step by the caller.
pub fn replay_capture(capture: &IslandCapture) -> Result<(), ReplayMismatch> {
    if capture.canon_hash != capture.snapshot.canon_hash
        || capture.epoch != capture.snapshot.epoch
        || capture.tick != capture.snapshot.tick.saturating_add(1)
    {
        return Err(ReplayMismatch::Identity);
    }
    let view = capture.snapshot.view().at_tick(capture.tick);
    let mut solved = solve_island(capture.island, &view).proposals;
    if solved.len() != 1 {
        return Err(ReplayMismatch::MissingProposal);
    }
    if solved.remove(0) != capture.proposal {
        return Err(ReplayMismatch::Proposal);
    }
    Ok(())
}
