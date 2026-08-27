//! Stateless 2.5D space proposer (K22). Not a physics engine.
//!
//! Integrates awake island velocities and emits `SpaceDelta` for **non-Actor**
//! loci (doors, relics, projectiles). Actor walk is `klotho-motion` root
//! clips — dual-truth is forbidden. The kernel derives swept (K24) and
//! queries `space_ix` (K23). No friction, stacking, joints, or hidden
//! integrator fields.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod overlap;

pub use overlap::{CapsuleMm, aabb_overlaps, capsule_overlaps_aabb, swept_aabb};

use klotho_commit::{AdmitBuf, Proposal, SyncProposer};
use klotho_core::{BlobId, HullWitness, LocusKind, PoseMm, Sigil, Tick, Vel3};
use klotho_world::{WorldView, world_aabb};

/// Walk speed used when tests seed a whole-mm vel. Not a physics constant.
pub const WALK_MM_PER_TICK: i32 = 20;

/// Zero-sized proposer. All inputs come from `&WorldView` (K22).
#[derive(Copy, Clone, Debug, Default)]
pub struct Space;

impl Space {
    /// Construct. There is no cached island graph.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SyncProposer for Space {
    fn name(&self) -> &'static str {
        "space"
    }

    fn propose(&mut self, view: &WorldView, _dt: Tick, out: &mut AdmitBuf) {
        for s in view.loci() {
            if let Some(delta) = propose_one(view, s) {
                out.push(delta);
            }
        }
    }
}

fn propose_one(view: &WorldView, s: Sigil) -> Option<Proposal> {
    if s.kind() == Some(LocusKind::Actor) {
        // Player / NPC walk is Motion (root clip). Dual-truth is forbidden.
        return None;
    }
    let sleep = view.island(s).map(|(_, t)| t).unwrap_or(0);
    if sleep > 0 {
        return None;
    }
    let pose = view.pose(s)?;
    let local = view.hull(s)?;
    let (vel, yaw_rate) = view.vel(s).unwrap_or((Vel3::ZERO, 0));
    if vel == Vel3::ZERO && yaw_rate == 0 {
        return None;
    }
    let next = PoseMm {
        x: pose.x.displace(vel.x),
        y: pose.y.displace(vel.y),
        z: pose.z.displace(vel.z),
        yaw: pose.yaw,
        pitch: pose.pitch,
        roll: pose.roll,
    };
    let from = world_aabb(local, pose.translation());
    let to = world_aabb(local, next.translation());
    let swept = swept_aabb(from, to);
    let hint = hits_closed(view, s, swept);
    let (island, _) = view.island(s).unwrap_or((0, 0));
    let hull = view.hull_id(s).unwrap_or(BlobId::ZERO);
    Some(Proposal::SpaceDelta {
        mover: s,
        pose: next,
        vel,
        yaw_rate,
        island,
        sleep_ticks: 0,
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
            if aabb_overlaps(swept, h) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use klotho_canon::cook_diffs;
    use klotho_commit::{CommitKernel, Proposal};
    use klotho_core::{
        AabbMm, BlobId, Budget, Hash, HullWitness, IVec3, LocusKind, Mm, PlayerId, PoseMm,
        RejectReason, Sigil, Tick, Vel3, VelFx, YawMd,
    };
    use klotho_ir::{CanonDiff, Rel, from_ron};
    use klotho_world::World;

    use super::*;

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
                when: Or(SourceIs(Space), SourceIs(Motion)),
                body: Pred(must: Not(SweptHitsOpaqueClosed), ought: None),
            )),
        ]"#;
        let d: Vec<CanonDiff> = from_ron(src).unwrap();
        let canon = cook_diffs(&d).unwrap();
        let opaque = canon.affordance_id("Opaque").unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let player = relic(1);
        let door = relic(2);
        k.bind_player(PlayerId(0), player);
        {
            let mut w = k.world_mut();
            w.insert_locus(player, LocusKind::Relic).unwrap();
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
            w.set_island(door, 1, 12).unwrap(); // sleeper still blocks
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
    fn space_is_zst() {
        assert_eq!(core::mem::size_of::<Space>(), 0);
    }

    #[test]
    fn actor_vel_is_not_space_integrated() {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let player = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        {
            let mut w = k.world_mut();
            w.insert_locus(player, LocusKind::Actor).unwrap();
            w.set_hull(player, box_xz(200, 1800, 200), hull_id(1))
                .unwrap();
            w.set_pose(player, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            w.set_vel(
                player,
                Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20)),
                0,
            )
            .unwrap();
        }
        let mut space = Space;
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut space]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(player).unwrap().z, Mm(0));
    }

    #[test]
    fn space_delta_keeps_pitch_roll() {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let s = relic(1);
        {
            let mut w = k.world_mut();
            w.insert_locus(s, LocusKind::Relic).unwrap();
            w.set_hull(s, box_xz(100, 1800, 100), hull_id(1)).unwrap();
            let mut pose = PoseMm::new(Mm(0), Mm(50), Mm(0), YawMd(0));
            pose.pitch = YawMd(1_000);
            pose.roll = YawMd(2_000);
            w.set_pose(s, pose).unwrap();
            w.set_vel(
                s,
                Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20)),
                0,
            )
            .unwrap();
        }
        let mut space = Space;
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut space]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        let got = k.world().view().pose(s).unwrap();
        assert_eq!(got.z, Mm(20));
        assert_eq!(got.y, Mm(50));
        assert_eq!(got.pitch, YawMd(1_000));
        assert_eq!(got.roll, YawMd(2_000));
    }

    #[test]
    fn idle_locked_door_blocks() {
        let (mut k, player, door) = kernel();
        assert!(k.world().view().opaque_closed(door));
        let vel_before = k.world().view().vel(player).unwrap();
        let mut space = Space;
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut space]).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(_, r)| matches!(r, RejectReason::Law(_) | RejectReason::WitnessMismatch)),
            "{d:?}"
        );
        assert_eq!(k.world().view().vel(player).unwrap(), vel_before);
        assert_eq!(
            k.world().view().pose(player).unwrap().z,
            Mm(1400),
            "rejected delta must not move"
        );
    }

    #[test]
    fn unlocked_door_admits_same_sweep() {
        let (mut k, player, door) = kernel();
        k.world_mut().del_rel(door, Rel::LockedBy, door).unwrap();
        assert!(!k.world().view().opaque_closed(door));
        let mut space = Space;
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut space]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(player).unwrap().z, Mm(1900));
    }

    #[test]
    fn wrong_hull_is_wrong_hull() {
        let (mut k, player, _) = kernel();
        let pose = k.world().view().pose(player).unwrap();
        k.ingest(Proposal::SpaceDelta {
            mover: player,
            pose,
            vel: Vel3::ZERO,
            yaw_rate: 0,
            island: 0,
            sleep_ticks: 0,
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
    fn awake64_is_under_four_ms() {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        for i in 0..64u128 {
            let s = relic(i + 10);
            let mut w = k.world_mut();
            w.insert_locus(s, LocusKind::Relic).unwrap();
            w.set_hull(s, box_xz(100, 1800, 100), hull_id((i as u8) | 1))
                .unwrap();
            w.set_pose(s, PoseMm::new(Mm(i as i32 * 400), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            w.set_vel(
                s,
                Vel3::new(
                    VelFx::from_mm_per_tick(WALK_MM_PER_TICK),
                    VelFx::ZERO,
                    VelFx::ZERO,
                ),
                0,
            )
            .unwrap();
        }
        let mut space = Space;
        let t0 = Instant::now();
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut space]).unwrap();
        let us = t0.elapsed().as_micros();
        assert!(d.rejects.is_empty(), "{d:?}");
        // Warn in local debug so a slow host does not flake; CI/release fails.
        klotho_debug::BudgetMode::from_env().enforce(us);
    }
}
