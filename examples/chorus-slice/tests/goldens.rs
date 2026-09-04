//! Chorus goldens: 2000 Far + 200 Full, Far Opaque still blocks, lod_period density.

use std::sync::Arc;

use chorus_slice::{
    FAR_COUNT, FULL_COUNT, apply_lod, boot, chorus_doc, far_relic, full_relic, pin,
};
use klotho_canon::cook;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{
    BlobId, Budget, Hash, HullWitness, LOD_PERIOD, Mm, NO_ISLAND, PoseMm, RejectReason, SimLod,
    Tick, Vel3, VelFx, YawMd,
};
use klotho_space::Space;
use klotho_trace::{ISLAND_SNAP_PERIOD_TICKS, TraceBody};
use klotho_world::World;

fn walk_vel() -> Vel3 {
    Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20))
}

#[test]
fn golden_01_two_thousand_far_two_hundred_full() {
    let k = boot();
    let view = k.world().view();
    for i in 0..FULL_COUNT {
        assert_eq!(view.sim_lod(full_relic(i)), SimLod::Full, "full relic {i}");
    }
    for i in 0..FAR_COUNT {
        assert_eq!(view.sim_lod(far_relic(i)), SimLod::Far, "far relic {i}");
    }
    let player = pin(&k, "player");
    let plaza = pin(&k, "plaza");
    let door = pin(&k, "door");
    assert_eq!(view.sim_lod(player), SimLod::Full);
    assert_eq!(view.sim_lod(plaza), SimLod::Full);
    assert_eq!(view.sim_lod(door), SimLod::Far);
    let _ = cook(&chorus_doc()).expect("Chorus cooks");
    let _: fn(World) -> CommitKernel = CommitKernel::new;
    let _same = CommitKernel::new(World::new(
        Arc::new(cook(&chorus_doc()).expect("cook")),
        Hash::ZERO,
    ));
}

#[test]
fn golden_02_far_opaque_still_blocks() {
    let mut k = boot();
    let door = pin(&k, "door");
    let mover = full_relic(0);
    assert_eq!(k.world().view().sim_lod(door), SimLod::Far);
    assert_eq!(k.world().view().sim_lod(mover), SimLod::Full);
    assert!(k.world().view().opaque_closed(door));
    let door_hull = k.world().view().posed_hull(door).expect("door hull");
    assert!(
        k.world()
            .view()
            .space_candidates(door_hull, true)
            .contains(&door),
        "Far OpaqueClosed must remain in space_ix"
    );
    let start = k.world().view().pose(mover).expect("mover pose");
    let blocked_at = PoseMm::new(Mm(40_000), Mm(0), Mm(1_900), YawMd::ZERO);
    let hull = k.world().view().hull_id(mover).unwrap_or(BlobId::ZERO);
    k.ingest(Proposal::SpaceDelta {
        mover,
        pose: blocked_at,
        vel: Vel3::ZERO,
        yaw_rate: 0,
        island: 0,
        sleep_ticks: 0,
        hull,
        witness: HullWitness::new(mover, blocked_at, true),
    });
    let d = k
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [])
        .expect("kernel");
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| matches!(r, RejectReason::Law(_) | RejectReason::WitnessMismatch)),
        "{d:?}"
    );
    assert_eq!(k.world().view().pose(mover).expect("mover pose"), start);
}

#[test]
fn golden_03_lod_period_trace_density() {
    assert_eq!(LOD_PERIOD, 6);
    let mut k = boot();
    let full = full_relic(0);
    let far = far_relic(0);
    k.world_mut()
        .set_vel(full, walk_vel(), 0)
        .expect("full vel");
    k.world_mut().set_vel(far, walk_vel(), 0).expect("far vel");
    apply_lod(&mut k);
    k.partition();

    let full_start = k.world().view().pose(full).expect("full pose").z;
    let far_start = k.world().view().pose(far).expect("far pose").z;
    let mut space = Space;
    let mut far_pose_ticks = 0usize;
    for _ in 0..12 {
        apply_lod(&mut k);
        let full_before = k.world().view().pose(full).expect("full").z;
        let far_before = k.world().view().pose(far).expect("far").z;
        let d = k
            .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut space])
            .expect("kernel");
        assert!(d.rejects.is_empty(), "{d:?}");
        let tick = k.world().tick();
        let full_after = k.world().view().pose(full).expect("full").z;
        let far_after = k.world().view().pose(far).expect("far").z;
        assert_ne!(
            full_after, full_before,
            "Full must step every tick {tick:?}"
        );
        if far_after != far_before {
            far_pose_ticks += 1;
            assert_eq!(
                tick.0 % u64::from(LOD_PERIOD),
                0,
                "Far pose must advance only on lod_period ticks, tick={tick:?}"
            );
        } else {
            assert_ne!(
                tick.0 % u64::from(LOD_PERIOD),
                0,
                "Far skipped a lod_period tick {tick:?}"
            );
        }
        assert_eq!(k.world().view().sim_lod(full), SimLod::Full);
        assert_eq!(k.world().view().sim_lod(far), SimLod::Far);
    }
    assert_eq!(far_pose_ticks, 2);
    assert_eq!(
        k.world().view().pose(full).expect("full").z,
        full_start.wrapping_add(Mm(20 * 12))
    );
    assert_eq!(
        k.world().view().pose(far).expect("far").z,
        far_start.wrapping_add(Mm(20 * 2))
    );

    k.partition();
    for i in 0..FAR_COUNT {
        let s = far_relic(i);
        assert_eq!(
            k.world().view().island(s).map(|(id, _)| id),
            Some(NO_ISLAND),
            "far relic {i}"
        );
    }

    let mut snap_d = None;
    while k.world().tick().0 < ISLAND_SNAP_PERIOD_TICKS {
        apply_lod(&mut k);
        let d = k
            .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut space])
            .expect("kernel");
        if k.world().tick().0 % ISLAND_SNAP_PERIOD_TICKS == 0 {
            snap_d = Some(d);
        }
    }
    let snap_d = snap_d.expect("IslandSnap tick");
    let far_members: Vec<_> = snap_d
        .events
        .iter()
        .filter_map(|e| match &e.body {
            TraceBody::IslandSnap(s) => Some(s),
            _ => None,
        })
        .flat_map(|s| s.members.iter().copied())
        .filter(|&s| (0..FAR_COUNT).any(|i| far_relic(i) == s))
        .collect();
    assert!(
        far_members.is_empty(),
        "Far crowd must not appear as IslandSnap members: {far_members:?}"
    );
}
