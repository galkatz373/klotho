//! Drift goldens: AABB floor, possess, K55, yaw-only seat, seam, locked door.

use std::sync::Arc;
use std::time::Instant;

use drift_slice::{boot, pin, replay};
use klotho_canon::{cook, cook_diffs};
use klotho_commit::{
    CommitKernel, METRIC_RESIDENCY_ROWS_APPLIED, METRIC_STREAM_HITCH_US, Proposal, ResidencyOp,
};
use klotho_core::{
    AabbMm, BlobId, Budget, Hash, HullWitness, IVec3, LocusKind, Mm, NO_ISLAND, PoseMm,
    RejectReason, Sigil, Tick, Vel3, VelFx, YawMd, rotate_xz,
};
use klotho_ir::{CanonDiff, Rel, from_ron};
use klotho_motion::{ClipSet, Motion};
use klotho_trace::{ProposalKind, TraceBody};
use klotho_world::{PlaceRow, PlaceSnap, World};

fn intents(src: &str) -> Vec<klotho_ir::PlayerIntent> {
    from_ron(src).expect("PlayerIntent RON")
}

fn door() -> Sigil {
    Sigil::pack(LocusKind::Relic, 0, 100).expect("door")
}

fn place_b_floor() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -50_000,
            y: -200,
            z: 0,
        },
        IVec3 {
            x: 50_000,
            y: 0,
            z: 50_000,
        },
    )
}

fn door_hull() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -400,
            y: 0,
            z: -50,
        },
        IVec3 {
            x: 400,
            y: 2000,
            z: 50,
        },
    )
}

fn place_a_floor() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -50_000,
            y: -200,
            z: -50_000,
        },
        IVec3 {
            x: 50_000,
            y: 0,
            z: 0,
        },
    )
}

fn yaw_only(parent: PoseMm, local: IVec3) -> PoseMm {
    let r = rotate_xz(local, parent.yaw);
    PoseMm {
        x: Mm(parent.x.0.wrapping_add(r.x)),
        y: Mm(parent.y.0.wrapping_add(r.y)),
        z: Mm(parent.z.0.wrapping_add(r.z)),
        yaw: parent.yaw,
        pitch: parent.pitch,
        roll: parent.roll,
    }
}

fn place_b_snap(k: &CommitKernel, with_door: bool) -> PlaceSnap {
    let p = pin(k, "place_b");
    let mut place_row = PlaceRow::new(p, LocusKind::Place);
    place_row.pose = Some(PoseMm::default());
    place_row.hull = Some(place_b_floor());
    let mut rows = vec![place_row];
    if with_door {
        let opaque = k.canon().affordance_id("Opaque").expect("Opaque");
        let mut door_row = PlaceRow::new(door(), LocusKind::Relic);
        door_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(1850), YawMd::ZERO));
        door_row.hull = Some(door_hull());
        door_row.afford = 1u64 << opaque.0;
        door_row.rels = vec![(Rel::In, p), (Rel::LockedBy, door())];
        rows.push(door_row);
    }
    PlaceSnap::new(
        p,
        k.world().canon_hash(),
        k.world().trace_prefix_hash(),
        rows,
    )
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

fn load_place_b(k: &mut CommitKernel, with_door: bool) -> klotho_trace::TraceDelta {
    let p = pin(k, "place_b");
    let snap = place_b_snap(k, with_door);
    k.ingest(residency(k, p, ResidencyOp::Load, snap));
    k.step(Tick(1), Budget::AAA_ADVENTURE, &mut [])
        .expect("load")
}

fn phys_delta(mover: Sigil, pose: PoseMm, hull: BlobId, hint: bool) -> Proposal {
    Proposal::PhysDelta {
        mover,
        pose,
        vel: Vel3::ZERO,
        yaw_rate: 0,
        pitch_rate: 0,
        roll_rate: 0,
        island: 0,
        sleep_ticks: 0,
        hull,
        witness: HullWitness::new(mover, pose, hint),
        support: None,
    }
}

fn possess(k: &mut CommitKernel) {
    let ds = replay(
        k,
        &intents(include_str!("../fixtures/golden_02_possess.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
}

#[test]
fn golden_01_two_places_aabb_floor_is_not_a_body() {
    let mut k = boot();
    let a = pin(&k, "place_a");
    let b = pin(&k, "place_b");
    assert_eq!(a.kind(), Some(LocusKind::Place));
    assert_eq!(b.kind(), Some(LocusKind::Place));
    assert!(k.world().view().contains(a));
    assert!(!k.world().view().contains(b));
    let d = load_place_b(&mut k, false);
    assert!(d.rejects.is_empty(), "{d:?}");
    assert!(k.world().view().contains(b));
    assert_eq!(k.world().view().hull(a), Some(place_a_floor()));
    assert_eq!(k.world().view().hull(b), Some(place_b_floor()));
    k.partition();
    assert_eq!(
        k.world().view().island(a).map(|(id, _)| id),
        Some(NO_ISLAND)
    );
    assert_eq!(
        k.world().view().island(b).map(|(id, _)| id),
        Some(NO_ISLAND)
    );
}

#[test]
fn golden_02_possess_piloted_by_one_driver() {
    let mut k = boot();
    let player = pin(&k, "player");
    let vehicle = pin(&k, "vehicle");
    let driveable = k.canon().affordance_id("Driveable").expect("Driveable");
    assert!(k.world().view().has_affordance(vehicle, driveable));
    assert!(!k.world().view().has_rel(player, Rel::PilotedBy, vehicle));
    possess(&mut k);
    assert!(k.world().view().has_rel(player, Rel::PilotedBy, vehicle));
    let pilots: Vec<_> = k
        .world()
        .view()
        .loci()
        .filter(|&s| k.world().view().has_rel(s, Rel::PilotedBy, vehicle))
        .collect();
    assert_eq!(pilots, vec![player]);
    assert!(k.world().view().loci().all(|s| !k.world().view().has_rel(
        s,
        Rel::AttachedTo,
        vehicle
    )));
}

#[test]
fn golden_03_possess_at_t_nacks_motion_root() {
    let mut k = boot();
    let player = pin(&k, "player");
    let vehicle = pin(&k, "vehicle");
    k.world_mut()
        .set_vel(
            player,
            Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20)),
            0,
        )
        .unwrap();
    let start = k.world().view().pose(player).unwrap();
    let vehicle_next = PoseMm::new(Mm(10), Mm(50), Mm(0), YawMd::ZERO);
    let mut possess_pi = intents(include_str!("../fixtures/golden_02_possess.ron"))
        .into_iter()
        .next()
        .expect("possess");
    possess_pi.at = k.world().tick();
    k.ingest(Proposal::Player(possess_pi));
    k.ingest(phys_delta(vehicle, vehicle_next, BlobId::ZERO, false));
    let mut motion = Motion::with_clips(ClipSet::walk_mm(20));
    let d = k
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut motion])
        .unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(kind, r)| *kind == ProposalKind::Motion && *r == RejectReason::Conflict),
        "{d:?}"
    );
    assert!(k.world().view().has_rel(player, Rel::PilotedBy, vehicle));
    let local = k.world().view().attach_local(player).expect("seat");
    let got = k.world().view().pose(player).unwrap();
    assert_eq!(got, yaw_only(vehicle_next, local));
    assert_ne!(got.z, start.z.wrapping_add(Mm(20)));
    assert_eq!(k.world().view().pose(vehicle).unwrap(), vehicle_next);
}

#[test]
fn golden_04_yaw_only_seat_compose() {
    let mut k = boot();
    let player = pin(&k, "player");
    let vehicle = pin(&k, "vehicle");
    possess(&mut k);
    let local = k.world().view().attach_local(player).expect("seat");
    let mut next = PoseMm::new(Mm(10), Mm(50), Mm(0), YawMd(YawMd::QUARTER_TURN));
    next.pitch = YawMd(1_000);
    next.roll = YawMd(2_000);
    k.ingest(phys_delta(vehicle, next, BlobId::ZERO, false));
    let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(d.rejects.is_empty(), "{d:?}");
    let got = k.world().view().pose(player).unwrap();
    assert_eq!(got, yaw_only(next, local));
    assert_eq!(got.yaw, next.yaw);
    assert_eq!(got.pitch, next.pitch);
    assert_eq!(got.roll, next.roll);
}

#[test]
fn golden_05_seam_load_evict() {
    let mut k = boot();
    let b = pin(&k, "place_b");
    assert!(!k.world().view().contains(b));
    let loaded = load_place_b(&mut k, true);
    assert!(loaded.rejects.is_empty(), "{loaded:?}");
    let n = place_b_snap(&k, true).len() as u32;
    assert!(
        loaded.events.iter().any(|e| matches!(
            e.body,
            TraceBody::PlaceLoaded { place, n: got } if place == b && got == n
        )),
        "{loaded:?}"
    );
    assert!(k.world().view().contains(b));
    assert!(k.world().view().contains(door()));
    k.ingest(residency(&k, b, ResidencyOp::Evict, place_b_snap(&k, true)));
    let evicted = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(evicted.rejects.is_empty(), "{evicted:?}");
    assert!(
        evicted
            .events
            .iter()
            .any(|e| matches!(e.body, TraceBody::PlaceEvicted { place } if place == b)),
        "{evicted:?}"
    );
    assert!(k.world().view().contains(b));
    assert!(!k.world().view().contains(door()));
}

#[test]
fn golden_06_locked_door_in_b_blocks_when_loaded() {
    let mut k = boot();
    let vehicle = pin(&k, "vehicle");
    let loaded = load_place_b(&mut k, true);
    assert!(loaded.rejects.is_empty(), "{loaded:?}");
    assert!(k.world().view().opaque_closed(door()));
    k.world_mut()
        .set_pose(vehicle, PoseMm::new(Mm(0), Mm(0), Mm(1400), YawMd::ZERO))
        .unwrap();
    let blocked_at = PoseMm::new(Mm(0), Mm(0), Mm(1900), YawMd::ZERO);
    k.ingest(phys_delta(vehicle, blocked_at, BlobId::ZERO, true));
    let blocked = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(
        blocked
            .rejects
            .iter()
            .any(|(_, r)| matches!(r, RejectReason::Law(_))),
        "{blocked:?}"
    );
    assert_eq!(k.world().view().pose(vehicle).unwrap().z, Mm(1400));

    k.ingest(residency(
        &k,
        pin(&k, "place_b"),
        ResidencyOp::Evict,
        place_b_snap(&k, true),
    ));
    let evicted = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(evicted.rejects.is_empty(), "{evicted:?}");
    assert!(!k.world().view().contains(door()));
    k.ingest(phys_delta(vehicle, blocked_at, BlobId::ZERO, false));
    let open = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(open.rejects.is_empty(), "{open:?}");
    assert_eq!(k.world().view().pose(vehicle).unwrap().z, Mm(1900));
}

#[test]
fn golden_07_phys_off_vehicle_does_not_move() {
    let mut k = boot();
    let player = pin(&k, "player");
    let vehicle = pin(&k, "vehicle");
    possess(&mut k);
    let before = k.world().view().pose(vehicle).unwrap();
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_07_steer.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert_eq!(k.world().view().pose(vehicle).unwrap(), before);
    assert!(k.world().view().phys_req(player).is_some());
}

#[test]
fn golden_08_residency_metrics() {
    let mut k = boot();
    let b = pin(&k, "place_b");
    let snap = place_b_snap(&k, true);
    let expect_n = u32::try_from(snap.len()).expect("rows");
    k.ingest(residency(&k, b, ResidencyOp::Load, snap));
    let t0 = Instant::now();
    let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    let hitch_us = t0.elapsed().as_micros();
    assert!(d.rejects.is_empty(), "{d:?}");
    let got = d.events.iter().find_map(|e| match e.body {
        TraceBody::PlaceLoaded { place, n } if place == b => Some(n),
        _ => None,
    });
    assert_eq!(
        (METRIC_RESIDENCY_ROWS_APPLIED, got),
        ("klotho.residency.rows_applied", Some(expect_n)),
        "{d:?}"
    );
    assert_eq!(METRIC_STREAM_HITCH_US, "klotho.stream.hitch_us");
    assert!(
        hitch_us < 60_000_000u128,
        "{METRIC_STREAM_HITCH_US} hitch_us={hitch_us}"
    );
}

#[test]
fn golden_09_same_kernel_binary_as_hearth() {
    let _drift = boot();
    let hearth: Vec<CanonDiff> = from_ron(include_str!(
        "../../../crates/klotho-canon/fixtures/hearth_diffs.ron"
    ))
    .unwrap();
    let canon = cook_diffs(&hearth).unwrap();
    let _hearth = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    let drift_doc = drift_slice::drift_doc();
    let _ = cook(&drift_doc).unwrap();
    let _: fn(World) -> CommitKernel = CommitKernel::new;
}
