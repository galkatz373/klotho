//! PHYS-A12 release behavior on the combined Anvil Canon.
use std::sync::Arc;

use anvil_slice::{boot, named, sword, tick};
use klotho_commit::CommitKernel;
use klotho_core::{
    AabbMm, BlobId, Hash, IVec3, LocusKind, Mm, PlayerId, PoseMm, Sigil, Tick, YawMd,
};
use klotho_phys::{capture_island, replay_capture, summarize_timings};
use klotho_save::{SaveError, SavePortability, check_load, decode, encode, pause_save, restore};
use klotho_trace::{TraceBody, TraceLog};
use klotho_world::{ResumeError, World};

fn run(ticks: usize) -> (Hash, Vec<u8>) {
    let mut k = boot();
    let action = sword(&mut k);
    assert!(action.rejects.is_empty(), "{action:?}");
    for _ in 0..ticks {
        let delta = tick(&mut k);
        assert!(
            delta.rejects.is_empty(),
            "tick {}: {delta:?}",
            k.world().tick().0
        );
    }
    (
        k.world().trace_prefix_hash(),
        k.snapshot().encode().unwrap(),
    )
}

#[test]
fn long_run_is_repeatable_and_remains_inside_behavior_envelopes() {
    let first = run(120);
    if std::env::var_os("KLOTHO_PINNED_PHYS").is_some() {
        assert_eq!(
            first.0.to_string(),
            "9075e1b7db1a8374889ed3965aa4c65ec39d655bf9ffd3d8e77ba192b0763564"
        );
    }
    assert_eq!(first, run(120));

    let mut k = boot();
    sword(&mut k);
    let mut hits = 0;
    let mut breaks = 0;
    for _ in 0..120 {
        let delta = tick(&mut k);
        assert!(delta.rejects.is_empty(), "{delta:?}");
        hits += delta.events.iter().filter(|e| matches!(e.body, TraceBody::Emitted { a, b: Some(b), .. } if a == named("actor") && b == named("target"))).count();
        breaks += delta
            .events
            .iter()
            .filter(|e| matches!(e.body, TraceBody::ConstraintBroken { .. }))
            .count();
    }
    assert_eq!((hits, breaks), (1, 1));
    let view = k.world().view();
    assert_eq!(
        view.qty(named("target"), k.canon().resource_id("health").unwrap()),
        75
    );
    assert!(view.pose(named("push_crate")).unwrap().z.0 > 650);
    assert!(view.pose(named("platform")).unwrap().x.0 > 4_000);
    assert!(view.pose(named("rider")).unwrap().x.0 > 4_500);
    let middle = view.pose(named("stack_middle")).unwrap();
    assert!((-8_000..=-6_000).contains(&middle.x.0), "{middle:?}");
    assert!((-10..=2_500).contains(&middle.y.0), "{middle:?}");
}

#[test]
fn pause_save_resumes_combined_action_structure_and_motion() {
    for checkpoint in [2, 15, 30] {
        let mut original = boot();
        sword(&mut original);
        for _ in 0..checkpoint {
            assert!(tick(&mut original).rejects.is_empty());
        }
        let saved = pause_save(&original.snapshot()).unwrap();
        assert!(saved.suffix.is_empty());
        let loaded = decode(&encode(&saved).unwrap()).unwrap();
        assert_eq!(loaded.portability, SavePortability::current());
        let restored = restore(&loaded, saved.prefix, saved.canon_hash).unwrap();
        assert_eq!(saved.snap.encode().unwrap(), restored.encode().unwrap());
        assert_eq!(
            check_load(&loaded, saved.prefix, Hash::from_bytes([7; 32])),
            Err(SaveError::CanonMismatch)
        );
        let mut foreign = loaded.clone();
        foreign.portability = SavePortability::SamePlatform { os: 99, arch: 99 };
        assert_eq!(
            check_load(&foreign, saved.prefix, saved.canon_hash),
            Err(SaveError::PlatformMismatch)
        );

        let canon = Arc::new(original.canon().clone());
        let trace = TraceLog::from_events(original.world().trace().events().to_vec());
        assert!(matches!(
            World::resume(
                Arc::clone(&canon),
                Hash::from_bytes([8; 32]),
                &restored,
                trace.clone()
            ),
            Err(ResumeError::CanonMismatch)
        ));
        let mut wrong = trace.clone();
        wrong.append(klotho_trace::TraceEvent::new(
            Tick(restored.tick.0 + 1),
            TraceBody::SaveRequested,
        ));
        assert!(matches!(
            World::resume(Arc::clone(&canon), saved.canon_hash, &restored, wrong),
            Err(ResumeError::TraceMismatch)
        ));
        let world = World::resume(canon, saved.canon_hash, &restored, trace).unwrap();
        let mut resumed = CommitKernel::new(world);
        resumed.bind_player(PlayerId(0), named("actor"));

        for _ in checkpoint..60 {
            let a = tick(&mut original);
            let b = tick(&mut resumed);
            assert_eq!(a.rejects, b.rejects);
            assert_eq!(a.events, b.events);
        }
        assert_eq!(
            original.world().trace_prefix_hash(),
            resumed.world().trace_prefix_hash()
        );
        assert_eq!(
            original.snapshot().encode().unwrap(),
            resumed.snapshot().encode().unwrap()
        );
    }
}

#[test]
fn repeated_captures_replay_and_report_bounded_workload() {
    let mut k = boot();
    sword(&mut k);
    let mut samples = Vec::new();
    for _ in 0..30 {
        k.partition();
        let island = k.world().view().island(named("push_actor")).unwrap().0;
        let capture = capture_island(k.snapshot(), island).expect("capture");
        replay_capture(&capture).expect("same proposal from published snapshot");
        samples.push(capture.timings());
        assert!(tick(&mut k).rejects.is_empty());
    }
    let report = summarize_timings(&samples).unwrap();
    assert_eq!(report.samples, 30);
    assert!(report.max_bodies <= 256);
    assert!(report.max_contacts <= 1_024);
    assert!(report.max_constraints <= 512);
    for stage in [
        report.broad,
        report.character,
        report.vehicle,
        report.narrow,
        report.constraint,
        report.encode,
    ] {
        assert!(stage.p50_us <= stage.p95_us);
        assert!(stage.p95_us <= stage.p99_us);
        assert!(stage.p99_us <= stage.max_us);
    }
}

#[test]
fn adventure_awake_body_capacity_admits_independent_islands() {
    let mut k = boot();
    let hull = AabbMm::new(
        IVec3 {
            x: -100,
            y: 0,
            z: -100,
        },
        IVec3 {
            x: 100,
            y: 200,
            z: 100,
        },
    );
    for i in 0..2_000u128 {
        let s = Sigil::pack(LocusKind::Relic, 0, 1_000 + i).unwrap();
        let mut w = k.world_mut();
        w.insert_locus(s, LocusKind::Relic).unwrap();
        w.set_hull(s, hull, BlobId::from_bytes([42; 32])).unwrap();
        w.set_pose(
            s,
            PoseMm::new(Mm(100_000 + i as i32 * 1_000), Mm(500), Mm(0), YawMd::ZERO),
        )
        .unwrap();
    }
    let islands = k.partition();
    assert!(islands.len() >= 2_000, "{} islands", islands.len());
    let delta = tick(&mut k);
    assert!(delta.rejects.is_empty(), "{} rejects", delta.rejects.len());
    assert!(
        k.world()
            .view()
            .pose(Sigil::pack(LocusKind::Relic, 0, 1_000).unwrap())
            .unwrap()
            .y
            .0
            < 500
    );
}
