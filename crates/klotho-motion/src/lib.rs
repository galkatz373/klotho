//! Verb→clip + root-motion proposer (K22). Not motion matching.
//!
//! A cooked [`ClipSet`] maps `(Verb, grounded)` to one looping or one-shot
//! clip. Selection is deterministic. Root motion is millimetre deltas applied
//! as `Proposal::MotionDelta`; the kernel admits against hull witnesses.
//! Gameplay-critical timing lives in Rite `WAIT` + Laws, not clip notifies.
//!
//! Clip time is derived from `WorldView::tick` (no hidden integrator state).
//! Empty clip joints are identity so Hearth hashes stay root-only.
//!
//! Actor locomotion is Motion's job. `klotho-space` skips `LocusKind::Actor`
//! so root motion and island integration cannot dual-truth the same body.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use klotho_anim::{Clip, ClipSet, WALK_MM_PER_TICK};
pub use klotho_core::rotate_xz;

use klotho_commit::{AdmitBuf, IslandProposer, Proposal, SyncProposer};
use klotho_core::{BlobId, HullWitness, IVec3, LocusKind, Mm, PoseMm, Sigil, Tick, Vel3, VelFx};
use klotho_ir::Verb;
use klotho_world::{WorldView, world_aabb};

/// Verb→clip proposer. Holds only the cooked table (a cache of F, not tick state).
#[derive(Clone, Debug)]
pub struct Motion {
    clips: ClipSet,
}

impl Motion {
    /// Hearth biped table (T-pose idle, +Z walk).
    #[must_use]
    pub fn hearth() -> Self {
        Self {
            clips: ClipSet::hearth(),
        }
    }

    /// Proposer over an already-cooked table.
    #[must_use]
    pub fn with_clips(clips: ClipSet) -> Self {
        Self { clips }
    }

    /// Cooked table in use.
    #[must_use]
    pub fn clips(&self) -> &ClipSet {
        &self.clips
    }
}

impl Default for Motion {
    fn default() -> Self {
        Self::hearth()
    }
}

impl SyncProposer for Motion {
    fn name(&self) -> &'static str {
        "motion"
    }

    fn propose(&mut self, view: &WorldView, _dt: Tick, out: &mut AdmitBuf) {
        for s in view.loci() {
            if let Some(delta) = propose_one(&self.clips, view, s) {
                out.push(delta);
            }
        }
    }
}

impl IslandProposer for Motion {
    fn name(&self) -> &'static str {
        "motion"
    }

    fn propose_island(&self, island: u16, view: &WorldView, out: &mut AdmitBuf) {
        for s in view.loci() {
            if view.island(s).map(|(id, _)| id) != Some(island) {
                continue;
            }
            if let Some(delta) = propose_one(&self.clips, view, s) {
                out.push(delta);
            }
        }
    }
}

fn propose_one(clips: &ClipSet, view: &WorldView, s: Sigil) -> Option<Proposal> {
    if s.kind() != Some(LocusKind::Actor) {
        return None;
    }
    if view.attach_parent(s).is_some() {
        return None;
    }
    let sleep = view.island(s).map(|(_, t)| t).unwrap_or(0);
    if sleep > 0 {
        return None;
    }
    let pose = view.pose(s)?;
    let local = view.hull(s)?;
    let (vel, yaw_rate) = view.vel(s).unwrap_or((Vel3::ZERO, 0));
    let grounded = view.support(s).is_some() || pose.y.0 <= 0;
    let verb = if vel.x != VelFx::ZERO || vel.y != VelFx::ZERO || vel.z != VelFx::ZERO {
        Verb::Move
    } else {
        Verb::Look
    };
    let clip = clips.lookup(verb, grounded)?;
    let root = rotate_xz(clip.sample(view.tick()), pose.yaw);
    if root == IVec3::ZERO {
        return None;
    }
    let next = PoseMm {
        x: pose.x.wrapping_add(Mm(root.x)),
        y: pose.y.wrapping_add(Mm(root.y)),
        z: pose.z.wrapping_add(Mm(root.z)),
        yaw: pose.yaw,
        pitch: pose.pitch,
        roll: pose.roll,
    };
    let from = world_aabb(local, pose.translation());
    let to = world_aabb(local, next.translation());
    let swept = from.swept_union(to);
    let hint = hits_closed(view, s, swept);
    let (island, _) = view.island(s).unwrap_or((0, 0));
    let hull = view.hull_id(s).unwrap_or(BlobId::ZERO);
    Some(Proposal::MotionDelta {
        mover: s,
        pose: next,
        // Vel is the Move request (analog/tests). Space skips Actors, so it
        // is not also integrated. Zeroing it here would drop stick on the
        // next tick.
        vel,
        yaw_rate,
        island,
        sleep_ticks: 0,
        clip: clip.id,
        root,
        hull,
        witness: HullWitness::new(s, next, hint),
    })
}

fn hits_closed(view: &WorldView, mover: Sigil, swept: klotho_core::AabbMm) -> bool {
    for o in view.space_candidates(swept, true) {
        if o == mover {
            continue;
        }
        if let Some(h) = view.posed_hull(o) {
            if h.intersects(swept) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::{CommitKernel, Proposal};
    use klotho_core::{
        AabbMm, BlobId, Budget, Hash, HullWitness, IVec3, LocusKind, Mm, PlayerId, PoseMm,
        RejectReason, Sigil, Tick, Vel3, VelFx, YawMd,
    };
    use klotho_ir::{CanonDiff, Rel, from_ron};
    use klotho_world::World;

    use super::*;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn hull_id(n: u8) -> BlobId {
        let mut b = [0u8; 32];
        b[0] = n;
        BlobId::from_bytes(b)
    }

    fn box_xz(hx: i32, hy: i32, hz: i32) -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -hx,
                y: 0,
                z: -hz,
            },
            IVec3 {
                x: hx,
                y: hy,
                z: hz,
            },
        )
    }

    fn kernel() -> (CommitKernel, Sigil, Sigil) {
        let src = r#"[
            AddAffordance(Affordance(id: "Opaque", requires: [], grants: [], conflicts: [])),
            AddLaw(Law(
                id: "never_clip_closed",
                when: Or(SourceIs(Phys), Or(SourceIs(Space), SourceIs(Motion))),
                body: Pred(must: Not(SweptHitsOpaqueClosed), ought: None),
            )),
        ]"#;
        let d: Vec<CanonDiff> = from_ron(src).unwrap();
        let canon = cook_diffs(&d).unwrap();
        let opaque = canon.affordance_id("Opaque").unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let player = actor(1);
        let door = relic(2);
        k.bind_player(PlayerId(0), player);
        {
            let mut w = k.world_mut();
            w.insert_locus(player, LocusKind::Actor).unwrap();
            w.insert_locus(door, LocusKind::Relic).unwrap();
            w.set_hull(player, box_xz(200, 1800, 200), hull_id(1))
                .unwrap();
            w.set_hull(door, box_xz(400, 2000, 50), hull_id(2)).unwrap();
            w.set_pose(player, PoseMm::new(Mm(0), Mm(0), Mm(1400), YawMd(0)))
                .unwrap();
            w.set_pose(door, PoseMm::new(Mm(0), Mm(0), Mm(1850), YawMd(0)))
                .unwrap();
            w.set_affordance(door, opaque, true).unwrap();
            w.add_rel(door, Rel::LockedBy, door).unwrap();
            w.set_island(door, 1, 12).unwrap();
            w.set_vel(
                player,
                Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(500)),
                0,
            )
            .unwrap();
        }
        (k, player, door)
    }

    #[test]
    fn tpose_idle_does_not_move() {
        let (mut k, player, _) = kernel();
        k.world_mut().set_vel(player, Vel3::ZERO, 0).unwrap();
        let mut motion = Motion::hearth();
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut motion]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(player).unwrap().z, Mm(1400));
    }

    #[test]
    fn walk_clip_applies_root_along_yaw() {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let player = actor(1);
        {
            let mut w = k.world_mut();
            w.insert_locus(player, LocusKind::Actor).unwrap();
            w.set_hull(player, box_xz(200, 1800, 200), hull_id(1))
                .unwrap();
            w.set_pose(
                player,
                PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(YawMd::QUARTER_TURN)),
            )
            .unwrap();
            w.set_vel(player, Vel3::new(VelFx::ONE, VelFx::ZERO, VelFx::ZERO), 0)
                .unwrap();
        }
        let mut motion = Motion::hearth();
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut motion]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        let p = k.world().view().pose(player).unwrap();
        assert_eq!(p.x, Mm(WALK_MM_PER_TICK));
        assert_eq!(p.z, Mm(0));
        assert_eq!(
            k.world().view().vel(player).unwrap(),
            (Vel3::new(VelFx::ONE, VelFx::ZERO, VelFx::ZERO), 0)
        );
    }

    #[test]
    fn idle_locked_door_blocks_motion_delta() {
        let (mut k, player, door) = kernel();
        assert!(k.world().view().opaque_closed(door));
        let vel_before = k.world().view().vel(player).unwrap();
        let mut motion = Motion::with_clips(ClipSet::walk_mm(500));
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut motion]).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(_, r)| matches!(r, RejectReason::Law(_) | RejectReason::WitnessMismatch)),
            "{d:?}"
        );
        assert_eq!(k.world().view().vel(player).unwrap(), vel_before);
        assert_eq!(k.world().view().pose(player).unwrap().z, Mm(1400));
    }

    #[test]
    fn unlocked_door_admits_same_root() {
        let (mut k, player, door) = kernel();
        k.world_mut().del_rel(door, Rel::LockedBy, door).unwrap();
        assert!(!k.world().view().opaque_closed(door));
        let mut motion = Motion::with_clips(ClipSet::walk_mm(500));
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut motion]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(player).unwrap().z, Mm(1900));
    }

    #[test]
    fn wrong_hull_is_wrong_hull() {
        let (mut k, player, _) = kernel();
        let pose = k.world().view().pose(player).unwrap();
        k.ingest(Proposal::MotionDelta {
            mover: player,
            pose,
            vel: Vel3::ZERO,
            yaw_rate: 0,
            island: 0,
            sleep_ticks: 0,
            clip: 0,
            root: IVec3::ZERO,
            hull: hull_id(99),
            witness: HullWitness::new(player, pose, false),
        });
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects.iter().any(|(_, r)| *r == RejectReason::WrongHull),
            "{d:?}"
        );
    }

    #[test]
    fn relic_is_not_clip_driven() {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let barrel = relic(3);
        {
            let mut w = k.world_mut();
            w.insert_locus(barrel, LocusKind::Relic).unwrap();
            w.set_hull(barrel, box_xz(300, 900, 300), hull_id(3))
                .unwrap();
            w.set_pose(barrel, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            w.set_vel(
                barrel,
                Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20)),
                0,
            )
            .unwrap();
        }
        let mut motion = Motion::hearth();
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut motion]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(barrel).unwrap().z, Mm(0));
    }
}
