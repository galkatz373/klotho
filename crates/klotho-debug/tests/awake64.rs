//! 64-awake microbench: the 4 ms kernel budget gate.

use std::sync::Arc;
use std::time::Instant;

use klotho_canon::cook_diffs;
use klotho_commit::CommitKernel;
use klotho_core::{
    AabbMm, BlobId, Budget, Hash, IVec3, LocusKind, Mm, PoseMm, Sigil, Tick, Vel3, VelFx, YawMd,
};
use klotho_debug::BudgetMode;
use klotho_ir::{CanonDiff, from_ron};
use klotho_space::{Space, WALK_MM_PER_TICK};
use klotho_trace::{ISLAND_SNAP_PERIOD_TICKS, PoseReason, TraceBody, TraceEvent, encode_event};
use klotho_world::World;

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

#[test]
fn awake64_is_under_four_ms() {
    let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
    let canon = cook_diffs(&diffs).unwrap();
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    for i in 0..64u128 {
        let s = relic(i + 10);
        let mut w = k.world_mut();
        w.insert_locus(s, LocusKind::Relic).unwrap();
        w.set_hull(s, box_xz(100, 1800, 100), hull_id((i as u8) | 1))
            .unwrap();
        w.set_pose(s, PoseMm::new(Mm(i as i32 * 400), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        w.set_vel(
            s,
            Vel3::new(
                VelFx::from_mm_per_tick(WALK_MM_PER_TICK),
                VelFx::ZERO,
                VelFx::ZERO,
            ),
            0,
        )
        .unwrap();
    }
    let mut space = Space;
    let t0 = Instant::now();
    let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut space]).unwrap();
    let us = t0.elapsed().as_micros();
    assert!(d.rejects.is_empty(), "{d:?}");
    BudgetMode::from_env().enforce(us);
}

#[test]
fn awake64_120_tick_trace_bytes_drop_by_orders_of_magnitude() {
    let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
    let canon = cook_diffs(&diffs).unwrap();
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    for i in 0..64u128 {
        let s = relic(i + 10);
        let mut w = k.world_mut();
        w.insert_locus(s, LocusKind::Relic).unwrap();
        w.set_hull(s, box_xz(100, 1800, 100), hull_id((i as u8) | 1))
            .unwrap();
        w.set_pose(s, PoseMm::new(Mm(i as i32 * 400), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        w.set_vel(s, VelFx::from_mm_per_tick(WALK_MM_PER_TICK), VelFx::ZERO, 0)
            .unwrap();
    }
    let mut space = Space;
    let mut new_bytes = 0usize;
    let mut snap_ticks = 0u32;
    for _ in 0..120 {
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut space]).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        for e in &d.events {
            assert!(
                !matches!(e.body, TraceBody::PoseCommitted { .. }),
                "per-admit PoseCommitted must not land on the tape"
            );
            if matches!(e.body, TraceBody::IslandSnap(_)) {
                snap_ticks += 1;
            }
            new_bytes += encode_event(e).len();
        }
    }
    let old_event = TraceEvent::new(
        Tick(1),
        TraceBody::PoseCommitted {
            s: relic(10),
            pose: PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)),
            reason: PoseReason::Land,
        },
    );
    let old_bytes = 64usize * 120 * encode_event(&old_event).len();
    assert_eq!(snap_ticks, (120 / ISLAND_SNAP_PERIOD_TICKS) as u32);
    assert!(new_bytes > 0, "2 Hz IslandSnap must still land");
    assert!(
        new_bytes * 10 < old_bytes,
        "64-awake 120-tick Trace bytes {new_bytes} not 10× under old tape {old_bytes}"
    );
}
