//! Bounded combined Phys golden. All authoritative changes pass through CommitKernel.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use klotho_canon::cook_diffs;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{
    AabbMm, BlobId, BodyMode, BodyPhysics, Budget, CharacterPhysics, ConstraintKind,
    ConstraintPhysics, ContactSocket, ContactSweep, ContactTrack, Hash, IVec3, LocusKind, Mm,
    PlayerId, PoseMm, ShapeKind, Sigil, Tick, Vel3, VelFx, YawMd,
};
use klotho_ir::{
    Agency, Analog, CanonDiff, Channel, IntentTarget, PlayerIntent, Rel, Verb, from_ron,
};
use klotho_phys::Phys;
use klotho_trace::TraceDelta;
use klotho_world::World;

const DIFFS: &str = r#"[
 AddAffordance(Affordance(id:"Hittable",requires:[],grants:[],conflicts:[])),
 AddAffordance(Affordance(id:"Fragment",requires:[],grants:[],conflicts:[])),
 AddRite(RiteGraph(id:"melee",cap_steps:16,cap_ticks:8,entry:0,nodes:[
   {pc:0,op:Bind(Target)}, {pc:1,op:Wait(2,Some(Aim))},
   {pc:2,op:Emit("Hit")}, {pc:3,op:Complete(Success)}])),
 AddRite(RiteGraph(id:"apply_hit",cap_steps:8,cap_ticks:4,entry:0,nodes:[
   {pc:0,op:Spend("health",25,2)}, {pc:1,op:Complete(Success)},
   {pc:2,op:Complete(Fail)}]))
]"#;

fn id(kind: LocusKind, n: u128) -> Sigil {
    Sigil::pack(kind, 0, n).expect("bounded Anvil id")
}
fn actor() -> Sigil {
    id(LocusKind::Actor, 1)
}
fn target() -> Sigil {
    id(LocusKind::Relic, 2)
}
fn push_actor() -> Sigil {
    id(LocusKind::Actor, 3)
}
fn push_crate() -> Sigil {
    id(LocusKind::Relic, 4)
}
fn platform() -> Sigil {
    id(LocusKind::Relic, 5)
}
fn rider() -> Sigil {
    id(LocusKind::Actor, 6)
}
fn hinge_a() -> Sigil {
    id(LocusKind::Relic, 30)
}
fn hinge_b() -> Sigil {
    id(LocusKind::Relic, 31)
}
fn break_a() -> Sigil {
    id(LocusKind::Relic, 40)
}
fn break_b() -> Sigil {
    id(LocusKind::Relic, 41)
}

fn point(x: i32, y: i32, z: i32) -> IVec3 {
    IVec3 { x, y, z }
}
fn pose(x: i32, y: i32, z: i32) -> PoseMm {
    PoseMm::new(Mm(x), Mm(y), Mm(z), YawMd::ZERO)
}
fn box_hull(x: i32, y: i32, z: i32) -> AabbMm {
    AabbMm::new(point(-x, 0, -z), point(x, y, z))
}
fn plant(k: &mut CommitKernel, s: Sigil, hull: AabbMm, at: PoseMm) {
    let mut w = k.world_mut();
    w.insert_locus(s, s.kind().expect("kind")).expect("locus");
    w.set_hull(s, hull, BlobId::from_bytes([s.id() as u8; 32]))
        .expect("hull");
    w.set_pose(s, at).expect("pose");
}
fn track() -> ContactTrack {
    let p = point;
    ContactTrack {
        skeleton: Hash::from_bytes([1; 32]),
        instrument: Hash::from_bytes([2; 32]),
        action: Hash::from_bytes([3; 32]),
        rite: "melee".into(),
        tick_hz: 30,
        wait_pc: 1,
        channel: Channel::Aim.as_u8(),
        wait_ticks: 2,
        roots: vec![p(0, 0, 0); 3],
        sockets: vec![ContactSocket {
            name: "grip".into(),
            samples: vec![p(0, 900, 700); 3],
        }],
        sweeps: vec![ContactSweep {
            name: "blade".into(),
            socket: "grip".into(),
            radius_mm: 60,
            samples: vec![
                [p(-600, 900, 700), p(-600, 1300, 700)],
                [p(600, 900, 700), p(600, 1300, 700)],
                [p(-600, 900, 700), p(-600, 1300, 700)],
            ],
        }],
        plants: vec![],
    }
}

/// Cook and seed the combined Anvil scene. No title content or bespoke component types.
#[must_use]
pub fn boot() -> CommitKernel {
    let mut canon =
        cook_diffs(&from_ron::<Vec<CanonDiff>>(DIFFS).expect("Anvil diffs")).expect("Anvil Canon");
    for s in [actor(), push_actor(), rider()] {
        assert!(canon.bind_physics(
            s,
            BodyPhysics {
                shape: ShapeKind::Capsule,
                character: Some(CharacterPhysics::default()),
                ..BodyPhysics::default()
            }
        ));
    }
    assert!(canon.bind_physics(
        target(),
        BodyPhysics {
            mode: BodyMode::Static,
            ..BodyPhysics::default()
        }
    ));
    assert!(canon.bind_physics(
        platform(),
        BodyPhysics {
            mode: BodyMode::Kinematic,
            ..BodyPhysics::default()
        }
    ));
    for (s, height) in [
        (id(LocusKind::Place, 2), 250),
        (id(LocusKind::Place, 3), 577),
        (id(LocusKind::Place, 4), 1192),
        (id(LocusKind::Place, 5), 200),
    ] {
        assert!(canon.bind_physics(
            s,
            BodyPhysics {
                mode: BodyMode::Static,
                shape: if height == 250 || height == 200 {
                    ShapeKind::OrientedBox
                } else {
                    ShapeKind::Heightfield
                },
                ..BodyPhysics::default()
            }
        ));
    }
    let contact = track();
    assert!(contact.is_valid());
    canon.contact_tracks.insert(actor(), contact);
    assert!(canon.bind_constraint(
        id(LocusKind::Relic, 90),
        ConstraintPhysics {
            kind: ConstraintKind::Hinge,
            a: hinge_a(),
            b: hinge_b(),
            anchor_a: point(200, 200, 0),
            anchor_b: point(-200, 200, 0),
            limit_md: 90_000,
            binding: BlobId::from_bytes([90; 32]),
            ..ConstraintPhysics::default()
        }
    ));
    assert!(canon.bind_constraint(
        id(LocusKind::Relic, 91),
        ConstraintPhysics {
            kind: ConstraintKind::Fixed,
            a: break_a(),
            b: break_b(),
            anchor_a: point(200, 200, 0),
            anchor_b: point(-200, 200, 0),
            break_impulse: 1,
            fragments: 2,
            binding: BlobId::from_bytes([91; 32]),
            ..ConstraintPhysics::default()
        }
    ));
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    k.bind_player(PlayerId(0), actor());
    plant(
        &mut k,
        id(LocusKind::Place, 1),
        AabbMm::new(point(-50_000, -200, -50_000), point(50_000, 0, 50_000)),
        pose(0, 0, 0),
    );
    plant(
        &mut k,
        id(LocusKind::Place, 2),
        box_hull(600, 250, 500),
        pose(-3000, 0, 1400),
    );
    plant(
        &mut k,
        id(LocusKind::Place, 3),
        AabbMm::new(point(-1000, 0, 0), point(1000, 577, 1000)),
        pose(-1000, 0, 3000),
    );
    plant(
        &mut k,
        id(LocusKind::Place, 4),
        AabbMm::new(point(-1000, 0, 0), point(1000, 1192, 1000)),
        pose(1000, 0, 3000),
    );
    plant(
        &mut k,
        id(LocusKind::Place, 5),
        box_hull(600, 200, 500),
        pose(-3000, 0, 2600),
    );
    for i in 0..5 {
        let s = id(LocusKind::Relic, 10 + i);
        let mut at = pose(-7000, i as i32 * 400, 0);
        if i == 2 {
            at.yaw = YawMd(15_000);
            at.roll = YawMd(5_000);
        }
        plant(&mut k, s, box_hull(200, 400, 200), at);
    }
    plant(
        &mut k,
        actor(),
        box_hull(300, 1800, 300),
        pose(-12_000, 0, 0),
    );
    plant(
        &mut k,
        target(),
        box_hull(100, 1800, 100),
        pose(-12_000, 0, 700),
    );
    plant(
        &mut k,
        push_actor(),
        box_hull(300, 1800, 300),
        pose(0, 0, 0),
    );
    plant(
        &mut k,
        push_crate(),
        box_hull(200, 400, 200),
        pose(0, 0, 650),
    );
    plant(
        &mut k,
        platform(),
        box_hull(1000, 200, 1000),
        pose(4000, 0, 0),
    );
    plant(
        &mut k,
        rider(),
        box_hull(300, 1800, 300),
        pose(4500, 200, 0),
    );
    for (s, x) in [
        (hinge_a(), 8000),
        (hinge_b(), 8400),
        (break_a(), 12000),
        (break_b(), 13200),
    ] {
        plant(&mut k, s, box_hull(200, 400, 200), pose(x, 400, 0));
    }
    let health = k.canon().resource_id("health").expect("health");
    let hittable = k.canon().affordance_id("Hittable").expect("Hittable");
    k.world_mut()
        .set_qty(target(), health, 100)
        .expect("health");
    k.world_mut()
        .set_affordance(target(), hittable, true)
        .expect("hittable");
    k.world_mut()
        .add_rel(break_a(), Rel::PartOf, break_b())
        .expect("structure");
    k.world_mut()
        .set_vel(
            push_actor(),
            Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20)),
            0,
        )
        .expect("drive");
    k.world_mut()
        .set_vel(
            platform(),
            Vel3::new(VelFx::from_mm_per_tick(100), VelFx::ZERO, VelFx::ZERO),
            1000,
        )
        .expect("platform");
    k
}

/// Start the player-authorized sword action.
pub fn sword(k: &mut CommitKernel) -> TraceDelta {
    k.ingest(Proposal::Player(PlayerIntent {
        player: PlayerId(0),
        at: k.world().tick(),
        verb: Verb::Use,
        target: IntentTarget::Sigil(target()),
        analog: Analog::default(),
        agency: Agency {
            assist: klotho_ir::AssistLevel::None,
            claimed: vec![Channel::Aim],
        },
    }));
    k.step(Tick(1), Budget::AAA_ADVENTURE, &mut [])
        .expect("kernel")
}

/// Advance all current physics islands one authoritative tick.
pub fn tick(k: &mut CommitKernel) -> TraceDelta {
    k.partition();
    k.step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Phys])
        .expect("kernel")
}

/// Key semantic actors and bodies used by integration assertions.
#[must_use]
pub fn named(name: &str) -> Sigil {
    match name {
        "actor" => actor(),
        "target" => target(),
        "push_actor" => push_actor(),
        "push_crate" => push_crate(),
        "platform" => platform(),
        "rider" => rider(),
        "break_a" => break_a(),
        "break_b" => break_b(),
        "stack_middle" => id(LocusKind::Relic, 12),
        "stair_200" => id(LocusKind::Place, 5),
        "stair_250" => id(LocusKind::Place, 2),
        "slope_30" => id(LocusKind::Place, 3),
        "slope_50" => id(LocusKind::Place, 4),
        _ => panic!("unknown Anvil name"),
    }
}
