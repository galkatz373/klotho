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
mod partition;
mod proposal;
mod rite;
mod swept;

pub use admit::{AdmitBuf, IslandProposer, SyncProposer};
pub use kernel::{CommitKernel, EpochApplyError};
pub use klotho_core::{KernelFault, RejectReason};
pub use klotho_trace::TraceDelta;
pub use partition::{Partition, partition_islands};
pub use proposal::{
    BodyDelta, ConstraintBreakClaim, ConstraintRef, ContactClaim, MAX_PHYS_ISLAND_BODIES,
    MAX_PHYS_ISLAND_BREAKS, MAX_PHYS_ISLAND_CHILDREN, MAX_PHYS_ISLAND_CONSTRAINTS,
    MAX_PHYS_ISLAND_CONTACTS, MAX_PHYS_ISLAND_MEMBERS, MAX_PHYS_ISLAND_WRITE_LOCI, Proposal,
    ResidencyOp,
};

/// PlaceSnap rows applied on a residency load (`TraceBody::PlaceLoaded.n`).
pub const METRIC_RESIDENCY_ROWS_APPLIED: &str = "klotho.residency.rows_applied";
/// Wall time of a Place load `step`, microseconds.
pub const METRIC_STREAM_HITCH_US: &str = "klotho.stream.hitch_us";
/// `now - at` for an admitted rewind Fire. 0 if same tick or rewind disabled.
pub const METRIC_REWIND_TICKS_USED: &str = "klotho.rewind.ticks_used";

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{
        AabbMm, BlobId, BodyPhysics, Budget, Epoch, Hash, HullWitness, IVec3, LocusKind, Mm,
        NO_ISLAND, PlayerId, PoseMm, QuantizedContact, ResourceId, ShapeKind, Sigil, Tick, Vel3,
        YawMd, rotate_xz,
    };
    use klotho_ir::{
        Agency, Analog, CanonDiff, Channel, IntentTarget, MindIntent, PlayerIntent, Rel, Verb,
        from_ron,
    };
    use klotho_trace::{ISLAND_SNAP_PERIOD_TICKS, ProposalKind, RiteEnd, TraceBody, TraceEvent};
    use klotho_world::{MAX_PLACE_ROWS, PlaceRow, PlaceSnap};

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

    fn body_delta(mover: Sigil, pose: PoseMm) -> BodyDelta {
        BodyDelta {
            mover,
            pose,
            vel: Vel3::ZERO,
            yaw_rate: 0,
            pitch_rate: 0,
            roll_rate: 0,
            sleep_ticks: 0,
            hull: hull_id(1),
            witness: HullWitness::new(mover, pose, false),
            support: None,
        }
    }

    fn phys_island(mover: Sigil, pose: PoseMm, members: Vec<Sigil>) -> Proposal {
        Proposal::PhysIsland {
            epoch: klotho_core::Epoch::ZERO,
            tick: Tick(1),
            island: 0,
            members,
            bodies: vec![body_delta(mover, pose)],
            contacts: Vec::new(),
            constraints: Vec::new(),
            breaks: Vec::new(),
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
    fn use_on_hittable_picks_melee() {
        let src = r#"[
            AddAffordance(Affordance(id: "Hittable", requires: [], grants: [], conflicts: [])),
            AddRite(RiteGraph(id: "fire", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Spend("stamina", 10, 2),
                Complete(Success),
                Complete(Fail),
            ])),
            AddRite(RiteGraph(id: "melee", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Wait(2, Some(Aim)),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, s, stamina) = kernel_with(src, 10);
        let dummy = actor(2);
        k.world_mut().insert_locus(dummy, LocusKind::Actor).unwrap();
        let hittable = k.canon().affordance_id("Hittable").unwrap();
        k.world_mut().set_affordance(dummy, hittable, true).unwrap();
        let mut p = player_use();
        p.target = IntentTarget::Sigil(dummy);
        k.ingest(Proposal::Player(p));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().qty(s, stamina), 10);
        assert!(k.world().view().first_rite(s).is_some());
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
        let (spawned, template) = spawned.expect("spawned");
        assert_eq!(template, 0, "{d:?}");
        assert_ne!(spawned, s);
        assert_eq!(spawned.kind(), Some(LocusKind::Relic));
        assert_eq!(k.world().view().loci().count(), 2);
        assert!(k.world().view().contains(spawned));
        let req = k.world().view().phys_req(s).expect("phys_req");
        assert_eq!(req.lin.x, 3);
        assert_eq!(req.ang.y, 1);
        assert_eq!(k.world().view().qty(s, ResourceId(0)), 0);
    }

    #[test]
    fn admit_key_is_total_and_reserves_phys_residency() {
        let s0 = relic(1);
        let s1 = relic(2);
        let phys = phys_island(s0, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)), vec![s0]);
        let space = space_delta(s1, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        let motion = motion_delta(s0, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        assert_eq!(Proposal::Player(player_use()).order_key(), 0);
        let place = Sigil::pack(LocusKind::Place, 0, 1).unwrap();
        let load = Proposal::Residency {
            place,
            op: ResidencyOp::Load,
            prefix: Hash::ZERO,
            canon_hash: Hash::ZERO,
            snap: Arc::new(PlaceSnap::new(place, Hash::ZERO, Hash::ZERO, Vec::new())),
        };
        assert_eq!(load.order_key(), 1);
        assert_eq!(phys.order_key(), 2);
        assert!(load.order_key() < phys.order_key());
        assert!(load.order_key() < space.order_key());
        assert_eq!(space.order_key(), 3);
        assert_eq!(motion.order_key(), 4);
        assert!(Proposal::Player(player_use()).admit_key(0) < load.admit_key(0));
        assert!(load.admit_key(0) < space.admit_key(0));
        assert_eq!(
            Proposal::Mind(klotho_ir::MindIntent {
                locus: s0,
                verb: Verb::Look,
                target: IntentTarget::None,
                utility: 0,
            })
            .order_key(),
            5
        );
        assert!(Proposal::Player(player_use()).admit_key(9) < phys.admit_key(0));
        assert!(phys.admit_key(0) < space.admit_key(0));
        assert!(space.admit_key(0) < motion.admit_key(0));
        let a = space_delta(s0, PoseMm::new(Mm(1), Mm(0), Mm(0), YawMd(0)));
        let mut b = space_delta(s0, PoseMm::new(Mm(2), Mm(0), Mm(0), YawMd(0)));
        if let Proposal::SpaceDelta { island, .. } = &mut b {
            *island = 1;
        }
        assert!(a.admit_key(0) < b.admit_key(0));
        assert!(a.admit_key(0) < a.admit_key(1));
    }

    #[test]
    fn space_then_motion_same_mover_is_conflict() {
        let mut k = empty_kernel();
        let s = relic(1);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0));
        plant_mover(&mut k, s, start, 0, 0);
        k.ingest(space_delta(s, PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0))));
        k.ingest(motion_delta(s, PoseMm::new(Mm(20), Mm(0), Mm(0), YawMd(0))));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects.iter().any(|(_, r)| *r == RejectReason::Conflict),
            "{d:?}"
        );
        assert_eq!(k.world().view().pose(s).unwrap().x, Mm(10));
    }

    #[test]
    fn phys_parent_nacks_attached_child_motion_and_composes_yaw_only() {
        let mut k = empty_kernel();
        let parent = relic(1);
        let child = relic(2);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0));
        plant_mover(&mut k, parent, start, 0, 0);
        plant_mover(
            &mut k,
            child,
            PoseMm::new(Mm(1_000), Mm(200), Mm(0), YawMd(0)),
            0,
            0,
        );
        k.world_mut()
            .add_rel(child, klotho_ir::Rel::AttachedTo, parent)
            .unwrap();
        assert_eq!(
            k.world().view().attach_local(child),
            Some(IVec3 {
                x: 1_000,
                y: 200,
                z: 0
            })
        );
        let mut next = PoseMm::new(Mm(10), Mm(50), Mm(0), YawMd(YawMd::QUARTER_TURN));
        next.pitch = YawMd(1_000);
        next.roll = YawMd(2_000);
        k.ingest(phys_island(parent, next, vec![parent, child]));
        k.ingest(motion_delta(
            child,
            PoseMm::new(Mm(50_020), Mm(0), Mm(0), YawMd(0)),
        ));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == klotho_trace::ProposalKind::Motion
                    && *r == RejectReason::Conflict),
            "{d:?}"
        );
        assert_eq!(k.world().view().pose(parent).unwrap(), next);
        let got = k.world().view().pose(child).unwrap();
        let offset = rotate_xz(
            IVec3 {
                x: 1_000,
                y: 200,
                z: 0,
            },
            next.yaw,
        );
        assert_eq!(got.x, Mm(next.x.0.wrapping_add(offset.x)));
        assert_eq!(got.y, Mm(next.y.0.wrapping_add(offset.y)));
        assert_eq!(got.z, Mm(next.z.0.wrapping_add(offset.z)));
        assert_eq!(got.yaw, next.yaw);
        assert_eq!(got.pitch, next.pitch);
        assert_eq!(got.roll, next.roll);
    }

    #[test]
    fn phys_island_law_reject_rolls_back_every_body_byte_for_byte() {
        let canon = cook(
            r#"[AddLaw(Law(id: "phys.permit", when: SourceIs(Phys), body: Pred(
                must: Qty(Self, "permit", Ge, 1), ought: None)))]"#,
        );
        let permit = canon.resource_id("permit").expect("permit resource");
        let mut k = CommitKernel::new(klotho_world::World::new(Arc::new(canon), Hash::ZERO));
        let a = relic(1);
        let b = relic(2);
        let start_a = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let start_b = PoseMm::new(Mm(1_000), Mm(0), Mm(0), YawMd::ZERO);
        plant_mover(&mut k, a, start_a, 0, 0);
        plant_mover(&mut k, b, start_b, 0, 0);
        k.world_mut().set_qty(a, permit, 1).unwrap();
        let mut before = k.snapshot().encode().unwrap();
        let proposal = Proposal::PhysIsland {
            epoch: klotho_core::Epoch::ZERO,
            tick: Tick(1),
            island: 0,
            members: vec![a, b],
            bodies: vec![
                body_delta(a, PoseMm::new(Mm(100), Mm(0), Mm(0), YawMd::ZERO)),
                body_delta(b, PoseMm::new(Mm(1_100), Mm(0), Mm(0), YawMd::ZERO)),
            ],
            contacts: Vec::new(),
            constraints: Vec::new(),
            breaks: Vec::new(),
        };
        k.ingest(proposal);
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, reason)| *kind == ProposalKind::Phys
                    && matches!(reason, RejectReason::Law(_))),
            "{d:?}"
        );
        assert_eq!(k.world().view().pose(a), Some(start_a));
        assert_eq!(k.world().view().pose(b), Some(start_b));
        assert!(d.events.is_empty());
        let mut after = k.snapshot().encode().unwrap();
        // Snapshot header tick is the causal clock, not Projection. Normalize
        // only that field and compare the canonical Projection bytes exactly.
        before[16..24].fill(0);
        after[16..24].fill(0);
        assert_eq!(before, after, "rejected island changed Projection bytes");
    }

    #[test]
    fn phys_island_rejects_reordered_duplicate_and_mismatched_membership() {
        let a = relic(1);
        let b = relic(2);
        let start_a = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let start_b = PoseMm::new(Mm(1_000), Mm(0), Mm(0), YawMd::ZERO);
        for malformed in 0..4 {
            let mut k = empty_kernel();
            plant_mover(&mut k, a, start_a, 0, 0);
            plant_mover(&mut k, b, start_b, 0, 0);
            let mut members = vec![a, b];
            let mut bodies = vec![
                body_delta(a, PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd::ZERO)),
                body_delta(b, PoseMm::new(Mm(1_010), Mm(0), Mm(0), YawMd::ZERO)),
            ];
            match malformed {
                0 => bodies.swap(0, 1),
                1 => members[1] = a,
                2 => {
                    members.pop();
                }
                3 => {
                    bodies.push(bodies[0]);
                }
                _ => unreachable!(),
            }
            k.ingest(Proposal::PhysIsland {
                epoch: klotho_core::Epoch::ZERO,
                tick: Tick(1),
                island: 0,
                members,
                bodies,
                contacts: Vec::new(),
                constraints: Vec::new(),
                breaks: Vec::new(),
            });
            let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
            assert_eq!(
                d.rejects,
                vec![(ProposalKind::Phys, RejectReason::WitnessMismatch)],
                "case {malformed}: {d:?}"
            );
            assert_eq!(k.world().view().pose(a), Some(start_a));
            assert_eq!(k.world().view().pose(b), Some(start_b));
        }
    }

    #[test]
    fn phys_island_caps_and_tick_are_fail_closed() {
        let a = relic(1);
        let b = relic(2);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let other = PoseMm::new(Mm(50), Mm(0), Mm(0), YawMd::ZERO);
        for stale_tick in [true, false] {
            let mut k = empty_kernel();
            plant_mover(&mut k, a, start, 0, 0);
            plant_mover(&mut k, b, other, 1, 0);
            let body = body_delta(a, PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd::ZERO));
            let claim = ContactClaim {
                a,
                b,
                shape_a: hull_id(1),
                shape_b: hull_id(1),
                kind_a: ShapeKind::OrientedBox,
                kind_b: ShapeKind::OrientedBox,
                feature: 0,
                witness: body.witness,
            };
            k.ingest(Proposal::PhysIsland {
                epoch: klotho_core::Epoch::ZERO,
                tick: if stale_tick { Tick::ZERO } else { Tick(1) },
                island: 0,
                members: vec![a],
                bodies: vec![body],
                contacts: if stale_tick {
                    Vec::new()
                } else {
                    vec![claim; MAX_PHYS_ISLAND_CONTACTS + 1]
                },
                constraints: Vec::new(),
                breaks: Vec::new(),
            });
            let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
            assert_eq!(
                d.rejects,
                vec![(
                    ProposalKind::Phys,
                    if stale_tick {
                        RejectReason::EpochMismatch
                    } else {
                        RejectReason::IslandTooLarge
                    }
                )]
            );
            assert_eq!(k.world().view().pose(a), Some(start));
        }
    }

    fn plant_closed(k: &mut CommitKernel, s: Sigil, pose: PoseMm, hull: AabbMm, id: BlobId) {
        let opaque = k.canon().affordance_id("Opaque");
        let mut w = k.world_mut();
        w.insert_locus(s, LocusKind::Relic).unwrap();
        w.set_hull(s, hull, id).unwrap();
        w.set_pose(s, pose).unwrap();
        w.set_island(s, 1, 12).unwrap();
        if let Some(a) = opaque {
            w.set_affordance(s, a, true).unwrap();
        }
        w.add_rel(s, Rel::LockedBy, s).unwrap();
    }

    #[test]
    fn phys_island_resting_contact_is_legal() {
        let mut k = opaque_kernel();
        let mover = relic(1);
        let wall = relic(2);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(100), YawMd::ZERO);
        plant_mover(&mut k, mover, start, 0, 0);
        plant_closed(
            &mut k,
            wall,
            PoseMm::new(Mm(0), Mm(0), Mm(250), YawMd::ZERO),
            box_xz(400, 2_000, 50),
            hull_id(2),
        );
        k.ingest(phys_island(mover, start, vec![mover]));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(mover), Some(start));
    }

    #[test]
    fn phys_island_thin_wall_crossing_fails_closed() {
        let mut k = opaque_kernel();
        let mover = relic(1);
        let wall = relic(2);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let end = PoseMm::new(Mm(0), Mm(0), Mm(2_000), YawMd::ZERO);
        plant_mover(&mut k, mover, start, 0, 0);
        plant_closed(
            &mut k,
            wall,
            PoseMm::new(Mm(0), Mm(0), Mm(1_000), YawMd::ZERO),
            AabbMm::new(
                IVec3 {
                    x: -10_000,
                    y: 0,
                    z: -5,
                },
                IVec3 {
                    x: 10_000,
                    y: 2_000,
                    z: 5,
                },
            ),
            hull_id(2),
        );
        k.ingest(phys_island(mover, end, vec![mover]));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert_eq!(
            d.rejects,
            vec![(ProposalKind::Phys, RejectReason::WitnessMismatch)]
        );
        assert_eq!(k.world().view().pose(mover), Some(start));
    }

    #[test]
    fn phys_island_stale_shape_and_epoch_witnesses_fail_closed() {
        let mover = relic(1);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let next = PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd::ZERO);
        for case in 0..3 {
            let mut k = empty_kernel();
            plant_mover(&mut k, mover, start, 0, 0);
            let mut body = body_delta(mover, next);
            match case {
                0 => body.witness.epoch = Epoch(1),
                1 => body.witness.shape = ShapeKind::TriangleMesh,
                2 => body.hull = hull_id(9),
                _ => unreachable!(),
            }
            k.ingest(Proposal::PhysIsland {
                epoch: Epoch::ZERO,
                tick: Tick(1),
                island: 0,
                members: vec![mover],
                bodies: vec![body],
                contacts: Vec::new(),
                constraints: Vec::new(),
                breaks: Vec::new(),
            });
            let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
            let want = if case == 0 {
                RejectReason::StaleEpoch
            } else if case == 2 {
                RejectReason::WrongHull
            } else {
                RejectReason::WitnessMismatch
            };
            assert_eq!(d.rejects, vec![(ProposalKind::Phys, want)], "case {case}");
            assert_eq!(k.world().view().pose(mover), Some(start));
        }
    }

    #[test]
    fn phys_island_shape_kind_is_bound_by_canon() {
        let mover = relic(1);
        let mut canon = cook("[]");
        assert!(canon.bind_physics(
            mover,
            BodyPhysics {
                shape: ShapeKind::Convex,
                ..BodyPhysics::default()
            },
        ));
        let mut k = CommitKernel::new(klotho_world::World::new(Arc::new(canon), Hash::ZERO));
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let next = PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd::ZERO);
        plant_mover(&mut k, mover, start, 0, 0);
        k.ingest(phys_island(mover, next, vec![mover]));
        let rejected = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert_eq!(
            rejected.rejects,
            vec![(ProposalKind::Phys, RejectReason::WrongHull)]
        );

        let mut body = body_delta(mover, next);
        body.witness.shape = ShapeKind::Convex;
        k.ingest(Proposal::PhysIsland {
            epoch: Epoch::ZERO,
            tick: Tick(2),
            island: 0,
            members: vec![mover],
            bodies: vec![body],
            contacts: Vec::new(),
            constraints: Vec::new(),
            breaks: Vec::new(),
        });
        let admitted = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(admitted.rejects.is_empty(), "{admitted:?}");
        assert_eq!(k.world().view().pose(mover), Some(next));
    }

    #[test]
    fn phys_island_contact_requires_reproduced_evidence_not_aabb() {
        let mut k = empty_kernel();
        let a = relic(1);
        let b = relic(2);
        let pa = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let pb = PoseMm::new(Mm(150), Mm(0), Mm(0), YawMd::ZERO);
        plant_mover(&mut k, a, pa, 0, 0);
        plant_mover(&mut k, b, pb, 0, 0);
        let body_a = body_delta(a, pa);
        let body_b = body_delta(b, pb);
        let aabb_only = ContactClaim {
            a,
            b,
            shape_a: hull_id(1),
            shape_b: hull_id(1),
            kind_a: ShapeKind::OrientedBox,
            kind_b: ShapeKind::OrientedBox,
            feature: 0,
            witness: HullWitness::new(a, pa, true),
        };
        k.ingest(Proposal::PhysIsland {
            epoch: Epoch::ZERO,
            tick: Tick(1),
            island: 0,
            members: vec![a, b],
            bodies: vec![body_a, body_b],
            contacts: vec![aabb_only],
            constraints: Vec::new(),
            breaks: Vec::new(),
        });
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert_eq!(
            d.rejects,
            vec![(ProposalKind::Phys, RejectReason::WitnessMismatch)]
        );
        assert_eq!(k.world().view().pose(a), Some(pa));
        assert_eq!(k.world().view().pose(b), Some(pb));
    }

    #[test]
    fn phys_island_rotated_box_changes_admitted_contact() {
        let local = AabbMm::new(
            IVec3 {
                x: -1_000,
                y: 0,
                z: -50,
            },
            IVec3 {
                x: 1_000,
                y: 100,
                z: 50,
            },
        );
        let other_local = box_xz(50, 100, 50);
        let long = relic(1);
        let stub = relic(2);
        let pa0 = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let pa90 = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(YawMd::QUARTER_TURN));
        let pb = PoseMm::new(Mm(600), Mm(0), Mm(0), YawMd::ZERO);
        let shape = klotho_geom::Shape::oriented_box(local).unwrap();
        let other = klotho_geom::Shape::oriented_box(other_local).unwrap();
        assert!(
            klotho_geom::contact(shape, pa0, other, pb)
                .unwrap()
                .is_some()
        );
        assert!(
            klotho_geom::contact(shape, pa90, other, pb)
                .unwrap()
                .is_none()
        );

        let mut k = empty_kernel();
        {
            let mut w = k.world_mut();
            w.insert_locus(long, LocusKind::Relic).unwrap();
            w.insert_locus(stub, LocusKind::Relic).unwrap();
            w.set_hull(long, local, hull_id(1)).unwrap();
            w.set_hull(stub, other_local, hull_id(1)).unwrap();
            w.set_pose(long, pa0).unwrap();
            w.set_pose(stub, pb).unwrap();
            w.set_island(long, 0, 0).unwrap();
            w.set_island(stub, 0, 0).unwrap();
        }
        let computed = klotho_geom::contact(shape, pa0, other, pb)
            .unwrap()
            .expect("yaw0 contact");
        let mut witness = HullWitness::new(long, pa0, false);
        witness.evidence = Some(computed);
        let claim = ContactClaim {
            a: long,
            b: stub,
            shape_a: hull_id(1),
            shape_b: hull_id(1),
            kind_a: ShapeKind::OrientedBox,
            kind_b: ShapeKind::OrientedBox,
            feature: computed.feature,
            witness,
        };
        k.ingest(Proposal::PhysIsland {
            epoch: Epoch::ZERO,
            tick: Tick(1),
            island: 0,
            members: vec![long, stub],
            bodies: vec![body_delta(long, pa0), body_delta(stub, pb)],
            contacts: vec![claim],
            constraints: Vec::new(),
            breaks: Vec::new(),
        });
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects.is_empty(),
            "yaw0 must admit reproduced contact: {d:?}"
        );

        let mut k90 = empty_kernel();
        {
            let mut w = k90.world_mut();
            w.insert_locus(long, LocusKind::Relic).unwrap();
            w.insert_locus(stub, LocusKind::Relic).unwrap();
            w.set_hull(long, local, hull_id(1)).unwrap();
            w.set_hull(stub, other_local, hull_id(1)).unwrap();
            w.set_pose(long, pa90).unwrap();
            w.set_pose(stub, pb).unwrap();
            w.set_island(long, 0, 0).unwrap();
            w.set_island(stub, 0, 0).unwrap();
        }
        let mut fake = HullWitness::new(long, pa90, false);
        fake.evidence = Some(QuantizedContact {
            point: IVec3 {
                x: 300,
                y: 50,
                z: 0,
            },
            normal: (32767, 0, 0),
            depth_mm: 1,
            feature: 0,
        });
        k90.ingest(Proposal::PhysIsland {
            epoch: Epoch::ZERO,
            tick: Tick(1),
            island: 0,
            members: vec![long, stub],
            bodies: vec![body_delta(long, pa90), body_delta(stub, pb)],
            contacts: vec![ContactClaim {
                a: long,
                b: stub,
                shape_a: hull_id(1),
                shape_b: hull_id(1),
                kind_a: ShapeKind::OrientedBox,
                kind_b: ShapeKind::OrientedBox,
                feature: 0,
                witness: fake,
            }],
            constraints: Vec::new(),
            breaks: Vec::new(),
        });
        let d90 = k90.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert_eq!(
            d90.rejects,
            vec![(ProposalKind::Phys, RejectReason::WitnessMismatch)]
        );
    }

    #[test]
    fn phys_island_broadphase_order_does_not_change_admission() {
        let mut k = opaque_kernel();
        let mover = relic(1);
        let w0 = relic(2);
        let w1 = relic(3);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        plant_mover(&mut k, mover, start, 0, 0);
        plant_closed(
            &mut k,
            w0,
            PoseMm::new(Mm(-8_000), Mm(0), Mm(0), YawMd::ZERO),
            box_xz(100, 2_000, 100),
            hull_id(2),
        );
        plant_closed(
            &mut k,
            w1,
            PoseMm::new(Mm(8_000), Mm(0), Mm(0), YawMd::ZERO),
            box_xz(100, 2_000, 100),
            hull_id(3),
        );
        let next = PoseMm::new(Mm(20), Mm(0), Mm(0), YawMd::ZERO);
        k.ingest(phys_island(mover, next, vec![mover]));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(mover), Some(next));
    }

    #[test]
    fn parent_spatial_nacks_attached_child_motion() {
        let mut k = empty_kernel();
        let parent = relic(1);
        let child = relic(2);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0));
        plant_mover(&mut k, parent, start, 0, 0);
        plant_mover(
            &mut k,
            child,
            PoseMm::new(Mm(50_000), Mm(0), Mm(0), YawMd(0)),
            0,
            0,
        );
        k.world_mut()
            .add_rel(child, klotho_ir::Rel::AttachedTo, parent)
            .unwrap();
        k.ingest(space_delta(
            parent,
            PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0)),
        ));
        k.ingest(motion_delta(
            child,
            PoseMm::new(Mm(50_020), Mm(0), Mm(0), YawMd(0)),
        ));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == klotho_trace::ProposalKind::Motion
                    && *r == RejectReason::Conflict),
            "{d:?}"
        );
        assert_eq!(k.world().view().pose(parent).unwrap().x, Mm(10));
        assert_eq!(k.world().view().pose(child).unwrap().x, Mm(50_000));
    }

    #[test]
    fn us_sim_zero_still_admits() {
        // us_sim is telemetry only (K14): it never changes admission.
        let mut k = empty_kernel();
        let s = relic(1);
        plant_mover(&mut k, s, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)), 0, 0);
        k.ingest(space_delta(s, PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0))));
        let mut budget = Budget::HEARTH;
        budget.us_sim = 0;
        let d = k.step(Tick(1), budget, &mut []).unwrap();
        assert!(
            d.rejects.iter().all(|(_, r)| *r != RejectReason::Budget),
            "{d:?}"
        );
        assert_eq!(k.world().view().pose(s).unwrap().x, Mm(10));
    }

    #[test]
    fn equal_spatial_keys_reject_whole_class() {
        // No arrival-order fallback: two same-key grains both Conflict and
        // neither is admitted.
        let mut k = empty_kernel();
        let s = relic(1);
        plant_mover(&mut k, s, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)), 0, 0);
        k.ingest(space_delta(s, PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0))));
        k.ingest(space_delta(s, PoseMm::new(Mm(20), Mm(0), Mm(0), YawMd(0))));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert_eq!(d.rejects.len(), 2, "{d:?}");
        assert!(
            d.rejects.iter().all(|(_, r)| *r == RejectReason::Conflict),
            "{d:?}"
        );
        assert!(d.events.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(s).unwrap().x, Mm(0));
    }

    #[test]
    fn equal_player_keys_reject_whole_class() {
        // Same-slot Player intents share one admit key. Distinct grains are
        // ambiguous: both lose instead of racing on ingest order.
        // (Byte-identical retries collapse instead; see below.)
        let mut k = empty_kernel();
        let mut other = player_use();
        other.verb = Verb::Move;
        k.ingest(Proposal::Player(player_use()));
        k.ingest(Proposal::Player(other));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert_eq!(d.rejects.len(), 2, "{d:?}");
        assert!(
            d.rejects.iter().all(|(_, r)| *r == RejectReason::Conflict),
            "{d:?}"
        );
        assert!(d.events.is_empty(), "{d:?}");
    }

    #[test]
    fn identical_retries_admit_exactly_once() {
        // Byte-identical grains are an idempotent retry, not ambiguity: one
        // admits, the rest drop silently with no reject.
        let mut k = empty_kernel();
        let s = relic(1);
        plant_mover(&mut k, s, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)), 0, 0);
        let retry = space_delta(s, PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0)));
        k.ingest(retry.clone());
        k.ingest(retry);
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().pose(s).unwrap().x, Mm(10));
    }

    #[test]
    fn phys_island_admits_valid_constraint_and_break() {
        let a = relic(1);
        let b = relic(2);
        let cid = relic(9);
        let binding = hull_id(7);
        let mut canon = cook("[]");
        assert!(canon.bind_constraint(
            cid,
            klotho_core::ConstraintPhysics {
                a,
                b,
                binding,
                break_impulse: 10,
                ..klotho_core::ConstraintPhysics::default()
            }
        ));
        let mut k = CommitKernel::new(klotho_world::World::new(Arc::new(canon), Hash::ZERO));
        let start_a = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let start_b = PoseMm::new(Mm(400), Mm(0), Mm(0), YawMd::ZERO);
        plant_mover(&mut k, a, start_a, 0, 0);
        plant_mover(&mut k, b, start_b, 0, 0);
        k.ingest(Proposal::PhysIsland {
            epoch: Epoch::ZERO,
            tick: Tick(1),
            island: 0,
            members: vec![a, b],
            bodies: vec![body_delta(a, start_a), body_delta(b, start_b)],
            contacts: Vec::new(),
            constraints: vec![crate::ConstraintRef {
                constraint: cid,
                binding,
                impulse: 12,
            }],
            breaks: vec![crate::ConstraintBreakClaim {
                constraint: cid,
                impulse: 12,
            }],
        });
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        let state = k.world().view().constraint_state(cid).unwrap();
        assert!(state.broken);
        assert_eq!(state.impulse, 12);
    }

    #[test]
    fn phys_island_rejects_unknown_constraint_binding() {
        let a = relic(1);
        let start = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let mut k = empty_kernel();
        plant_mover(&mut k, a, start, 0, 0);
        k.ingest(Proposal::PhysIsland {
            epoch: Epoch::ZERO,
            tick: Tick(1),
            island: 0,
            members: vec![a],
            bodies: vec![body_delta(a, start)],
            contacts: Vec::new(),
            constraints: vec![crate::ConstraintRef {
                constraint: relic(9),
                binding: hull_id(7),
                impulse: 1,
            }],
            breaks: Vec::new(),
        });
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert_eq!(
            d.rejects,
            vec![(ProposalKind::Phys, RejectReason::WrongHull)]
        );
        assert_eq!(k.world().view().pose(a), Some(start));
    }

    #[test]
    fn partition_writes_live_island_ids() {
        let mut k = empty_kernel();
        let a = relic(1);
        let b = relic(2);
        plant_mover(&mut k, a, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)), 9, 0);
        plant_mover(
            &mut k,
            b,
            PoseMm::new(Mm(50_000), Mm(0), Mm(0), YawMd(0)),
            9,
            0,
        );
        let islands = k.partition();
        assert_eq!(islands.len(), 2);
        assert_eq!(k.world().view().island(a), Some((0, 0)));
        assert_eq!(k.world().view().island(b), Some((1, 0)));
    }

    #[test]
    fn partition_unassigned_is_no_island() {
        let mut k = empty_kernel();
        let awake = relic(1);
        let sleeper = relic(2);
        plant_mover(
            &mut k,
            awake,
            PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)),
            9,
            0,
        );
        plant_mover(
            &mut k,
            sleeper,
            PoseMm::new(Mm(50_000), Mm(0), Mm(0), YawMd(0)),
            9,
            klotho_core::SLEEP_AFTER_TICKS,
        );
        let islands = k.partition();
        assert_eq!(islands, vec![(0, vec![awake])]);
        assert_eq!(k.world().view().island(awake), Some((0, 0)));
        assert_eq!(
            k.world().view().island(sleeper),
            Some((NO_ISLAND, klotho_core::SLEEP_AFTER_TICKS))
        );
    }

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    fn opaque_kernel() -> CommitKernel {
        let mut h = [0u8; 32];
        h[0] = 1;
        CommitKernel::new(klotho_world::World::new(
            Arc::new(cook(
                r#"[AddAffordance(Affordance(id: "Opaque", requires: [], grants: [], conflicts: []))]"#,
            )),
            Hash::from_bytes(h),
        ))
    }

    fn residency(k: &CommitKernel, place: Sigil, op: ResidencyOp, snap: PlaceSnap) -> Proposal {
        Proposal::Residency {
            place,
            op,
            prefix: k.world().trace_prefix_hash(),
            canon_hash: k.world().canon_hash(),
            snap: Arc::new(snap),
        }
    }

    fn door_snap(k: &CommitKernel, p: Sigil, door: Sigil) -> PlaceSnap {
        let opaque = k.canon().affordance_id("Opaque").unwrap();
        let mut place_row = PlaceRow::new(p, LocusKind::Place);
        place_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        let mut door_row = PlaceRow::new(door, LocusKind::Relic);
        door_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        door_row.hull = Some(box_xz(400, 500, 400));
        door_row.afford = 1u64 << opaque.0;
        door_row.rels = vec![(Rel::In, p), (Rel::LockedBy, door)];
        PlaceSnap::new(
            p,
            k.world().canon_hash(),
            k.world().trace_prefix_hash(),
            vec![place_row, door_row],
        )
    }

    #[test]
    fn residency_load_inserts_rows_and_indexes_opaque_closed() {
        let mut k = opaque_kernel();
        let p = place(1);
        let door = relic(2);
        let snap = door_snap(&k, p, door);
        k.ingest(residency(&k, p, ResidencyOp::Load, snap));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(
            d.events.iter().any(|e| matches!(
                e.body,
                TraceBody::PlaceLoaded { place, n: 2 } if place == p
            )),
            "{d:?}"
        );
        assert!(k.world().view().contains(door));
        assert!(k.world().view().opaque_closed(door));
        let hits = k.world().view().space_candidates(
            AabbMm::new(
                IVec3 {
                    x: -10,
                    y: 0,
                    z: -10,
                },
                IVec3 {
                    x: 10,
                    y: 100,
                    z: 10,
                },
            ),
            true,
        );
        assert_eq!(hits, vec![door]);
    }

    #[test]
    fn residency_hash_mismatch_is_fail_closed() {
        let mut k = opaque_kernel();
        let p = place(1);
        let door = relic(2);
        let mut snap = door_snap(&k, p, door);
        snap = PlaceSnap::new(
            snap.place,
            Hash::from_bytes([9; 32]),
            snap.prefix,
            snap.rows().to_vec(),
        );
        let before = k.world().view().loci().count();
        k.ingest(residency(&k, p, ResidencyOp::Load, snap));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == ProposalKind::Residency && *r == RejectReason::Residency),
            "{d:?}"
        );
        assert!(
            !d.events
                .iter()
                .any(|e| matches!(e.body, TraceBody::PlaceLoaded { .. }))
        );
        assert_eq!(k.world().view().loci().count(), before);
    }

    #[test]
    fn residency_capture_prefix_need_not_match_live() {
        let mut k = opaque_kernel();
        let p = place(1);
        let door = relic(2);
        let mut snap = door_snap(&k, p, door);
        snap = PlaceSnap::new(
            snap.place,
            snap.canon_hash,
            Hash::ZERO,
            snap.rows().to_vec(),
        );
        k.ingest(residency(&k, p, ResidencyOp::Load, snap));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().contains(door));
    }

    #[test]
    fn residency_place_mismatch_is_residency() {
        let mut k = opaque_kernel();
        let p = place(1);
        let other = place(9);
        let door = relic(2);
        let snap = door_snap(&k, p, door);
        let before = k.world().view().loci().count();
        k.ingest(Proposal::Residency {
            place: other,
            op: ResidencyOp::Load,
            prefix: k.world().trace_prefix_hash(),
            canon_hash: k.world().canon_hash(),
            snap: Arc::new(snap),
        });
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == ProposalKind::Residency && *r == RejectReason::Residency),
            "{d:?}"
        );
        assert_eq!(k.world().view().loci().count(), before);
    }

    #[test]
    fn residency_canon_mismatch_is_epoch() {
        let mut k = opaque_kernel();
        let p = place(1);
        let door = relic(2);
        let rows = door_snap(&k, p, door).rows().to_vec();
        let wrong = Hash::from_bytes([7; 32]);
        let snap = PlaceSnap::new(p, wrong, k.world().trace_prefix_hash(), rows);
        let before = k.world().view().loci().count();
        k.ingest(Proposal::Residency {
            place: p,
            op: ResidencyOp::Load,
            prefix: k.world().trace_prefix_hash(),
            canon_hash: wrong,
            snap: Arc::new(snap),
        });
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects.iter().any(|(kind, r)| {
                *kind == ProposalKind::Residency && *r == RejectReason::EpochMismatch
            }),
            "{d:?}"
        );
        assert!(
            !d.events
                .iter()
                .any(|e| matches!(e.body, TraceBody::PlaceLoaded { .. }))
        );
        assert_eq!(k.world().view().loci().count(), before);
    }

    #[test]
    fn residency_evict_drops_owned_keeps_migrating_ends_rites() {
        let mut k = opaque_kernel();
        let p = place(1);
        let owned = relic(2);
        let migrant = relic(3);
        let host = relic(9);
        let mut place_row = PlaceRow::new(p, LocusKind::Place);
        place_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        let mut owned_row = PlaceRow::new(owned, LocusKind::Relic);
        owned_row.pose = Some(PoseMm::new(Mm(1), Mm(0), Mm(0), YawMd(0)));
        owned_row.rels = vec![(Rel::In, p)];
        let mut migrant_row = PlaceRow::new(migrant, LocusKind::Actor);
        migrant_row.pose = Some(PoseMm::new(Mm(2), Mm(0), Mm(0), YawMd(0)));
        migrant_row.rels = vec![(Rel::In, p), (Rel::AttachedTo, host)];
        let snap = PlaceSnap::new(
            p,
            k.world().canon_hash(),
            k.world().trace_prefix_hash(),
            vec![place_row, owned_row, migrant_row],
        );
        k.world_mut().insert_locus(host, LocusKind::Relic).unwrap();
        k.ingest(residency(&k, p, ResidencyOp::Load, snap.clone()));
        k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        k.world_mut().append(TraceEvent::new(
            Tick(1),
            TraceBody::RiteBegan {
                actor: owned,
                rite: 4,
                target: None,
            },
        ));
        assert!(k.world().view().first_rite(owned).is_some());
        let evict = PlaceSnap::new(
            p,
            k.world().canon_hash(),
            k.world().trace_prefix_hash(),
            snap.rows().to_vec(),
        );
        k.ingest(residency(&k, p, ResidencyOp::Evict, evict));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(
            d.events
                .iter()
                .any(|e| matches!(e.body, TraceBody::PlaceEvicted { place } if place == p)),
            "{d:?}"
        );
        assert!(
            d.events.iter().any(|e| matches!(
                e.body,
                TraceBody::RiteEnded {
                    actor,
                    rite: 4,
                    status: RiteEnd::Evicted,
                } if actor == owned
            )),
            "{d:?}"
        );
        assert!(!k.world().view().contains(owned));
        assert!(k.world().view().contains(migrant));
        assert!(k.world().view().contains(p));
        assert!(k.world().view().has_rel(migrant, Rel::AttachedTo, host));
    }

    #[test]
    fn residency_load_then_space_same_mover_is_conflict() {
        let mut k = opaque_kernel();
        let p = place(1);
        let mover = relic(2);
        plant_mover(
            &mut k,
            mover,
            PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)),
            0,
            0,
        );
        let mut place_row = PlaceRow::new(p, LocusKind::Place);
        place_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        let mut mover_row = PlaceRow::new(mover, LocusKind::Relic);
        mover_row.pose = Some(PoseMm::new(Mm(500), Mm(0), Mm(0), YawMd(0)));
        mover_row.hull = Some(box_xz(100, 1800, 100));
        mover_row.hull_id = hull_id(1);
        mover_row.rels = vec![(Rel::In, p)];
        let snap = PlaceSnap::new(
            p,
            k.world().canon_hash(),
            k.world().trace_prefix_hash(),
            vec![place_row, mover_row],
        );
        k.ingest(residency(&k, p, ResidencyOp::Load, snap));
        k.ingest(space_delta(
            mover,
            PoseMm::new(Mm(10), Mm(0), Mm(0), YawMd(0)),
        ));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == ProposalKind::Space && *r == RejectReason::Conflict),
            "{d:?}"
        );
        assert_eq!(k.world().view().pose(mover).unwrap().x, Mm(500));
    }

    #[test]
    fn residency_second_load_same_tick_is_epoch() {
        let mut k = opaque_kernel();
        let a = place(1);
        let b = place(2);
        let snap_a = PlaceSnap::new(
            a,
            k.world().canon_hash(),
            k.world().trace_prefix_hash(),
            vec![PlaceRow::new(a, LocusKind::Place)],
        );
        let snap_b = PlaceSnap::new(
            b,
            k.world().canon_hash(),
            k.world().trace_prefix_hash(),
            vec![PlaceRow::new(b, LocusKind::Place)],
        );
        k.ingest(residency(&k, a, ResidencyOp::Load, snap_a));
        k.ingest(residency(&k, b, ResidencyOp::Load, snap_b));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.events
                .iter()
                .any(|e| matches!(e.body, TraceBody::PlaceLoaded { place, .. } if place == a)),
            "{d:?}"
        );
        assert!(
            d.rejects.iter().any(|(kind, r)| {
                *kind == ProposalKind::Residency && *r == RejectReason::EpochMismatch
            }),
            "{d:?}"
        );
        assert!(!k.world().view().contains(b));
    }

    #[test]
    fn residency_second_place_loads_next_tick() {
        let mut k = opaque_kernel();
        let a = place(1);
        let b = place(2);
        let snap_a = PlaceSnap::new(
            a,
            k.world().canon_hash(),
            Hash::ZERO,
            vec![PlaceRow::new(a, LocusKind::Place)],
        );
        let snap_b = PlaceSnap::new(
            b,
            k.world().canon_hash(),
            Hash::ZERO,
            vec![PlaceRow::new(b, LocusKind::Place)],
        );
        k.ingest(residency(&k, a, ResidencyOp::Load, snap_a));
        let d1 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d1.rejects.is_empty(), "{d1:?}");
        k.ingest(residency(&k, b, ResidencyOp::Load, snap_b));
        let d2 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d2.rejects.is_empty(), "{d2:?}");
        assert!(k.world().view().contains(a));
        assert!(k.world().view().contains(b));
    }

    #[test]
    fn residency_oversize_duplicate_and_cap_are_fail_closed() {
        let mut k = opaque_kernel();
        let p = place(1);
        let before = k.world().view().loci().count();
        let too_big = vec![PlaceRow::new(relic(1), LocusKind::Relic); MAX_PLACE_ROWS + 1];
        let snap = PlaceSnap::new(p, k.world().canon_hash(), Hash::ZERO, too_big);
        k.ingest(residency(&k, p, ResidencyOp::Load, snap));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == ProposalKind::Residency && *r == RejectReason::Residency),
            "{d:?}"
        );
        assert!(
            !d.events
                .iter()
                .any(|e| matches!(e.body, TraceBody::PlaceLoaded { .. }))
        );
        assert_eq!(k.world().view().loci().count(), before);

        let row = PlaceRow::new(relic(1), LocusKind::Relic);
        let dup = PlaceSnap::new(
            p,
            k.world().canon_hash(),
            Hash::ZERO,
            vec![row.clone(), row],
        );
        k.ingest(residency(&k, p, ResidencyOp::Load, dup));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == ProposalKind::Residency && *r == RejectReason::Residency),
            "{d:?}"
        );
        assert_eq!(k.world().view().loci().count(), before);

        let mut capped = CommitKernel::new(klotho_world::World::with_locus_cap(
            Arc::new(cook("[]")),
            k.world().canon_hash(),
            4,
        ));
        let rows: Vec<_> = (0..5u128)
            .map(|i| PlaceRow::new(relic(i + 1), LocusKind::Relic))
            .collect();
        let snap = PlaceSnap::new(p, capped.world().canon_hash(), Hash::ZERO, rows);
        capped.ingest(residency(&capped, p, ResidencyOp::Load, snap));
        let d = capped.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == ProposalKind::Residency && *r == RejectReason::Residency),
            "{d:?}"
        );
        assert_eq!(capped.world().view().loci().count(), 0);
        assert!(
            !d.events
                .iter()
                .any(|e| matches!(e.body, TraceBody::PlaceLoaded { .. }))
        );
    }

    #[test]
    fn residency_missing_place_evict_rejects() {
        let mut k = opaque_kernel();
        let p = place(1);
        let snap = PlaceSnap::new(
            p,
            k.world().canon_hash(),
            k.world().trace_prefix_hash(),
            vec![PlaceRow::new(p, LocusKind::Place)],
        );
        k.ingest(residency(&k, p, ResidencyOp::Evict, snap));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(kind, r)| *kind == ProposalKind::Residency && *r == RejectReason::Residency),
            "{d:?}"
        );
        assert!(
            !d.events
                .iter()
                .any(|e| matches!(e.body, TraceBody::PlaceEvicted { .. }))
        );
    }
}
