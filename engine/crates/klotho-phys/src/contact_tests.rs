//! PHYS-A08 bounded animation-contact acceptance, without a presenter.
use crate::Phys;
use klotho_canon::cook_diffs;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{
    AabbMm, BlobId, BodyMode, BodyPhysics, Budget, CharacterPhysics, ContactSocket, ContactSweep,
    ContactTrack, Hash, IVec3, LocusKind, Mm, PlayerId, PoseMm, Sigil, Tick, YawMd,
};
use klotho_ir::{Agency, Analog, CanonDiff, Channel, IntentTarget, PlayerIntent, Verb, from_ron};
use klotho_trace::TraceBody;
use klotho_world::World;
use std::sync::Arc;
const DIFFS: &str = r#"[
AddAffordance(Affordance(id:"Hittable",requires:[],grants:[],conflicts:[])),
AddRite(RiteGraph(id:"melee",cap_steps:16,cap_ticks:8,entry:0,nodes:[
{pc:0,op:Bind(Target)}, {pc:1,op:Wait(2,Some(Aim))},
{pc:2,op:Emit("Hit")}, {pc:3,op:Complete(Success)}])),
AddRite(RiteGraph(id:"apply_hit",cap_steps:8,cap_ticks:4,entry:0,nodes:[
{pc:0,op:Spend("health",25,2)}, {pc:1,op:Complete(Success)}, {pc:2,op:Complete(Fail)}]))
]"#;
fn id(kind: LocusKind, n: u128) -> Sigil {
    Sigil::pack(kind, 0, n).unwrap()
}
fn actor() -> Sigil {
    id(LocusKind::Actor, 1)
}
fn target() -> Sigil {
    id(LocusKind::Relic, 2)
}
fn p(x: i32, y: i32, z: i32) -> IVec3 {
    IVec3 { x, y, z }
}
fn pose(x: i32, y: i32, z: i32) -> PoseMm {
    PoseMm::new(Mm(x), Mm(y), Mm(z), YawMd::ZERO)
}
fn plant(k: &mut CommitKernel, s: Sigil, half: i32, height: i32, at: PoseMm) {
    let mut w = k.world_mut();
    w.insert_locus(s, s.kind().unwrap()).unwrap();
    w.set_hull(
        s,
        AabbMm::new(p(-half, 0, -half), p(half, height, half)),
        BlobId::from_bytes([s.id() as u8; 32]),
    )
    .unwrap();
    w.set_pose(s, at).unwrap();
}
fn boot(miss: bool, root: i32) -> CommitKernel {
    boot_canon(miss, root, DIFFS)
}
fn boot_canon(miss: bool, root: i32, diffs: &str) -> CommitKernel {
    let mut canon = cook_diffs(&from_ron::<Vec<CanonDiff>>(diffs).unwrap()).unwrap();
    canon.bind_physics(
        actor(),
        BodyPhysics {
            shape: klotho_core::ShapeKind::Capsule,
            character: Some(CharacterPhysics::default()),
            ..BodyPhysics::default()
        },
    );
    canon.bind_physics(
        target(),
        BodyPhysics {
            mode: BodyMode::Static,
            ..BodyPhysics::default()
        },
    );
    let ends = vec![
        [p(-600, 900, 700), p(-600, 1300, 700)],
        [p(600, 900, 700), p(600, 1300, 700)],
        [p(-600, 900, 700), p(-600, 1300, 700)],
    ];
    let track = ContactTrack {
        skeleton: Hash::from_bytes([1; 32]),
        instrument: Hash::from_bytes([2; 32]),
        action: Hash::from_bytes([3; 32]),
        rite: "melee".into(),
        tick_hz: 30,
        wait_pc: 1,
        channel: Channel::Aim.as_u8(),
        wait_ticks: 2,
        roots: vec![p(0, 0, 0), p(0, 0, root), p(0, 0, root * 2)],
        sockets: vec![ContactSocket {
            name: "grip".into(),
            samples: vec![p(0, 900, 700); 3],
        }],
        sweeps: vec![ContactSweep {
            name: "blade".into(),
            socket: "grip".into(),
            radius_mm: 60,
            samples: ends,
        }],
        plants: vec![],
    };
    assert!(track.is_valid());
    canon.contact_tracks.insert(actor(), track);
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    plant(&mut k, actor(), 300, 1800, pose(0, 0, 0));
    plant(
        &mut k,
        target(),
        100,
        1800,
        pose(if miss { 2000 } else { 0 }, 0, 700),
    );
    plant(
        &mut k,
        id(LocusKind::Place, 9),
        10000,
        200,
        pose(0, -200, 0),
    );
    let health = k.canon().resource_id("health").unwrap();
    let hittable = k.canon().affordance_id("Hittable").unwrap();
    k.world_mut().set_qty(target(), health, 100).unwrap();
    k.world_mut()
        .set_affordance(target(), hittable, true)
        .unwrap();
    k.bind_player(PlayerId(0), actor());
    k
}
fn packet(k: &CommitKernel, claimed: bool) -> PlayerIntent {
    PlayerIntent {
        player: PlayerId(0),
        at: k.world().tick(),
        verb: Verb::Use,
        target: IntentTarget::Sigil(target()),
        analog: Analog::default(),
        agency: Agency {
            assist: klotho_ir::AssistLevel::None,
            claimed: if claimed { vec![Channel::Aim] } else { vec![] },
        },
    }
}
fn start(k: &mut CommitKernel) {
    k.ingest(Proposal::Player(packet(k, true)));
    let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(d.rejects.is_empty(), "{d:?}");
}
fn tick(k: &mut CommitKernel) {
    k.partition();
    let d = k
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Phys])
        .unwrap();
    assert!(d.rejects.is_empty(), "{d:?}")
}
fn health(k: &CommitKernel) -> i32 {
    k.world()
        .view()
        .qty(target(), k.canon().resource_id("health").unwrap())
}
#[test]
fn crossing_hits_once_and_visible_miss_expires_without_damage() {
    for miss in [false, true] {
        let mut k = boot(miss, 0);
        start(&mut k);
        for _ in 0..4 {
            tick(&mut k)
        }
        assert_eq!(health(&k), if miss { 100 } else { 75 });
        assert!(k.world().view().first_rite(actor()).is_none());
    }
}
#[test]
fn no_active_window_has_no_contact_and_player_cannot_shortcut_it() {
    let mut k = boot(false, 0);
    tick(&mut k);
    assert_eq!(health(&k), 100);
    start(&mut k);
    k.ingest(Proposal::Player(packet(&k, true)));
    let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert_eq!(d.rejects[0].1, klotho_core::RejectReason::UnclaimedAgency);
    assert_eq!(health(&k), 100);
    tick(&mut k);
    assert_eq!(health(&k), 75);
}
#[test]
fn unclaimed_player_mind_and_infer_cannot_start_contact_action() {
    let mut k = boot(false, 0);
    k.ingest(Proposal::Player(packet(&k, false)));
    let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert_eq!(d.rejects[0].1, klotho_core::RejectReason::UnclaimedAgency);
    assert!(k.world().view().first_rite(actor()).is_none());
}
#[test]
fn invalid_root_or_contact_rolls_back_damage_phase_and_all_body_rows() {
    for fault in 0..5 {
        let mut k = boot(false, 0);
        start(&mut k);
        k.partition();
        let island = k.world().view().island(actor()).unwrap().0;
        let mut solved = crate::solve_island(island, &k.world().view());
        let Proposal::PhysIsland {
            bodies,
            motion_contacts,
            tick: at,
            epoch,
            ..
        } = &mut solved.proposals[0]
        else {
            panic!()
        };
        *at = Tick(k.world().tick().0 + 1);
        assert_eq!(motion_contacts.len(), 1);
        match fault {
            0 => bodies[0].hull = BlobId::from_bytes([99; 32]),
            1 => motion_contacts[0].witness.time = u16::MAX,
            2 => motion_contacts[0].track.instrument = Hash::ZERO,
            3 => motion_contacts.push(motion_contacts[0].clone()),
            _ => *epoch = klotho_core::Epoch(99),
        }
        let mut before = k.snapshot().encode().unwrap();
        k.ingest(solved.proposals.remove(0));
        let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
        assert!(!d.rejects.is_empty());
        assert_eq!(health(&k), 100);
        let mut after = k.snapshot().encode().unwrap();
        before[16..24].fill(0);
        after[16..24].fill(0);
        assert_eq!(before, after);
    }
}
#[test]
fn trace_suffix_restores_window_and_admitted_damage() {
    let mut k = boot(false, 0);
    let snap = k.snapshot();
    start(&mut k);
    let started = k.world().trace().events().to_vec();
    let replay = snap.replay_suffix(&started);
    assert!(replay.view().contact_window(actor()).is_some());
    tick(&mut k);
    assert_eq!(health(&k), 75);
    assert!(
        k.world().trace().events().iter().any(
            |e| matches!(e.body,TraceBody::Emitted{a,b:Some(b),..} if a==actor()&&b==target())
        )
    );
    let replay = snap.replay_suffix(k.world().trace().events());
    assert_eq!(
        replay
            .view()
            .qty(target(), k.canon().resource_id("health").unwrap()),
        75
    );
    assert!(replay.view().first_rite(actor()).is_none());
}

#[test]
fn mind_and_infer_have_no_player_contact_authority() {
    for active in [false, true] {
        for infer in [false, true] {
            let mut k = boot(false, 0);
            if active {
                start(&mut k)
            }
            let proposal = if infer {
                Proposal::Infer(klotho_ir::InferIntent {
                    model: klotho_ir::ModelId(klotho_ir::Name::from("contact")),
                    locus: Some(actor()),
                    verb: Verb::Use,
                    target: IntentTarget::Sigil(target()),
                    claimed_facts: vec![],
                })
            } else {
                Proposal::Mind(klotho_ir::MindIntent {
                    locus: actor(),
                    verb: Verb::Use,
                    target: IntentTarget::Sigil(target()),
                    utility: 1,
                })
            };
            k.ingest(proposal);
            let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
            assert_eq!(d.rejects[0].1, klotho_core::RejectReason::UnclaimedAgency);
            assert_eq!(health(&k), 100);
        }
    }
}
#[test]
fn skipped_ticks_expire_the_absolute_window() {
    let mut k = boot(false, 0);
    start(&mut k);
    k.partition();
    let d = k
        .step(Tick(10), Budget::AAA_ADVENTURE, &mut [&mut Phys])
        .unwrap();
    assert!(d.rejects.is_empty(), "{d:?}");
    assert_eq!(health(&k), 100);
    assert!(k.world().view().first_rite(actor()).is_none());
}
#[test]
fn resolved_wall_root_cannot_leave_a_desired_root_hit() {
    let mut k = boot(false, 1000);
    k.world_mut().set_pose(target(), pose(0, 0, 1650)).unwrap();
    let wall = id(LocusKind::Place, 8);
    plant(&mut k, wall, 2000, 1800, pose(0, 0, 2400));
    // The front of this box is z=400: the capsule can only reach z=100.
    start(&mut k);
    for _ in 0..3 {
        tick(&mut k)
    }
    assert!(k.world().view().pose(actor()).unwrap().z.0 <= 103);
    assert_eq!(health(&k), 100);
}
#[test]
fn pause_save_preserves_action_identity_and_resume_result() {
    for pending in [false, true] {
        let diffs = if pending {
            include_str!("../../klotho-canon/fixtures/ember.ron")
        } else {
            DIFFS
        };
        let mut original = boot_canon(false, 0, diffs);
        start(&mut original);
        if pending {
            tick(&mut original);
            assert_eq!(health(&original), 100);
        }

        let saved = klotho_save::pause_save(&original.snapshot()).unwrap();
        let loaded = klotho_save::decode(&klotho_save::encode(&saved).unwrap()).unwrap();
        let restored = klotho_save::restore(&loaded, saved.prefix, saved.canon_hash).unwrap();
        let rows = restored.snap_rows();
        let active = rows
            .iter()
            .find(|r| r.sigil == actor())
            .unwrap()
            .rites
            .clone();
        assert_eq!(active[0].1.started_at, Tick(1));
        assert_eq!(active[0].1.wait_at, Tick(if pending { 2 } else { 1 }));
        assert_eq!(active[0].1.contact_hit, pending);
        let mut resumed = boot_canon(false, 0, diffs);
        for e in original.world().trace().events().to_vec() {
            resumed.world_mut().append(e)
        }
        for row in rows {
            let mut w = resumed.world_mut();
            if let Some(p) = row.pose {
                w.set_pose(row.sigil, p).unwrap()
            }
            w.set_vel(row.sigil, row.vel, row.yaw_rate).unwrap();
            w.set_rates(row.sigil, row.yaw_rate, row.pitch_rate, row.roll_rate)
                .unwrap();
            w.set_support(row.sigil, row.support).unwrap();
            w.set_island(row.sigil, row.island, row.sleep_ticks)
                .unwrap();
            for (res, qty) in row.qty {
                w.set_qty(row.sigil, res, qty).unwrap()
            }
            let mut spec = w.begin_spec();
            for (rid, machine) in row.rites {
                spec.put_rite(row.sigil, klotho_canon::RiteId(rid), machine)
            }
            w.commit_spec(spec);
        }
        resumed.world_mut().set_tick(saved.trace_from_tick);
        for _ in 0..3 {
            tick(&mut original);
            tick(&mut resumed)
        }
        assert_eq!(health(&resumed), 75);
        assert_eq!(
            original.snapshot().encode().unwrap(),
            resumed.snapshot().encode().unwrap()
        );
    }
}
#[test]
fn one_and_eight_workers_match_during_contact_admission() {
    fn run(workers: usize) -> (Hash, Vec<u8>) {
        let mut k = boot(false, 0);
        for n in 10..=17 {
            plant(
                &mut k,
                id(LocusKind::Relic, n),
                100,
                200,
                pose(n as i32 * 4000, 500, 0),
            )
        }
        start(&mut k);
        struct Jobs(usize);
        impl klotho_commit::SyncProposer for Jobs {
            fn name(&self) -> &'static str {
                "contact-jobs"
            }
            fn propose(
                &mut self,
                view: &klotho_world::WorldView,
                _: Tick,
                out: &mut klotho_commit::AdmitBuf,
            ) {
                let mut groups = std::collections::BTreeMap::<u16, Vec<Sigil>>::new();
                for s in view.loci() {
                    if let Some((island, _)) = view.island(s) {
                        if island != klotho_core::NO_ISLAND {
                            groups.entry(island).or_default().push(s)
                        }
                    }
                }
                let groups = groups.into_iter().collect::<Vec<_>>();
                for (p, _) in klotho_jobs::propose_islands(self.0, &groups, &[&Phys], view) {
                    out.push(p)
                }
            }
        }
        for _ in 0..3 {
            let islands = k.partition();
            assert!(islands.len() >= 8);
            let d = k
                .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Jobs(workers)])
                .unwrap();
            assert!(d.rejects.is_empty(), "{d:?}")
        }
        assert_eq!(health(&k), 75);
        (
            k.world().trace_prefix_hash(),
            k.snapshot().encode().unwrap(),
        )
    }
    assert_eq!(run(1), run(8));
}
#[test]
fn target_law_failure_rolls_back_contact_and_damage() {
    let diffs = format!(
        r#"{},AddLaw(Law(id:"contact.health",when:EqVerb(Use),body:Pred(must:Qty(Target,"health",Ge,100),ought:None)))]"#,
        DIFFS.trim_end().trim_end_matches(']')
    );
    let mut k = boot_canon(false, 0, &diffs);
    start(&mut k);
    k.partition();
    let mut before = k.snapshot().encode().unwrap();
    let d = k
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Phys])
        .unwrap();
    assert!(matches!(d.rejects[0].1, klotho_core::RejectReason::Law(_)));
    assert_eq!(health(&k), 100);
    let mut after = k.snapshot().encode().unwrap();
    before[16..24].fill(0);
    after[16..24].fill(0);
    assert_eq!(before, after);
}
#[test]
fn a_seed_wait_cannot_supply_player_provenance() {
    let mut k = boot(false, 0);
    let rite = k
        .canon()
        .rites
        .iter()
        .find(|r| r.name.as_str() == "melee")
        .unwrap()
        .id;
    k.world_mut().append(klotho_trace::TraceEvent::new(
        Tick(1),
        TraceBody::RiteBegan {
            actor: actor(),
            rite: rite.0,
            target: Some(target()),
        },
    ));
    k.world_mut().append(klotho_trace::TraceEvent::new(
        Tick(1),
        TraceBody::RiteAdvanced {
            actor: actor(),
            rite: rite.0,
            pc: 2,
            wait_left: 2,
        },
    ));
    assert!(k.world().view().contact_window(actor()).is_none());
    k.partition();
    let d = k
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Phys])
        .unwrap();
    assert_eq!(d.rejects[0].1, klotho_core::RejectReason::UnclaimedAgency);
    assert_eq!(health(&k), 100);
}
