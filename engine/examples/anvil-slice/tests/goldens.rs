//! Combined Anvil acceptance.
use anvil_slice::{boot, named, sword, tick};
use klotho_core::{Budget, RejectReason, Tick};
use klotho_phys::{capture_island, replay_capture, summarize_timings};
use klotho_trace::TraceBody;

#[test]
fn frozen_geometry_and_hinge_are_present_in_one_canon() {
    let k = boot();
    let view = k.world().view();
    let middle = view.pose(named("stack_middle")).unwrap();
    assert_eq!((middle.yaw.0, middle.roll.0), (15_000, 5_000));
    assert_eq!(view.hull(named("stair_200")).unwrap().max.y, 200);
    assert_eq!(view.hull(named("stair_250")).unwrap().max.y, 250);
    assert_eq!(view.hull(named("slope_30")).unwrap().max.y, 577);
    assert_eq!(view.hull(named("slope_50")).unwrap().max.y, 1192);
    assert!(
        view.constraints()
            .any(|(_, c)| c.kind == klotho_core::ConstraintKind::Hinge)
    );
}

#[test]
fn combined_scene_admits_contact_push_platform_and_break() {
    let mut k = boot();
    let actor = named("actor");
    let target = named("target");
    let pushed = named("push_crate");
    let platform = named("platform");
    let rider = named("rider");
    let start = sword(&mut k);
    assert!(start.rejects.is_empty(), "{start:?}");
    let mut breaks = 0;
    let mut hits = 0;
    for _ in 0..30 {
        let delta = tick(&mut k);
        assert!(delta.rejects.is_empty(), "{delta:?}");
        breaks += delta
            .events
            .iter()
            .filter(|e| matches!(e.body, TraceBody::ConstraintBroken { .. }))
            .count();
        hits += delta.events.iter().filter(|e| matches!(e.body, TraceBody::Emitted { a, b: Some(b), .. } if a == actor && b == target)).count();
    }
    assert_eq!(breaks, 1);
    assert_eq!(hits, 1);
    assert_eq!(
        k.world()
            .view()
            .qty(target, k.canon().resource_id("health").unwrap()),
        75
    );
    assert!(k.world().view().pose(pushed).unwrap().z.0 > 650);
    assert!(k.world().view().pose(platform).unwrap().x.0 > 4000);
    assert!(k.world().view().pose(rider).unwrap().x.0 > 4500);
}

#[test]
fn snapshot_capture_replays_exact_proposal_and_reject_explains_member() {
    let mut k = boot();
    k.partition();
    let s = named("push_actor");
    let island = k.world().view().island(s).unwrap().0;
    let capture = capture_island(k.snapshot(), island).expect("island capture");
    assert!(replay_capture(&capture).is_ok());
    let perf = summarize_timings(&[capture.timings()]).unwrap();
    assert_eq!(perf.max_bodies, 2);
    assert_eq!(perf.samples, 1);
    let actor_overlay = capture.bodies().into_iter().find(|b| b.sigil == s).unwrap();
    assert_eq!(actor_overlay.desired_root.unwrap().z, 20);
    assert_eq!(actor_overlay.step_mm, Some(250));
    let mut bad = capture.proposal().clone();
    if let klotho_commit::Proposal::PhysIsland { bodies, tick, .. } = &mut bad {
        *tick = Tick(k.world().tick().0 + 1);
        bodies
            .iter_mut()
            .find(|b| b.mover == named("push_crate"))
            .unwrap()
            .hull = klotho_core::BlobId::from_bytes([77; 32]);
    } else {
        panic!("expected physical island")
    }
    let mut before = k.snapshot().encode().unwrap();
    k.ingest(bad);
    let d = k.step(Tick(1), Budget::AAA_ADVENTURE, &mut []).unwrap();
    assert!(
        d.rejects.iter().any(|(_, r)| *r == RejectReason::WrongHull),
        "{d:?}"
    );
    let mut after = k.snapshot().encode().unwrap();
    before[16..24].fill(0);
    after[16..24].fill(0);
    assert_eq!(before, after);
}

#[test]
fn active_weapon_and_constraint_overlays_are_read_only() {
    let mut k = boot();
    sword(&mut k);
    k.partition();
    let actor_island = k.world().view().island(named("actor")).unwrap().0;
    let weapon = capture_island(k.snapshot(), actor_island).unwrap();
    assert_eq!(weapon.sweeps().len(), 1);
    assert_eq!(weapon.sweeps()[0].channel, "blade");
    assert_eq!(weapon.sweeps()[0].wait_boundary, 0);
    let break_island = k.world().view().island(named("break_a")).unwrap().0;
    let structure = capture_island(k.snapshot(), break_island).unwrap();
    let joint = structure
        .constraints()
        .into_iter()
        .find(|c| c.threshold == 1)
        .unwrap();
    assert!(joint.break_proposed);
    assert!(!joint.broken);
    assert!(replay_capture(&structure).is_ok());
    assert_eq!(
        k.world()
            .view()
            .qty(named("target"), k.canon().resource_id("health").unwrap()),
        100
    );
}
