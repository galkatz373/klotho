//! Scalar AABB island proposer. f32 internals, one quantized [`Proposal::PhysIsland`] per island.
//!
//! Sequential positional correction; contact set rebuilt per substep. No hashed
//! warm-start. Floor / `OpaqueClosed` scenery is occupancy, not a body.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod quant;
mod solver;

use std::collections::BTreeSet;

use klotho_commit::{AdmitBuf, IslandProposer, SyncProposer};
use klotho_core::{NO_ISLAND, Tick};
use klotho_world::WorldView;

pub use quant::{METRIC_QUANT_RESIDUAL_MM, METRIC_REJECTED_NON_FINITE};
pub use solver::{SolveOut, solve_island};

/// Zero-sized proposer. All inputs come from `&WorldView` (K22).
#[derive(Copy, Clone, Debug, Default)]
pub struct Phys;

impl Phys {
    /// Construct. There is no cached island graph.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SyncProposer for Phys {
    fn name(&self) -> &'static str {
        "phys"
    }

    fn propose(&mut self, view: &WorldView, _dt: Tick, out: &mut AdmitBuf) {
        let mut islands = BTreeSet::new();
        for s in view.loci() {
            if let Some((id, _)) = view.island(s) {
                if id != NO_ISLAND {
                    islands.insert(id);
                }
            }
        }
        for id in islands {
            let solved = solve_island(id, view);
            // No I/O in the proposer: discards stay pure data on SolveOut.
            // In debug builds a non-finite body trips loudly instead of
            // hiding as a sleeping body.
            debug_assert!(
                solved.rejected_non_finite.is_empty(),
                "{}: discarded solver bodies",
                METRIC_REJECTED_NON_FINITE
            );
            for p in solved.proposals {
                out.push(p);
            }
        }
    }
}

impl IslandProposer for Phys {
    fn name(&self) -> &'static str {
        "phys"
    }

    fn propose_island(&self, island: u16, view: &WorldView, out: &mut AdmitBuf) {
        let solved = solve_island(island, view);
        debug_assert!(
            solved.rejected_non_finite.is_empty(),
            "{}: discarded solver bodies",
            METRIC_REJECTED_NON_FINITE
        );
        for p in solved.proposals {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::{BodyDelta, CommitKernel, Proposal};
    use klotho_core::{
        AabbMm, BlobId, Budget, Hash, HullWitness, IVec3, LocusKind, Mm, NO_ISLAND, PhysRequest,
        PlayerId, PoseMm, RejectReason, Sigil, Tick, Vel3, VelFx, YawMd,
    };
    use klotho_ir::{CanonDiff, Rel, from_ron};
    use klotho_motion::Motion;
    use klotho_world::World;

    use super::*;

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    fn hull_id(n: u8) -> BlobId {
        let mut b = [0u8; 32];
        b[0] = n;
        BlobId::from_bytes(b)
    }

    fn crate_hull() -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -200,
                y: 0,
                z: -200,
            },
            IVec3 {
                x: 200,
                y: 400,
                z: 200,
            },
        )
    }

    fn floor_hull() -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -50_000,
                y: -200,
                z: -50_000,
            },
            IVec3 {
                x: 50_000,
                y: 0,
                z: 50_000,
            },
        )
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

    fn empty_world() -> World {
        let d: Vec<CanonDiff> = from_ron("[]").unwrap();
        World::new(Arc::new(cook_diffs(&d).unwrap()), Hash::ZERO)
    }

    fn plant_crate(k: &mut CommitKernel, s: Sigil, y: i32, sleep: u16) {
        let mut w = k.world_mut();
        w.insert_locus(s, LocusKind::Relic).unwrap();
        w.set_hull(s, crate_hull(), hull_id(1)).unwrap();
        w.set_pose(s, PoseMm::new(Mm(0), Mm(y), Mm(0), YawMd(0)))
            .unwrap();
        w.set_island(s, 99, sleep).unwrap();
    }

    fn plant_floor(k: &mut CommitKernel) -> Sigil {
        let floor = place(9);
        let mut w = k.world_mut();
        w.insert_locus(floor, LocusKind::Place).unwrap();
        w.set_hull(floor, floor_hull(), hull_id(9)).unwrap();
        w.set_pose(floor, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        w.set_island(floor, 99, 0).unwrap();
        floor
    }

    fn stacked_kernel() -> (CommitKernel, Sigil, Sigil, Sigil, Sigil) {
        let mut k = CommitKernel::new(empty_world());
        let floor = plant_floor(&mut k);
        let bottom = relic(1);
        let mid = relic(2);
        let top = relic(3);
        plant_crate(&mut k, bottom, 0, 0);
        plant_crate(&mut k, mid, 400, 12);
        plant_crate(&mut k, top, 800, 12);
        k.world_mut()
            .set_vel(
                bottom,
                Vel3::new(VelFx::from_mm_per_tick(40), VelFx::ZERO, VelFx::ZERO),
                0,
            )
            .unwrap();
        (k, floor, bottom, mid, top)
    }

    fn p99(mut samples: Vec<f32>) -> f32 {
        assert!(!samples.is_empty());
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let i = samples.len().saturating_sub(1) * 99 / 100;
        samples[i]
    }

    #[test]
    fn phys_is_zst() {
        assert_eq!(core::mem::size_of::<Phys>(), 0);
    }

    #[test]
    fn quant_residual_p99_le_1mm_max_le_4mm() {
        let mut residuals = Vec::new();
        let mut phys = Phys;
        for origin_x in [0, 20_000_000] {
            let mut k = CommitKernel::new(empty_world());
            let _floor = plant_floor(&mut k);
            let s = relic(1);
            plant_crate(&mut k, s, 800, 0);
            k.world_mut()
                .set_pose(s, PoseMm::new(Mm(origin_x), Mm(800), Mm(0), YawMd(0)))
                .unwrap();
            for _ in 0..45 {
                k.partition();
                let view = k.world().view();
                let island = view.island(s).map(|(id, _)| id).unwrap_or(0);
                residuals.extend(solve_island(island, &view).residuals_mm);
                k.step(Tick(1), Budget::HEARTH, &mut [&mut phys]).unwrap();
            }
        }
        assert!(!residuals.is_empty());
        let max = residuals.iter().copied().fold(0.0_f32, f32::max);
        let p = p99(residuals);
        assert!(
            p <= 1.0 && max <= 4.0,
            "{METRIC_QUANT_RESIDUAL_MM} p99={p} max={max}"
        );
    }

    #[test]
    fn long_run_quantized_stack_stays_bounded() {
        // Per-tick residuals alone cannot show whether repeated f32 solve →
        // integer Projection feedback walks a quantization bin. Exercise a
        // non-zero origin, where f32 precision is less forgiving in v1.
        let mut k = CommitKernel::new(empty_world());
        let _floor = plant_floor(&mut k);
        let s = relic(1);
        plant_crate(&mut k, s, 800, 0);
        k.world_mut()
            .set_pose(s, PoseMm::new(Mm(20_000), Mm(800), Mm(-20_000), YawMd(0)))
            .unwrap();
        let mut phys = Phys;
        for tick in 1..=1_500 {
            k.partition();
            k.step(Tick(tick), Budget::HEARTH, &mut [&mut phys])
                .unwrap();
        }
        let pose = k.world().view().pose(s).unwrap();
        assert!(
            (0..=4).contains(&pose.y.0),
            "long-run stack drifted vertically: {pose:?}"
        );
        assert!(
            (19_998..=20_002).contains(&pose.x.0),
            "long-run stack drifted in x: {pose:?}"
        );
        assert!(
            (-20_002..=-19_998).contains(&pose.z.0),
            "long-run stack drifted in z: {pose:?}"
        );
    }

    #[test]
    fn stacking_bump_wakes_neighbors_and_stays_stacked() {
        let (mut k, floor, bottom, mid, top) = stacked_kernel();
        let islands = k.partition();
        assert!(
            islands
                .iter()
                .any(|(_, m)| m.contains(&bottom) && m.contains(&mid) && m.contains(&top)),
            "{islands:?}"
        );
        assert_eq!(k.world().view().island(floor).unwrap().0, NO_ISLAND);

        let mut phys = Phys;
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut phys]).unwrap();
        assert!(d.rejects.is_empty(), "first bump tick rejected: {d:?}");
        assert_eq!(k.world().view().island(bottom).unwrap().1, 0);
        assert_eq!(k.world().view().island(mid).unwrap().1, 0);
        assert_eq!(k.world().view().island(top).unwrap().1, 0);

        for _ in 0..24 {
            k.partition();
            let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut phys]).unwrap();
            assert!(d.rejects.is_empty(), "{d:?}");
        }
        let pb = k.world().view().pose(bottom).unwrap();
        let pm = k.world().view().pose(mid).unwrap();
        let pt = k.world().view().pose(top).unwrap();
        assert!(
            pm.y.0 > pb.y.0 && pt.y.0 > pm.y.0,
            "y order exploded: bottom={} mid={} top={}",
            pb.y.0,
            pm.y.0,
            pt.y.0
        );
        let span_x = (pb.x.0.max(pm.x.0).max(pt.x.0)) - (pb.x.0.min(pm.x.0).min(pt.x.0));
        assert!(
            span_x <= 400,
            "stack slid apart: span_x={span_x} {pb:?} {pm:?} {pt:?}"
        );
        assert!(
            k.world().view().support(bottom).is_some()
                || k.world()
                    .view()
                    .posed_hull(bottom)
                    .is_some_and(|h| h.min.y <= 4),
            "bottom lost the floor"
        );
        let hb = k.world().view().posed_hull(bottom).unwrap();
        let hm = k.world().view().posed_hull(mid).unwrap();
        let ht = k.world().view().posed_hull(top).unwrap();
        assert!(
            hb.intersects(hm) || hm.min.y - hb.max.y <= 20,
            "mid lost bottom: {hb:?} {hm:?}"
        );
        assert!(
            hm.intersects(ht) || ht.min.y - hm.max.y <= 20,
            "top lost mid: {hm:?} {ht:?}"
        );
    }

    #[test]
    fn never_clip_closed_rejects_phys_island() {
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
            w.set_island(door, 1, 12).unwrap();
        }
        let next = PoseMm::new(Mm(0), Mm(0), Mm(1900), YawMd(0));
        k.ingest(Proposal::PhysIsland {
            epoch: k.world().epoch(),
            tick: Tick(1),
            island: 0,
            members: vec![player],
            bodies: vec![BodyDelta {
                mover: player,
                pose: next,
                vel: Vel3::ZERO,
                yaw_rate: 0,
                pitch_rate: 0,
                roll_rate: 0,
                sleep_ticks: 0,
                hull: hull_id(1),
                witness: HullWitness::new(player, next, true),
                support: None,
            }],
            contacts: Vec::new(),
            constraints: Vec::new(),
            breaks: Vec::new(),
        });
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(_, r)| matches!(r, RejectReason::WitnessMismatch | RejectReason::Law(_))),
            "{d:?}"
        );
        assert_eq!(k.world().view().pose(player).unwrap().z, Mm(1400));
    }

    #[test]
    fn rotating_a_box_changes_solver_contact() {
        let d: Vec<CanonDiff> = from_ron(
            r#"[AddAffordance(Affordance(id: "Opaque", requires: [], grants: [], conflicts: []))]"#,
        )
        .unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(cook_diffs(&d).unwrap()), Hash::ZERO));
        let opaque = k.canon().affordance_id("Opaque").unwrap();
        let _floor = plant_floor(&mut k);
        let s = relic(1);
        let wall = relic(2);
        let long = AabbMm::new(
            IVec3 {
                x: -1_000,
                y: 0,
                z: -400,
            },
            IVec3 {
                x: 1_000,
                y: 400,
                z: 400,
            },
        );
        let wall_hull = AabbMm::new(
            IVec3 {
                x: -50,
                y: 0,
                z: -400,
            },
            IVec3 {
                x: 50,
                y: 2_000,
                z: 400,
            },
        );
        {
            let mut w = k.world_mut();
            w.insert_locus(s, LocusKind::Relic).unwrap();
            w.set_hull(s, long, hull_id(1)).unwrap();
            w.set_pose(s, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO))
                .unwrap();
            w.set_island(s, 0, 0).unwrap();
            w.insert_locus(wall, LocusKind::Relic).unwrap();
            w.set_hull(wall, wall_hull, hull_id(2)).unwrap();
            w.set_pose(wall, PoseMm::new(Mm(800), Mm(0), Mm(0), YawMd::ZERO))
                .unwrap();
            w.set_affordance(wall, opaque, true).unwrap();
            w.add_rel(wall, Rel::LockedBy, wall).unwrap();
            w.set_island(wall, 1, 12).unwrap();
        }
        k.partition();
        let island = k.world().view().island(s).unwrap().0;
        let yaw0 = solve_island(island, &k.world().view());
        k.world_mut()
            .set_pose(
                s,
                PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(YawMd::QUARTER_TURN)),
            )
            .unwrap();
        k.partition();
        let island = k.world().view().island(s).unwrap().0;
        let yaw90 = solve_island(island, &k.world().view());
        let x0 = match &yaw0.proposals[0] {
            Proposal::PhysIsland { bodies, .. } => bodies[0].pose.x.0,
            _ => panic!("expected island"),
        };
        let x90 = match &yaw90.proposals[0] {
            Proposal::PhysIsland { bodies, .. } => bodies[0].pose.x.0,
            _ => panic!("expected island"),
        };
        assert!(x0 < -10, "yaw 0 must be pushed off the wall, x={x0}");
        assert!(x90.abs() < 8, "yaw 90 must not hit the wall, x={x90}");
    }

    #[test]
    fn one_solver_result_is_one_sorted_island_proposal() {
        let (mut k, _floor, bottom, mid, top) = stacked_kernel();
        k.partition();
        let island = k.world().view().island(bottom).unwrap().0;
        let solved = solve_island(island, &k.world().view());
        assert_eq!(solved.proposals.len(), 1);
        let Proposal::PhysIsland {
            epoch,
            tick,
            members,
            bodies,
            contacts,
            constraints,
            breaks,
            ..
        } = &solved.proposals[0]
        else {
            panic!("solver emitted a non-island physics grain")
        };
        assert_eq!(*epoch, k.world().epoch());
        assert_eq!(*tick, k.world().tick());
        assert_eq!(members, &vec![bottom, mid, top]);
        assert_eq!(
            bodies.iter().map(|body| body.mover).collect::<Vec<_>>(),
            vec![bottom, mid, top]
        );
        assert!(contacts.is_empty());
        assert!(constraints.is_empty());
        assert!(breaks.is_empty());
    }

    #[test]
    fn phys_req_is_consumed_on_admit() {
        let mut k = CommitKernel::new(empty_world());
        let _floor = plant_floor(&mut k);
        let s = relic(1);
        plant_crate(&mut k, s, 0, 12);
        k.world_mut()
            .set_phys_req(
                s,
                PhysRequest {
                    lin: IVec3 { x: 20, y: 0, z: 0 },
                    ang: IVec3::ZERO,
                },
            )
            .unwrap();
        let islands = k.partition();
        assert!(
            islands.iter().any(|(_, m)| m.contains(&s)),
            "phys_req sleeper is a seed: {islands:?}"
        );
        let mut phys = Phys;
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut phys]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().phys_req(s).is_none());
        let x1 = k.world().view().pose(s).unwrap().x;
        let (v1, _) = k.world().view().vel(s).unwrap();
        k.partition();
        let d2 = k.step(Tick(1), Budget::HEARTH, &mut [&mut phys]).unwrap();
        assert!(d2.rejects.is_empty(), "{d2:?}");
        assert!(k.world().view().phys_req(s).is_none());
        let (v2, _) = k.world().view().vel(s).unwrap();
        assert!(
            (v2.x.0 - v1.x.0).unsigned_abs() < VelFx::from_mm_per_tick(10).0.unsigned_abs(),
            "stale phys_req must not add Δv every tick: v1={} v2={} x1={}",
            v1.x.0,
            v2.x.0,
            x1.0
        );
        k.world_mut().set_vel(s, Vel3::ZERO, 0).unwrap();
        k.world_mut().set_island(s, 99, 12).unwrap();
        let part = k.partition();
        assert!(
            !part.iter().any(|(_, m)| m.contains(&s)),
            "cleared phys_req must not keep seeding: {part:?}"
        );
    }

    #[test]
    fn attached_actor_is_skipped_at_propose() {
        let mut k = CommitKernel::new(empty_world());
        let parent = relic(1);
        let child = actor(2);
        plant_crate(&mut k, parent, 0, 0);
        {
            let mut w = k.world_mut();
            w.insert_locus(child, LocusKind::Actor).unwrap();
            w.set_hull(child, box_xz(200, 1800, 200), hull_id(2))
                .unwrap();
            w.set_pose(child, PoseMm::new(Mm(1_000), Mm(200), Mm(0), YawMd(0)))
                .unwrap();
            w.set_island(child, 0, 0).unwrap();
            w.set_vel(
                child,
                Vel3::new(VelFx::from_mm_per_tick(20), VelFx::ZERO, VelFx::ZERO),
                0,
            )
            .unwrap();
            w.add_rel(child, Rel::AttachedTo, parent).unwrap();
        }
        k.partition();
        let mut buf = klotho_commit::AdmitBuf::new();
        let phys = Phys;
        let island = k
            .world()
            .view()
            .island(parent)
            .map(|(id, _)| id)
            .unwrap_or(0);
        phys.propose_island(island, &k.world().view(), &mut buf);
        assert!(
            buf.drain().iter().all(|p| match p {
                Proposal::PhysIsland { bodies, .. } =>
                    bodies.iter().all(|body| body.mover != child),
                _ => true,
            }),
            "attached child must not get its own BodyDelta"
        );
        let mut motion = Motion::hearth();
        let mut mbuf = klotho_commit::AdmitBuf::new();
        motion.propose(&k.world().view(), Tick(1), &mut mbuf);
        assert!(
            mbuf.drain().is_empty(),
            "pre-propose AttachedTo skips Motion"
        );
    }

    #[test]
    fn drift_vehicle_moves_on_steer() {
        let mut k = drift_slice::boot();
        let player = drift_slice::pin(&k, "player");
        let vehicle = drift_slice::pin(&k, "vehicle");
        let possess: Vec<klotho_ir::PlayerIntent> = from_ron(include_str!(
            "../../../examples/drift-slice/fixtures/golden_02_possess.ron"
        ))
        .unwrap();
        let ds = drift_slice::replay(&mut k, &possess);
        assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
        assert!(k.world().view().has_rel(player, Rel::PilotedBy, vehicle));
        let z0 = k.world().view().pose(vehicle).unwrap().z;
        let steer: Vec<klotho_ir::PlayerIntent> = from_ron(include_str!(
            "../../../examples/drift-slice/fixtures/golden_07_steer.ron"
        ))
        .unwrap();
        let ds = drift_slice::replay(&mut k, &steer);
        assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
        assert!(k.world().view().phys_req(player).is_some());
        k.partition();
        let mut phys = Phys;
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut phys]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().phys_req(player).is_none());
        let z1 = k.world().view().pose(vehicle).unwrap().z;
        assert_ne!(z1, z0, "folded driver PHYS_REQ must translate the vehicle");
    }
}
