use std::sync::Arc;

use klotho_canon::cook_diffs;
use klotho_commit::{AdmitBuf, CommitKernel, SyncProposer};
use klotho_core::{
    AabbMm, BlobId, BodyMode, BodyPhysics, Budget, Hash, IVec3, LocusKind, Mm, PhysRequest, PoseMm,
    ShapeKind, Sigil, Tick, VehiclePhysics, Vel3, VelFx, YawMd,
};
use klotho_ir::{CanonDiff, Rel, from_ron};
use klotho_motion::Motion;
use klotho_world::World;

use crate::Phys;

fn id(kind: LocusKind, n: u128) -> Sigil {
    Sigil::pack(kind, 0, n).unwrap()
}
fn chassis() -> Sigil {
    id(LocusKind::Relic, 1)
}
fn driver() -> Sigil {
    id(LocusKind::Actor, 2)
}
fn box_hull(x: i32, y: i32, z: i32) -> AabbMm {
    AabbMm::new(IVec3 { x: -x, y: 0, z: -z }, IVec3 { x, y, z })
}
fn chassis_hull() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -400,
            y: 100,
            z: -800,
        },
        IVec3 {
            x: 400,
            y: 500,
            z: 800,
        },
    )
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
fn rig(friction: u16) -> BodyPhysics {
    BodyPhysics {
        vehicle: Some(VehiclePhysics::default()),
        friction_permille: friction,
        ..BodyPhysics::default()
    }
}
fn scenery(friction: u16) -> BodyPhysics {
    BodyPhysics {
        mode: BodyMode::Static,
        friction_permille: friction,
        ..BodyPhysics::default()
    }
}
fn boot(floor_friction: u16, extra: &[(Sigil, BodyPhysics)]) -> CommitKernel {
    let mut canon = cook_diffs(&from_ron::<Vec<CanonDiff>>("[]").unwrap()).unwrap();
    assert!(canon.bind_physics(chassis(), rig(900)));
    assert!(canon.bind_physics(id(LocusKind::Place, 9), scenery(floor_friction)));
    for &(s, p) in extra {
        assert!(canon.bind_physics(s, p));
    }
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    plant(&mut k, chassis(), chassis_hull(), pose(0, 200, 0));
    plant(
        &mut k,
        id(LocusKind::Place, 9),
        box_hull(20_000, 200, 20_000),
        pose(0, -200, 0),
    );
    k
}
fn command(k: &mut CommitKernel, throttle: i32, brake: i32, steer: i32) {
    k.world_mut()
        .set_phys_req(
            chassis(),
            PhysRequest {
                lin: IVec3 {
                    x: 0,
                    y: brake,
                    z: throttle,
                },
                ang: IVec3 {
                    x: 0,
                    y: steer,
                    z: 0,
                },
            },
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

#[test]
fn vehicle_settles_on_flat_ground_without_scripted_translation() {
    let mut k = boot(900, &[]);
    let z0 = k.world().view().pose(chassis()).unwrap().z.0;
    for t in 1..=40 {
        tick(&mut k, t);
    }
    let p = k.world().view().pose(chassis()).unwrap();
    assert!(p.y.0 > 50 && p.y.0 < 350, "{p:?}");
    assert!((p.z.0 - z0).abs() < 40, "scripted translation: {p:?}");
    assert!(k.world().view().support(chassis()).is_some());
}

#[test]
fn vehicle_accelerate_coast_brake_and_reverse() {
    let mut k = boot(900, &[]);
    for t in 1..=12 {
        tick(&mut k, t);
    }
    command(&mut k, 40, 0, 0);
    for t in 13..=40 {
        tick(&mut k, t);
        if t < 40 {
            command(&mut k, 40, 0, 0);
        }
    }
    let v_fwd = k.world().view().vel(chassis()).unwrap().0.z.to_mm_trunc().0;
    let z_fwd = k.world().view().pose(chassis()).unwrap().z.0;
    assert!(v_fwd > 5 && z_fwd > 80, "accel v={v_fwd} z={z_fwd}");

    for t in 41..=55 {
        tick(&mut k, t);
    }
    let v_coast = k.world().view().vel(chassis()).unwrap().0.z.to_mm_trunc().0;
    assert!(v_coast > 0, "coast should retain forward speed: {v_coast}");

    command(&mut k, 0, 40, 0);
    for t in 56..=90 {
        tick(&mut k, t);
        if t < 90 {
            command(&mut k, 0, 40, 0);
        }
    }
    let v_brake = k.world().view().vel(chassis()).unwrap().0.z.to_mm_trunc().0;
    assert!(v_brake < v_coast, "brake v={v_brake} coast={v_coast}");

    let mut reverse = boot(900, &[]);
    for t in 1..=8 {
        tick(&mut reverse, t);
    }
    command(&mut reverse, -40, 0, 0);
    for t in 9..=40 {
        tick(&mut reverse, t);
        if t < 40 {
            command(&mut reverse, -40, 0, 0);
        }
    }
    let z_rev = reverse.world().view().pose(chassis()).unwrap().z.0;
    assert!(z_rev < -40, "reverse {z_rev}");
}

#[test]
fn vehicle_steer_yaws_and_displaces_laterally() {
    let mut k = boot(900, &[]);
    for t in 1..=8 {
        tick(&mut k, t);
    }
    command(&mut k, 40, 0, 15_000);
    for t in 9..=50 {
        tick(&mut k, t);
        if t < 50 {
            command(&mut k, 40, 0, 15_000);
        }
    }
    let p = k.world().view().pose(chassis()).unwrap();
    assert!(p.yaw.0.abs() > 1_000, "steer yaw {}", p.yaw.0);
    assert!(p.x.0.abs() > 20, "steer x {}", p.x.0);
}

#[test]
fn vehicle_loses_lateral_grip_at_frozen_excess_speed() {
    fn sideslip(speed: i32) -> i32 {
        let mut k = boot(900, &[]);
        for t in 1..=8 {
            tick(&mut k, t);
        }
        k.world_mut()
            .set_vel(
                chassis(),
                Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(speed)),
                0,
            )
            .unwrap();
        command(&mut k, 0, 0, 25_000);
        for t in 9..=24 {
            tick(&mut k, t);
            if t < 24 {
                command(&mut k, 0, 0, 25_000);
            }
        }
        let p = k.world().view().pose(chassis()).unwrap();
        let (v, _) = k.world().view().vel(chassis()).unwrap();
        let heading = klotho_core::rotate_xz(
            IVec3 {
                x: 0,
                y: 0,
                z: 1_000,
            },
            p.yaw,
        );
        let vx = v.x.to_mm_trunc().0;
        let vz = v.z.to_mm_trunc().0;
        (vx * heading.z - vz * heading.x).abs()
    }
    let slow = sideslip(8);
    let fast = sideslip(80);
    assert!(fast > slow + 1_000, "slow={slow} fast={fast}");
}

#[test]
fn vehicle_high_friction_out_accelerates_low_friction() {
    fn travel(mu: u16) -> i32 {
        let mut k = boot(mu, &[]);
        for t in 1..=8 {
            tick(&mut k, t);
        }
        command(&mut k, 40, 0, 0);
        for t in 9..=36 {
            tick(&mut k, t);
            if t < 36 {
                command(&mut k, 40, 0, 0);
            }
        }
        k.world().view().pose(chassis()).unwrap().z.0
    }
    let high = travel(900);
    let low = travel(150);
    assert!(high > low + 40, "high={high} low={low}");
}

#[test]
fn vehicle_settles_on_permitted_slope_and_slides_on_ice() {
    fn run(mu: u16) -> PoseMm {
        let ramp = id(LocusKind::Place, 3);
        let mut k = boot(
            mu,
            &[(
                ramp,
                BodyPhysics {
                    mode: BodyMode::Static,
                    shape: ShapeKind::Heightfield,
                    friction_permille: mu,
                    ..BodyPhysics::default()
                },
            )],
        );
        plant(
            &mut k,
            ramp,
            AabbMm::new(
                IVec3 {
                    x: -4_000,
                    y: 0,
                    z: 0,
                },
                IVec3 {
                    x: 4_000,
                    y: 804,
                    z: 3_000,
                },
            ),
            pose(0, 0, 0),
        );
        k.world_mut()
            .set_pose(chassis(), pose(0, 572, 1_500))
            .unwrap();
        for t in 1..=50 {
            tick(&mut k, t);
        }
        k.world().view().pose(chassis()).unwrap()
    }
    let grip = run(900);
    let ice = run(100);
    assert!(
        (grip.z.0 - 1_500).abs() < 400 && grip.y.0 > 250,
        "grip {grip:?}"
    );
    assert!(ice.z.0 < grip.z.0 - 80, "ice {ice:?} grip {grip:?}");
}

#[test]
fn driver_attach_and_detach_do_not_compete_for_pose() {
    let mut k = boot(900, &[]);
    plant(
        &mut k,
        driver(),
        box_hull(200, 1_800, 200),
        pose(1_000, 0, 0),
    );
    k.world_mut()
        .add_rel(driver(), Rel::PilotedBy, chassis())
        .unwrap();
    k.partition();
    let mut out = AdmitBuf::new();
    Motion::hearth().propose(&k.world().view(), Tick(1), &mut out);
    assert!(
        out.drain()
            .iter()
            .all(|p| !matches!(p, klotho_commit::Proposal::MotionDelta { mover, .. } if *mover == driver())),
        "attached driver must not emit MotionDelta"
    );
    command(&mut k, 40, 0, 0);
    tick(&mut k, 1);
    let vehicle_pose = k.world().view().pose(chassis()).unwrap();
    let seat = k.world().view().pose(driver()).unwrap();
    assert_eq!(seat.yaw, vehicle_pose.yaw);
    k.world_mut()
        .del_rel(driver(), Rel::PilotedBy, chassis())
        .unwrap();
    k.partition();
    let mut out = AdmitBuf::new();
    Motion::hearth().propose(&k.world().view(), Tick(1), &mut out);
    assert!(
        out.drain().iter().any(
            |p| matches!(p, klotho_commit::Proposal::MotionDelta { mover, .. } if *mover == driver())
        ) || k.world().view().attach_parent(driver()).is_none()
    );
}

#[test]
fn vehicle_save_roundtrip_resumes_the_same_physical_state() {
    let mut original = boot(900, &[]);
    for t in 1..=8 {
        tick(&mut original, t);
    }
    command(&mut original, 40, 0, 8_000);
    for t in 9..=18 {
        tick(&mut original, t);
        if t < 18 {
            command(&mut original, 40, 0, 8_000);
        }
    }
    let saved = klotho_save::pause_save(&original.snapshot()).unwrap();
    let loaded = klotho_save::decode(&klotho_save::encode(&saved).unwrap()).unwrap();
    let restored = klotho_save::restore(&loaded, saved.prefix, saved.canon_hash).unwrap();
    let mut resumed = boot(900, &[]);
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
    command(&mut original, 40, 0, 8_000);
    command(&mut resumed, 40, 0, 8_000);
    for t in 19..=32 {
        tick(&mut original, t);
        tick(&mut resumed, t);
        if t < 32 {
            command(&mut original, 40, 0, 8_000);
            command(&mut resumed, 40, 0, 8_000);
        }
        assert_eq!(
            original.world().view().pose(chassis()),
            resumed.world().view().pose(chassis())
        );
        assert_eq!(
            original.world().view().vel(chassis()),
            resumed.world().view().vel(chassis())
        );
    }
    assert_eq!(
        original.snapshot().encode().unwrap(),
        resumed.snapshot().encode().unwrap()
    );
}

#[test]
fn vehicle_one_and_eight_workers_preserve_trace_and_projection() {
    fn run(workers: usize) -> (Hash, Vec<u8>) {
        let mut k = boot(900, &[]);
        for n in 2..=9 {
            plant(
                &mut k,
                id(LocusKind::Relic, n),
                box_hull(100, 200, 100),
                pose(n as i32 * 4_000, 500, 0),
            );
        }
        command(&mut k, 40, 0, 0);
        for t in 1..=24 {
            let islands = k.partition();
            if t == 1 {
                assert!(islands.len() >= 8);
            }
            struct Jobs(usize);
            impl SyncProposer for Jobs {
                fn name(&self) -> &'static str {
                    "vehicle-test-jobs"
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
            if t < 24 {
                command(&mut k, 40, 0, 0);
            }
        }
        (
            k.world().trace_prefix_hash(),
            k.snapshot().encode().unwrap(),
        )
    }
    assert_eq!(run(1), run(8));
}

#[test]
fn vehicle_query_overflow_emits_no_island() {
    let mut statics = Vec::new();
    for i in 0..513 {
        statics.push(crate::solver::Occupancy {
            sigil: id(LocusKind::Relic, 100 + i as u128),
            local: box_hull(10, 10, 10),
            kind: ShapeKind::OrientedBox,
            pose: pose(i as i32 * 20, 0, 0),
            friction: 0.9,
            restitution: 0.0,
        });
    }
    assert!(crate::vehicle::prepare_vehicles(&statics).is_err());
}

#[test]
fn presented_wheel_rate_derives_from_admitted_chassis_velocity() {
    let mut k = boot(900, &[]);
    k.world_mut()
        .set_vel(
            chassis(),
            Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(40)),
            0,
        )
        .unwrap();
    let rate = k.world().view().presented_wheel_rate_md(chassis()).unwrap();
    assert!(rate > 0, "{rate}");
}
