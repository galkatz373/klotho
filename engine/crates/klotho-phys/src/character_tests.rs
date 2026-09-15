use std::sync::Arc;

use klotho_canon::cook_diffs;
use klotho_commit::{AdmitBuf, CommitKernel, Proposal, SyncProposer};
use klotho_core::{
    AabbMm, BlobId, BodyMode, BodyPhysics, Budget, CharacterPhysics, Hash, IVec3, LocusKind, Mm,
    PoseMm, ShapeKind, Sigil, Tick, Vel3, VelFx, YawMd,
};
use klotho_ir::{CanonDiff, from_ron};
use klotho_motion::Motion;
use klotho_world::World;

use crate::Phys;

fn id(kind: LocusKind, n: u128) -> Sigil {
    Sigil::pack(kind, 0, n).unwrap()
}
fn actor() -> Sigil {
    id(LocusKind::Actor, 1)
}
fn box_hull(x: i32, y: i32, z: i32) -> AabbMm {
    AabbMm::new(IVec3 { x: -x, y: 0, z: -z }, IVec3 { x, y, z })
}
fn pose(x: i32, y: i32, z: i32) -> PoseMm {
    PoseMm::new(Mm(x), Mm(y), Mm(z), YawMd::ZERO)
}
fn plant(k: &mut CommitKernel, s: Sigil, hull: AabbMm, pose: PoseMm) {
    let mut w = k.world_mut();
    w.insert_locus(s, s.kind().unwrap()).unwrap();
    w.set_hull(s, hull, BlobId::from_bytes([s.raw() as u8; 32]))
        .unwrap();
    w.set_pose(s, pose).unwrap();
    w.set_island(s, 99, 0).unwrap();
}
fn boot(extra: &[(Sigil, BodyPhysics)]) -> CommitKernel {
    let mut canon = cook_diffs(&from_ron::<Vec<CanonDiff>>("[]").unwrap()).unwrap();
    assert!(canon.bind_physics(
        actor(),
        BodyPhysics {
            shape: ShapeKind::Capsule,
            character: Some(CharacterPhysics::default()),
            ..BodyPhysics::default()
        }
    ));
    for &(s, p) in extra {
        assert!(canon.bind_physics(s, p));
    }
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    plant(&mut k, actor(), box_hull(300, 1800, 300), pose(0, 0, 0));
    plant(
        &mut k,
        id(LocusKind::Place, 9),
        box_hull(10000, 200, 10000),
        pose(0, -200, 0),
    );
    k
}
fn moving(k: &mut CommitKernel) {
    k.world_mut()
        .set_vel(
            actor(),
            Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20)),
            0,
        )
        .unwrap();
}
fn tick(k: &mut CommitKernel, t: u64) {
    k.partition();
    let d = k
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Phys])
        .unwrap();
    assert!(d.rejects.is_empty(), "tick {t}: {d:?}");
}
fn scenery() -> BodyPhysics {
    BodyPhysics {
        mode: BodyMode::Static,
        ..BodyPhysics::default()
    }
}

#[test]
fn character_stairs_walk_up_and_down_frozen_risers() {
    for riser in [200, 250] {
        let stair = id(LocusKind::Relic, 2);
        let mut k = boot(&[(stair, scenery())]);
        plant(&mut k, stair, box_hull(2000, riser, 600), pose(0, 0, 1100));
        moving(&mut k);
        let mut highest = 0;
        for t in 1..=130 {
            tick(&mut k, t);
            highest = highest.max(k.world().view().pose(actor()).unwrap().y.0);
        }
        let root = k.world().view().pose(actor()).unwrap();
        assert!(
            highest >= riser - 2,
            "riser={riser}, highest={highest}, final={root:?}"
        );
        assert!(
            root.z.0 >= 2400 && root.y.0.abs() <= 3,
            "riser={riser}: {root:?}"
        );
    }
}

#[test]
fn character_root_stops_at_thin_wall_and_has_one_owner() {
    let wall = id(LocusKind::Relic, 2);
    let mut k = boot(&[(wall, scenery())]);
    plant(&mut k, wall, box_hull(2000, 3000, 5), pose(0, 0, 800));
    moving(&mut k);
    let mut out = AdmitBuf::new();
    Motion::hearth().propose(&k.world().view(), Tick(1), &mut out);
    assert!(out.drain().is_empty());
    for t in 1..=80 {
        tick(&mut k, t);
    }
    let root = k.world().view().pose(actor()).unwrap();
    assert!((470..=500).contains(&root.z.0), "{root:?}");
    // A producer cannot regain ownership by forging a MotionDelta.
    let mut witness = klotho_core::HullWitness::new(actor(), pose(0, 0, 2000), false);
    witness.epoch = k.world().epoch();
    k.ingest(Proposal::MotionDelta {
        mover: actor(),
        pose: pose(0, 0, 2000),
        vel: Vel3::ZERO,
        yaw_rate: 0,
        island: k.world().view().island(actor()).unwrap().0,
        sleep_ticks: 0,
        clip: 0,
        root: IVec3 { x: 0, y: 0, z: 20 },
        hull: k.world().view().hull_id(actor()).unwrap(),
        witness,
    });
    let d = k.step(Tick(81), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| *r == klotho_core::RejectReason::Conflict)
    );
    assert_eq!(k.world().view().pose(actor()), Some(root));
}

#[test]
fn character_pushes_crate_in_one_atomic_island() {
    let crate_s = id(LocusKind::Relic, 2);
    let mut k = boot(&[]);
    plant(&mut k, crate_s, box_hull(200, 400, 200), pose(0, 0, 650));
    moving(&mut k);
    let islands = k.partition();
    assert!(
        islands
            .iter()
            .any(|(_, m)| m.contains(&actor()) && m.contains(&crate_s))
    );
    for t in 1..=40 {
        tick(&mut k, t);
    }
    assert!(k.world().view().pose(crate_s).unwrap().z.0 > 900);
    // An invalid member rolls the complete solution back.
    k.partition();
    let island = k.world().view().island(actor()).unwrap().0;
    let mut solved = crate::solve_island(island, &k.world().view());
    let Proposal::PhysIsland {
        bodies,
        tick: proposed_tick,
        ..
    } = &mut solved.proposals[0]
    else {
        panic!()
    };
    *proposed_tick = Tick(k.world().tick().0 + 1);
    bodies.iter_mut().find(|b| b.mover == crate_s).unwrap().hull = BlobId::from_bytes([77; 32]);
    let mut before = k.snapshot().encode().unwrap();
    k.ingest(solved.proposals.remove(0));
    let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| *r == klotho_core::RejectReason::WrongHull)
    );
    let mut after = k.snapshot().encode().unwrap();
    before[16..24].fill(0);
    after[16..24].fill(0);
    assert_eq!(before, after);
}

#[test]
fn translating_and_rotating_platform_carries_idle_character() {
    let platform = id(LocusKind::Relic, 2);
    let mut k = boot(&[(
        platform,
        BodyPhysics {
            mode: BodyMode::Kinematic,
            ..BodyPhysics::default()
        },
    )]);
    plant(&mut k, platform, box_hull(2000, 200, 2000), pose(0, 0, 0));
    k.world_mut().set_pose(actor(), pose(1000, 200, 0)).unwrap();
    k.world_mut()
        .set_vel(
            platform,
            Vel3::new(VelFx::from_mm_per_tick(10), VelFx::ZERO, VelFx::ZERO),
            2000,
        )
        .unwrap();
    for t in 1..=20 {
        tick(&mut k, t);
    }
    let root = k.world().view().pose(actor()).unwrap();
    let base = k.world().view().pose(platform).unwrap();
    assert_eq!(base.x.0, 200);
    let expected = klotho_core::rotate_xz(
        IVec3 {
            x: 1000,
            y: 0,
            z: 0,
        },
        base.yaw,
    );
    assert!(
        (root.x.0 - base.x.0 - expected.x).abs() <= 40,
        "{root:?} {base:?} {expected:?}"
    );
    assert!(
        (root.z.0 - expected.z).abs() <= 40 && (root.y.0 - 200).abs() <= 3,
        "{root:?}"
    );
}

#[test]
fn character_falls_and_grounds_without_y_zero_fallback() {
    let mut k = boot(&[]);
    k.world_mut().set_pose(actor(), pose(0, 600, 0)).unwrap();
    for t in 1..=40 {
        tick(&mut k, t);
    }
    let view = k.world().view();
    assert!(view.pose(actor()).unwrap().y.0.abs() <= 3);
    assert!(view.support(actor()).is_some());
}

#[test]
fn character_walks_permitted_slope_and_blocks_steep_ascent() {
    fn run(height: i32) -> PoseMm {
        let ramp = id(LocusKind::Place, 3);
        let mut k = boot(&[(
            ramp,
            BodyPhysics {
                mode: BodyMode::Static,
                shape: ShapeKind::Heightfield,
                ..BodyPhysics::default()
            },
        )]);
        plant(
            &mut k,
            ramp,
            AabbMm::new(
                IVec3 {
                    x: -2000,
                    y: 0,
                    z: 0,
                },
                IVec3 {
                    x: 2000,
                    y: height,
                    z: 3000,
                },
            ),
            pose(0, 0, 0),
        );
        // Begin on the ramp, rather than overlapping its finite lower edge.
        k.world_mut()
            .set_pose(actor(), pose(0, height / 5 + 180, 600))
            .unwrap();
        for t in 1..=30 {
            tick(&mut k, t);
        }
        moving(&mut k);
        for t in 31..=90 {
            tick(&mut k, t);
        }
        k.world().view().pose(actor()).unwrap()
    }
    let permitted = run(1732); // 30 degrees
    let steep = run(3575); // 50 degrees
    assert!(permitted.z.0 > 1500 && permitted.y.0 > 800, "{permitted:?}");
    assert!(steep.z.0 < 750, "{steep:?}");
}

#[test]
fn character_save_roundtrip_resumes_the_same_physical_state() {
    let platform = id(LocusKind::Relic, 2);
    let binding = [(
        platform,
        BodyPhysics {
            mode: BodyMode::Kinematic,
            ..BodyPhysics::default()
        },
    )];
    let mut original = boot(&binding);
    plant(
        &mut original,
        platform,
        box_hull(2000, 200, 2000),
        pose(0, 0, 0),
    );
    original
        .world_mut()
        .set_pose(actor(), pose(1000, 200, 0))
        .unwrap();
    original
        .world_mut()
        .set_vel(
            platform,
            Vel3::new(VelFx::from_mm_per_tick(10), VelFx::ZERO, VelFx::ZERO),
            2000,
        )
        .unwrap();
    for t in 1..=12 {
        tick(&mut original, t);
    }
    let saved = klotho_save::pause_save(&original.snapshot()).unwrap();
    let loaded = klotho_save::decode(&klotho_save::encode(&saved).unwrap()).unwrap();
    let restored = klotho_save::restore(&loaded, saved.prefix, saved.canon_hash).unwrap();
    // The save contains Projection; rebind the identical cooked Canon on boot.
    let mut resumed = boot(&binding);
    let view = restored.view();
    for s in view.loci() {
        if !resumed.world().view().loci().any(|r| r == s) {
            plant(
                &mut resumed,
                s,
                view.hull(s).unwrap(),
                view.pose(s).unwrap(),
            );
        }
        let mut w = resumed.world_mut();
        w.set_pose(s, view.pose(s).unwrap()).unwrap();
        let (v, yaw) = view.vel(s).unwrap();
        w.set_vel(s, v, yaw).unwrap();
        let (yaw, pitch, roll) = view.rates(s).unwrap();
        w.set_rates(s, yaw, pitch, roll).unwrap();
        w.set_support(s, view.support(s)).unwrap();
        let (island, sleep) = view.island(s).unwrap();
        w.set_island(s, island, sleep).unwrap();
    }
    for event in original.world().trace().events().to_vec() {
        resumed.world_mut().append(event);
    }
    resumed.world_mut().set_tick(saved.trace_from_tick);
    for t in 13..=32 {
        tick(&mut original, t);
        tick(&mut resumed, t);
        for s in [actor(), platform] {
            assert_eq!(
                original.world().view().pose(s),
                resumed.world().view().pose(s)
            );
            assert_eq!(
                original.world().view().vel(s),
                resumed.world().view().vel(s)
            );
            assert_eq!(
                original.world().view().support(s),
                resumed.world().view().support(s)
            );
        }
    }
    assert_eq!(
        original.snapshot().encode().unwrap(),
        resumed.snapshot().encode().unwrap()
    );
}

#[test]
fn character_one_and_eight_workers_preserve_trace_and_projection() {
    fn run(workers: usize) -> (Hash, Vec<u8>) {
        let mut k = boot(&[]);
        moving(&mut k);
        // Eight disjoint live islands ensure this exercises parallel proposal.
        for n in 2..=9 {
            plant(
                &mut k,
                id(LocusKind::Relic, n),
                box_hull(100, 200, 100),
                pose(n as i32 * 4000, 500, 0),
            );
        }
        for t in 1..=32 {
            let islands = k.partition();
            if t == 1 {
                assert!(islands.len() >= 8);
            }
            struct Jobs(usize);
            impl SyncProposer for Jobs {
                fn name(&self) -> &'static str {
                    "character-test-jobs"
                }
                fn propose(&mut self, view: &klotho_world::WorldView, _: Tick, out: &mut AdmitBuf) {
                    let mut groups = std::collections::BTreeMap::<u16, Vec<Sigil>>::new();
                    for s in view.loci() {
                        if let Some((island, _)) = view.island(s) {
                            if island != klotho_core::NO_ISLAND {
                                groups.entry(island).or_default().push(s);
                            }
                        }
                    }
                    let islands = groups.into_iter().collect::<Vec<_>>();
                    for (p, _) in klotho_jobs::propose_islands(self.0, &islands, &[&Phys], view) {
                        out.push(p);
                    }
                }
            }
            let d = k
                .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Jobs(workers)])
                .unwrap();
            assert!(d.rejects.is_empty(), "{d:?}");
        }
        (
            k.world().trace_prefix_hash(),
            k.snapshot().encode().unwrap(),
        )
    }
    assert_eq!(run(1), run(8));
}

#[test]
fn character_rejects_overheight_steps_and_forged_static_crossings() {
    let stair = id(LocusKind::Relic, 2);
    let mut k = boot(&[(stair, scenery())]);
    plant(&mut k, stair, box_hull(2000, 300, 600), pose(0, 0, 1100));
    moving(&mut k);
    for t in 1..=60 {
        tick(&mut k, t);
    }
    let before = k.world().view().pose(actor()).unwrap();
    assert!(before.z.0 < 500 && before.y.0 < 10, "{before:?}");
    k.partition();
    let island = k.world().view().island(actor()).unwrap().0;
    let mut proposal = crate::solve_island(island, &k.world().view())
        .proposals
        .remove(0);
    let Proposal::PhysIsland {
        bodies,
        tick: proposed_tick,
        ..
    } = &mut proposal
    else {
        panic!()
    };
    *proposed_tick = Tick(k.world().tick().0 + 1);
    bodies[0].pose.z.0 = before.z.0 + 1000;
    bodies[0].witness.proposed = bodies[0].pose;
    k.ingest(proposal);
    let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| *r == klotho_core::RejectReason::WitnessMismatch),
        "{d:?}"
    );
    assert_eq!(k.world().view().pose(actor()), Some(before));
}
