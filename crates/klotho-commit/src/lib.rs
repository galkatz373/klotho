//! CommitKernel: the only type that mutates committed Projection + Trace (K21).
//!
//! Each proposal is one transaction. A same-tick rite burst is one transaction.
//! `WAIT` commits and yields. Laws run on the would-be post-state; failure
//! discards the speculative delta.
//!
//! Enables `klotho-world/mutate`. Does not depend on space/motion/mind crates.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod admit;
mod kernel;
mod laws;
mod proposal;
mod rite;
mod swept;

pub use admit::{AdmitBuf, SyncProposer};
pub use kernel::CommitKernel;
pub use klotho_core::{KernelFault, RejectReason};
pub use klotho_trace::TraceDelta;
pub use proposal::Proposal;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{
        AabbMm, BlobId, Budget, Hash, HullWitness, IVec3, LocusKind, Mm, PlayerId, PoseMm,
        ResourceId, Sigil, Tick, Vel3, YawMd,
    };
    use klotho_ir::{
        Agency, Analog, CanonDiff, Channel, IntentTarget, MindIntent, PlayerIntent, Verb, from_ron,
    };
    use klotho_trace::{ISLAND_SNAP_PERIOD_TICKS, TraceBody};

    use super::*;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn cook(src: &str) -> klotho_canon::Canon {
        let d: Vec<CanonDiff> = from_ron(src).unwrap();
        cook_diffs(&d).unwrap()
    }

    fn kernel_with(src: &str, stamina: i32) -> (CommitKernel, Sigil, ResourceId) {
        let canon = cook(src);
        let stamina_id = canon.resource_id("stamina").unwrap_or(ResourceId(0));
        let mut k = CommitKernel::new(klotho_world::World::new(Arc::new(canon), Hash::ZERO));
        let s = actor(1);
        k.bind_player(PlayerId(0), s);
        k.world_mut().insert_locus(s, LocusKind::Actor).unwrap();
        if stamina != 0 {
            k.world_mut().set_qty(s, stamina_id, stamina).unwrap();
        }
        (k, s, stamina_id)
    }

    fn player_use() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(0),
            at: Tick(0),
            verb: Verb::Use,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: Agency::none(),
        }
    }

    const SPEND_LAW: &str = r#"[
        AddLaw(Law(id: "need.stamina", when: EqVerb(Use), body: Pred(
            must: Qty(Self, "stamina", Ge, 1), ought: None))),
        AddRite(RiteGraph(id: "spend", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
            Spend("stamina", 10, 2),
            Complete(Success),
            Complete(Fail),
        ])),
    ]"#;

    #[test]
    fn spend_then_law_fail_leaves_qty_unchanged() {
        let (mut k, s, stamina) = kernel_with(SPEND_LAW, 10);
        k.ingest(Proposal::Player(player_use()));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(_, r)| matches!(r, RejectReason::Law(_))),
            "{d:?}"
        );
        assert_eq!(k.world().view().qty(s, stamina), 10);
        assert!(d.events.is_empty());
    }

    #[test]
    fn spend_without_blocking_law_commits() {
        let src = r#"[
            AddRite(RiteGraph(id: "spend", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Spend("stamina", 10, 2),
                Complete(Success),
                Complete(Fail),
            ])),
        ]"#;
        let (mut k, s, stamina) = kernel_with(src, 10);
        k.ingest(Proposal::Player(player_use()));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().qty(s, stamina), 0);
    }

    #[test]
    fn wait_channel_rejects_mind() {
        let src = r#"[
            AddRite(RiteGraph(id: "lock", cap_steps: 8, cap_ticks: 180, entry: 0, nodes: [
                Wait(45, Some(Timing)),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, s, _) = kernel_with(src, 0);
        k.ingest(Proposal::Player(player_use()));
        let d1 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d1.rejects.is_empty(), "{d1:?}");
        assert!(k.world().view().first_rite(s).is_some());

        k.ingest(Proposal::Mind(MindIntent {
            locus: s,
            verb: Verb::Use,
            target: IntentTarget::None,
            utility: 0,
        }));
        let d2 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d2.rejects
                .iter()
                .any(|(_, r)| *r == RejectReason::UnclaimedAgency),
            "{d2:?}"
        );
    }

    #[test]
    fn player_timing_resumes_wait() {
        let src = r#"[
            AddRite(RiteGraph(id: "lock", cap_steps: 8, cap_ticks: 180, entry: 0, nodes: [
                Wait(45, Some(Timing)),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, s, _) = kernel_with(src, 0);
        k.ingest(Proposal::Player(player_use()));
        k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        let mut p = player_use();
        p.verb = Verb::Time;
        p.agency = Agency {
            claimed: vec![Channel::Timing],
            assist: Default::default(),
        };
        k.ingest(Proposal::Player(p));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().first_rite(s).is_none());
    }

    #[test]
    fn carry_does_not_start_lockpick() {
        let src = r#"[
            AddRite(RiteGraph(id: "lockpick", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Wait(45, Some(Timing)),
                Complete(Success),
            ])),
            AddRite(RiteGraph(id: "carry.pick", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                RelAdd(Target, WieldedBy, Self),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, s, _) = kernel_with(src, 0);
        let barrel = actor(2);
        k.world_mut()
            .insert_locus(barrel, LocusKind::Relic)
            .unwrap();
        let mut p = player_use();
        p.verb = Verb::Carry;
        p.target = IntentTarget::Sigil(barrel);
        k.ingest(Proposal::Player(p));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().first_rite(s).is_none());
        assert!(
            k.world()
                .view()
                .has_rel(barrel, klotho_ir::Rel::WieldedBy, s)
        );
    }

    #[test]
    fn cap_rejects_ninth_marked_locus() {
        let src = r#"[
            AddLaw(Law(id: "fire.bound", when: EqVerb(Use),
                body: Cap(mark: Qty(Self, "heat", Ge, 400), n: 1, require_rel: None))),
            AddRite(RiteGraph(id: "ignite", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Bind(Target),
                Setq(Target, "heat", 400),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, _, _) = kernel_with(src, 0);
        let a = actor(2);
        let b = actor(3);
        k.world_mut().insert_locus(a, LocusKind::Relic).unwrap();
        k.world_mut().insert_locus(b, LocusKind::Relic).unwrap();
        let mut p = player_use();
        p.target = IntentTarget::Sigil(a);
        k.ingest(Proposal::Player(p.clone()));
        let d1 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d1.rejects.is_empty(), "{d1:?}");
        p.target = IntentTarget::Sigil(b);
        k.ingest(Proposal::Player(p));
        let d2 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d2.rejects
                .iter()
                .any(|(_, r)| matches!(r, RejectReason::Law(_))),
            "{d2:?}"
        );
    }

    #[test]
    fn player_without_channel_may_resume_wait() {
        let src = r#"[
            AddRite(RiteGraph(id: "trade.offer", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                { pc: 0, op: Wait(12, Some(DialogueChoice)) },
                { pc: 1, op: Guard(AgencyClaimed(DialogueChoice), fail: 3) },
                { pc: 2, op: Complete(Success) },
                { pc: 3, op: Complete(Fail) },
            ])),
        ]"#;
        let (mut k, s, _) = kernel_with(src, 0);
        let mut talk = player_use();
        talk.verb = Verb::Talk;
        k.ingest(Proposal::Player(talk.clone()));
        k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(k.world().view().first_rite(s).is_some());
        k.ingest(Proposal::Player(talk));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().first_rite(s).is_none());
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

    fn empty_kernel() -> CommitKernel {
        CommitKernel::new(klotho_world::World::new(Arc::new(cook("[]")), Hash::ZERO))
    }

    fn plant_mover(k: &mut CommitKernel, s: Sigil, pose: PoseMm, island: u16, sleep: u16) {
        let mut w = k.world_mut();
        w.insert_locus(s, LocusKind::Relic).unwrap();
        w.set_hull(s, box_xz(100, 1800, 100), hull_id(1)).unwrap();
        w.set_pose(s, pose).unwrap();
        w.set_island(s, island, sleep).unwrap();
    }

    fn space_delta(mover: Sigil, pose: PoseMm) -> Proposal {
        Proposal::SpaceDelta {
            mover,
            pose,
            vel: Vel3::ZERO,
            yaw_rate: 0,
            island: 0,
            sleep_ticks: 0,
            hull: hull_id(1),
            witness: HullWitness::new(mover, pose, false),
        }
    }

    fn motion_delta(mover: Sigil, pose: PoseMm) -> Proposal {
        Proposal::MotionDelta {
            mover,
            pose,
            vel: Vel3::ZERO,
            yaw_rate: 0,
            island: 0,
            sleep_ticks: 0,
            clip: 0,
            root: IVec3::ZERO,
            hull: hull_id(1),
            witness: HullWitness::new(mover, pose, false),
        }
    }

    fn pose_committed(events: &[klotho_trace::TraceEvent]) -> bool {
        events
            .iter()
            .any(|e| matches!(e.body, TraceBody::PoseCommitted { .. }))
    }

    fn island_snaps(events: &[klotho_trace::TraceEvent]) -> Vec<&klotho_trace::IslandSnap> {
        events
            .iter()
            .filter_map(|e| match &e.body {
                TraceBody::IslandSnap(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn space_admit_writes_pose_without_pose_committed() {
        let mut k = empty_kernel();
        let s = relic(1);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0));
        let next = PoseMm::new(Mm(40), Mm(0), Mm(0), YawMd(0));
        plant_mover(&mut k, s, start, 0, 0);
        k.ingest(space_delta(s, next));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(s).unwrap(), next);
        assert!(!pose_committed(&d.events), "{d:?}");
        assert!(island_snaps(&d.events).is_empty(), "{d:?}");
    }

    #[test]
    fn motion_admit_writes_pose_without_pose_committed() {
        let mut k = empty_kernel();
        let s = relic(1);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0));
        let next = PoseMm::new(Mm(0), Mm(0), Mm(20), YawMd(0));
        plant_mover(&mut k, s, start, 0, 0);
        k.ingest(motion_delta(s, next));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(s).unwrap(), next);
        assert!(!pose_committed(&d.events), "{d:?}");
    }

    #[test]
    fn island_snap_two_hz_awake_posed_only() {
        let mut k = empty_kernel();
        let awake = relic(1);
        let sleeper = relic(2);
        let no_pose = relic(3);
        let other = relic(4);
        plant_mover(
            &mut k,
            awake,
            PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0)),
            0,
            0,
        );
        plant_mover(
            &mut k,
            sleeper,
            PoseMm::new(Mm(20), Mm(0), Mm(0), YawMd(0)),
            0,
            12,
        );
        {
            let mut w = k.world_mut();
            w.insert_locus(no_pose, LocusKind::Relic).unwrap();
            w.set_island(no_pose, 0, 0).unwrap();
        }
        plant_mover(
            &mut k,
            other,
            PoseMm::new(Mm(30), Mm(0), Mm(5), YawMd(90)),
            2,
            0,
        );

        for _ in 0..(ISLAND_SNAP_PERIOD_TICKS - 1) {
            let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
            assert!(island_snaps(&d.events).is_empty(), "{d:?}");
            assert!(!pose_committed(&d.events), "{d:?}");
        }
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert_eq!(d.tick.0 % ISLAND_SNAP_PERIOD_TICKS, 0);
        let snaps = island_snaps(&d.events);
        assert_eq!(snaps.len(), 2, "{d:?}");
        assert_eq!(snaps[0].island, 0);
        assert_eq!(snaps[0].members, vec![awake]);
        assert_eq!(
            snaps[0].poses,
            vec![PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0))]
        );
        assert_eq!(snaps[0].vels, vec![Vel3::ZERO]);
        assert_eq!(snaps[0].yaw_rates, vec![0]);
        assert_eq!(snaps[0].sleep_ticks, vec![0]);
        assert_eq!(snaps[1].island, 2);
        assert_eq!(snaps[1].members, vec![other]);
        assert_eq!(
            snaps[1].poses,
            vec![PoseMm::new(Mm(30), Mm(0), Mm(5), YawMd(90))]
        );
        assert!(
            k.world()
                .trace()
                .events()
                .iter()
                .any(|e| matches!(e.body, TraceBody::IslandSnap(_)))
        );
    }

    #[test]
    fn island_snap_skips_cadence_tick_when_all_sleep() {
        let mut k = empty_kernel();
        plant_mover(
            &mut k,
            relic(1),
            PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0)),
            0,
            12,
        );
        for _ in 0..ISLAND_SNAP_PERIOD_TICKS {
            let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
            assert!(island_snaps(&d.events).is_empty(), "{d:?}");
            assert!(!pose_committed(&d.events), "{d:?}");
        }
        assert_eq!(k.world().tick().0 % ISLAND_SNAP_PERIOD_TICKS, 0);
    }

    #[test]
    fn spawn_and_phys_req_commit() {
        let src = r#"[
            AddRite(RiteGraph(id: "ember.spawn", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Spawn("ember"),
                PhysReq(lin: IVec3(x: 3, y: 0, z: 0), ang: IVec3(x: 0, y: 1, z: 0)),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, s, stamina) = kernel_with(src, 0);
        let _ = stamina;
        k.ingest(Proposal::Player(player_use()));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        let spawned = d.events.iter().find_map(|e| match e.body {
            klotho_trace::TraceBody::Spawned {
                sigil, template, ..
            } => Some((sigil, template)),
            _ => None,
        });
        assert_eq!(spawned, Some((s, 0)), "{d:?}");
        assert_eq!(k.world().view().loci().count(), 1);
        let req = k.world().view().phys_req(s).expect("phys_req");
        assert_eq!(req.lin.x, 3);
        assert_eq!(req.ang.y, 1);
        assert_eq!(k.world().view().qty(s, ResourceId(0)), 0);
    }
}
