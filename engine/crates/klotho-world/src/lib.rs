//! World is a view of `(canon_hash, trace_prefix_hash)` plus the live Intent
//! heap. Projection columns (vel / island columns and `space_ix`) are derived.
//! Snapshots are checkpoints of Trace, not a second world.
//!
//! Write path is `WorldMut` behind feature `mutate` (crate unit tests also see
//! it). `klotho-commit` is the only runtime crate that enables the feature.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cow;
mod error;
mod grid;
mod heap;
mod mutate;
mod proj;
mod rewind;
mod snap;
mod snap_blob;
mod spec;
mod view;
mod world;

pub use error::{SnapError, WorldError};
pub use grid::{CELL_MM, GridIndex, PlaceIndex, world_aabb};
pub use heap::IntentHeap;
pub use klotho_core::{MAX_LOCI_HEARTH, MAX_LOCI_PROCESS, PackedIx};
#[cfg(any(test, feature = "mutate"))]
pub use mutate::WorldMut;
pub use proj::{Projection, RiteMachine};
pub use rewind::RewindRing;
pub use snap::{MAX_PLACE_ROWS, PlaceRow, PlaceSnap};
pub use snap_blob::{
    MAX_ROW_KNOWS, MAX_ROW_QTY, MAX_ROW_RELS, MAX_ROW_RITES, MAX_SNAP_ROWS, SNAP_BLOB_CAP,
    SNAP_MAGIC, SNAP_VERSION, SnapRow, check_snap_size,
};
#[cfg(any(test, feature = "mutate"))]
pub use spec::SpecDelta;
pub use view::{HITSCAN_RANGE_MM, WorldView};
pub use world::{World, WorldSnapshot};

/// Hearth packed-row cap. Process cap is [`MAX_LOCI_PROCESS`].
pub const MAX_LOCI: usize = MAX_LOCI_HEARTH;
/// Snapshot blob cap. Hearth is ~1–2 MB.
pub const SNAPSHOT_CAP: usize = 16 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{
        AabbMm, AffordanceId, BlobId, ConstraintState, Hash, IVec3, LocusKind, Mm, PackedIx,
        PhysRequest, PoseMm, ResourceId, Sigil, SimLod, Tick, Vel3, VelFx, YawMd,
    };
    use klotho_ir::{CanonDiff, Rel, from_ron};
    use klotho_trace::{PoseReason, TraceBody, TraceEvent};

    use super::*;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn box_mm(half: i32) -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -half,
                y: 0,
                z: -half,
            },
            IVec3 {
                x: half,
                y: 500,
                z: half,
            },
        )
    }

    fn opaque_canon() -> klotho_canon::Canon {
        let diffs: Vec<CanonDiff> = from_ron(
            r#"[AddAffordance(Affordance(id: "Opaque", requires: [], grants: [], conflicts: []))]"#,
        )
        .unwrap();
        cook_diffs(&diffs).unwrap()
    }

    fn opaque_world() -> World {
        World::new(Arc::new(opaque_canon()), Hash::ZERO)
    }

    fn opaque_world_cap(cap: usize) -> World {
        World::with_locus_cap(Arc::new(opaque_canon()), Hash::ZERO, cap)
    }

    #[test]
    fn prefix_hash_changes_when_trace_appends() {
        let mut w = opaque_world();
        let before = w.trace_prefix_hash();
        assert_eq!(before, klotho_trace::genesis_hash());
        w.mutate().append(TraceEvent::new(
            Tick(1),
            TraceBody::QtyChanged {
                id: actor(1),
                res: ResourceId(0),
                to: 10,
                quantum: 10,
            },
        ));
        assert_ne!(w.trace_prefix_hash(), before);
        let mid = w.trace_prefix_hash();
        w.mutate()
            .append(TraceEvent::new(Tick(2), TraceBody::SaveRequested));
        assert_ne!(w.trace_prefix_hash(), mid);
    }

    #[test]
    fn space_ix_rebuild_equals_incremental() {
        let mut w = opaque_world();
        let a = relic(1);
        let b = relic(2);
        {
            let mut m = w.mutate();
            m.insert_locus(a, LocusKind::Relic).unwrap();
            m.insert_locus(b, LocusKind::Relic).unwrap();
            m.set_hull(a, box_mm(200), BlobId::ZERO).unwrap();
            m.set_hull(b, box_mm(200), BlobId::ZERO).unwrap();
            m.set_pose(a, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            m.set_pose(b, PoseMm::new(Mm(3000), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
        }
        let inc = w.view().space_candidates(
            AabbMm::new(
                IVec3 {
                    x: -100,
                    y: 0,
                    z: -100,
                },
                IVec3 {
                    x: 100,
                    y: 500,
                    z: 100,
                },
            ),
            false,
        );
        assert!(inc.contains(&a));
        assert!(!inc.contains(&b));
        let before = w.view().space_candidates(
            AabbMm::new(
                IVec3 {
                    x: -10_000,
                    y: 0,
                    z: -10_000,
                },
                IVec3 {
                    x: 10_000,
                    y: 500,
                    z: 10_000,
                },
            ),
            false,
        );
        w.mutate().rebuild_space_ix();
        let after = w.view().space_candidates(
            AabbMm::new(
                IVec3 {
                    x: -10_000,
                    y: 0,
                    z: -10_000,
                },
                IVec3 {
                    x: 10_000,
                    y: 500,
                    z: 10_000,
                },
            ),
            false,
        );
        assert_eq!(before, after);
        w.mutate()
            .set_pose(b, PoseMm::new(Mm(100), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        let inc2 = w.view().space_candidates(
            AabbMm::new(
                IVec3 {
                    x: -100,
                    y: 0,
                    z: -100,
                },
                IVec3 {
                    x: 400,
                    y: 500,
                    z: 100,
                },
            ),
            false,
        );
        w.mutate().rebuild_space_ix();
        let reb2 = w.view().space_candidates(
            AabbMm::new(
                IVec3 {
                    x: -100,
                    y: 0,
                    z: -100,
                },
                IVec3 {
                    x: 400,
                    y: 500,
                    z: 100,
                },
            ),
            false,
        );
        assert_eq!(inc2, reb2);
        assert!(reb2.contains(&a) && reb2.contains(&b));
    }

    #[test]
    fn snapshot_under_16mb() {
        let mut w = opaque_world();
        {
            let mut m = w.mutate();
            for i in 0..MAX_LOCI as u128 {
                let s = relic(i + 1);
                m.insert_locus(s, LocusKind::Relic).unwrap();
                m.set_hull(s, box_mm(100), BlobId::ZERO).unwrap();
                m.set_pose(s, PoseMm::new(Mm((i as i32) * 200), Mm(0), Mm(0), YawMd(0)))
                    .unwrap();
            }
        }
        let snap = w.snapshot();
        assert!(
            snap.under_cap(),
            "approx_bytes={} cap={}",
            snap.approx_bytes(),
            SNAPSHOT_CAP
        );
        assert!(snap.approx_bytes() < SNAPSHOT_CAP);
    }

    #[test]
    fn snapshot_is_not_live_world() {
        let mut w = opaque_world();
        let s = relic(1);
        w.mutate().insert_locus(s, LocusKind::Relic).unwrap();
        w.mutate()
            .set_pose(s, PoseMm::new(Mm(1), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        let snap = w.snapshot();
        w.mutate()
            .set_pose(s, PoseMm::new(Mm(99), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        assert_eq!(snap.view().pose(s).unwrap().x, Mm(1));
        assert_eq!(w.view().pose(s).unwrap().x, Mm(99));
        assert_eq!(snap.trace_prefix_hash, w.trace_prefix_hash());
    }

    #[test]
    fn pose_committed_copies_six_dof() {
        let mut w = opaque_world();
        let s = relic(1);
        {
            let mut m = w.mutate();
            m.insert_locus(s, LocusKind::Relic).unwrap();
            let mut p = PoseMm::new(Mm(10), Mm(50), Mm(20), YawMd(30));
            p.pitch = YawMd(1_000);
            p.roll = YawMd(2_000);
            m.set_pose(s, p).unwrap();
            m.append(TraceEvent::new(
                Tick(1),
                TraceBody::PoseCommitted {
                    s,
                    pose: PoseMm::new(Mm(11), Mm(0), Mm(21), YawMd(31)),
                    reason: PoseReason::Land,
                },
            ));
        }
        let got = w.view().pose(s).unwrap();
        assert_eq!(got.x, Mm(11));
        assert_eq!(got.y, Mm(0));
        assert_eq!(got.z, Mm(21));
        assert_eq!(got.yaw, YawMd(31));
        assert_eq!(got.pitch, YawMd::ZERO);
        assert_eq!(got.roll, YawMd::ZERO);
    }

    #[test]
    fn spawn_apply_inserts_place_and_despawn_remain_noop() {
        let mut w = opaque_world();
        let s = relic(1);
        let place = Sigil::pack(LocusKind::Place, 0, 2).unwrap();
        let pose = PoseMm::new(Mm(10), Mm(50), Mm(20), YawMd(30));
        let spawned_at = PoseMm::new(Mm(3), Mm(4), Mm(5), YawMd(6));
        {
            let mut m = w.mutate();
            m.insert_locus(s, LocusKind::Relic).unwrap();
            m.insert_locus(place, LocusKind::Place).unwrap();
            m.set_pose(s, pose).unwrap();
            m.add_rel(s, Rel::In, place).unwrap();
            m.set_qty(s, ResourceId(0), 7).unwrap();
            for body in [
                TraceBody::PlaceLoaded { place, n: 99 },
                TraceBody::PlaceEvicted { place },
                TraceBody::Spawned {
                    template: 1,
                    sigil: relic(9),
                    at: spawned_at,
                },
                TraceBody::Despawned {
                    sigil: s,
                    generation: 3,
                },
            ] {
                m.append(TraceEvent::new(Tick(1), body));
            }
        }
        let view = w.view();
        assert_eq!(view.loci().count(), 3);
        assert_eq!(view.pose(s), Some(pose));
        assert!(view.has_rel(s, Rel::In, place));
        assert_eq!(view.qty(s, ResourceId(0)), 7);
        assert_eq!(view.kind(relic(9)), Some(LocusKind::Relic));
        assert_eq!(view.pose(relic(9)), Some(spawned_at));
    }

    #[test]
    fn locus_cap() {
        let mut w = opaque_world();
        let mut m = w.mutate();
        for i in 0..MAX_LOCI as u128 {
            m.insert_locus(relic(i + 1), LocusKind::Relic).unwrap();
        }
        assert_eq!(
            m.insert_locus(relic(9_999), LocusKind::Relic),
            Err(WorldError::LocusCap)
        );
    }

    #[test]
    fn opaque_closed_in_space_ix() {
        let mut w = opaque_world();
        let door = relic(1);
        let opaque = w.canon().affordance_id("Opaque").unwrap();
        {
            let mut m = w.mutate();
            m.insert_locus(door, LocusKind::Relic).unwrap();
            m.set_hull(door, box_mm(400), BlobId::ZERO).unwrap();
            m.set_pose(door, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            m.set_affordance(door, opaque, true).unwrap();
            m.add_rel(door, Rel::LockedBy, door).unwrap();
        }
        assert!(w.view().opaque_closed(door));
        let hits = w.view().space_candidates(
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
        w.mutate().append(TraceEvent::new(
            Tick(1),
            TraceBody::RelDel {
                a: door,
                rel: klotho_trace::RelTag::LOCKED_BY,
                b: door,
            },
        ));
        assert!(!w.view().opaque_closed(door));
        let hits = w.view().space_candidates(
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
        assert!(hits.is_empty());
    }

    #[test]
    fn with_affordance_and_qty() {
        let mut w = opaque_world();
        let s = relic(1);
        let opaque = AffordanceId(0);
        w.mutate().insert_locus(s, LocusKind::Relic).unwrap();
        w.mutate().set_affordance(s, opaque, true).unwrap();
        w.mutate().set_qty(s, ResourceId(1), 7).unwrap();
        assert_eq!(
            w.view().with_affordance(opaque).collect::<Vec<_>>(),
            vec![s]
        );
        assert_eq!(w.view().qty(s, ResourceId(1)), 7);
        assert_eq!(w.view().qty(s, ResourceId(2)), 0);
    }

    #[test]
    fn clear_phys_req_drops_the_row() {
        let mut w = opaque_world();
        let s = relic(1);
        {
            let mut m = w.mutate();
            m.insert_locus(s, LocusKind::Relic).unwrap();
            m.set_phys_req(
                s,
                klotho_core::PhysRequest {
                    lin: IVec3 { x: 3, y: 0, z: 0 },
                    ang: IVec3::ZERO,
                },
            )
            .unwrap();
        }
        assert!(w.view().phys_req(s).is_some());
        w.mutate().clear_phys_req(s).unwrap();
        assert!(w.view().phys_req(s).is_none());
    }

    #[test]
    fn attach_local_defaults_on_rel_add() {
        let mut w = opaque_world();
        let parent = relic(1);
        let child = relic(2);
        {
            let mut m = w.mutate();
            m.insert_locus(parent, LocusKind::Relic).unwrap();
            m.insert_locus(child, LocusKind::Relic).unwrap();
            m.set_pose(parent, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            m.set_pose(child, PoseMm::new(Mm(1000), Mm(200), Mm(0), YawMd(0)))
                .unwrap();
            m.add_rel(child, Rel::AttachedTo, parent).unwrap();
        }
        assert_eq!(
            w.view().attach_local(child),
            Some(IVec3 {
                x: 1000,
                y: 200,
                z: 0
            })
        );
        assert_eq!(w.view().attach_parent(child), Some(parent));
        w.mutate().del_rel(child, Rel::AttachedTo, parent).unwrap();
        assert!(w.view().attach_local(child).is_none());
    }

    #[test]
    fn packed_ix_is_u32() {
        assert_eq!(core::mem::size_of::<PackedIx>(), 4);
        assert_eq!(MAX_LOCI, 4_096);
        assert_eq!(MAX_LOCI_HEARTH, 4_096);
        assert_eq!(MAX_LOCI_PROCESS, 200_000);
        assert!(MAX_LOCI_PROCESS > usize::from(u16::MAX));
    }

    #[test]
    fn process_cap_beyond_hearth() {
        let extra = 8;
        let mut w = opaque_world_cap(MAX_LOCI + extra);
        assert_eq!(w.locus_cap(), MAX_LOCI + extra);
        let mut m = w.mutate();
        for i in 0..(MAX_LOCI + extra) as u128 {
            m.insert_locus(relic(i + 1), LocusKind::Relic).unwrap();
        }
        assert_eq!(
            m.insert_locus(relic(9_999), LocusKind::Relic),
            Err(WorldError::LocusCap)
        );
    }

    #[test]
    fn locus_cap_clamps_to_process() {
        let w = opaque_world_cap(MAX_LOCI_PROCESS + 1);
        assert_eq!(w.locus_cap(), MAX_LOCI_PROCESS);
    }

    #[test]
    fn snapshot_cow_shares_clean_chunks() {
        let mut w = opaque_world();
        let first = relic(1);
        let later = relic((crate::cow::COW_CHUNK + 1) as u128);
        {
            let mut m = w.mutate();
            for i in 0..(crate::cow::COW_CHUNK + 2) as u128 {
                let s = relic(i + 1);
                m.insert_locus(s, LocusKind::Relic).unwrap();
                m.set_pose(s, PoseMm::new(Mm(i as i32), Mm(0), Mm(0), YawMd(0)))
                    .unwrap();
            }
        }
        let snap1 = w.snapshot();
        w.mutate()
            .set_pose(first, PoseMm::new(Mm(99), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        let snap2 = w.snapshot();
        assert_eq!(snap1.view().pose(first).unwrap().x, Mm(0));
        assert_eq!(snap2.view().pose(first).unwrap().x, Mm(99));
        assert_eq!(
            snap1.view().pose(later).unwrap().x,
            snap2.view().pose(later).unwrap().x
        );
        assert!(!snap1.shares_pose_chunk(&snap2, 0));
        assert!(snap1.shares_pose_chunk(&snap2, crate::cow::COW_CHUNK));
    }

    fn origin_swept() -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -200,
                y: 0,
                z: -200,
            },
            IVec3 {
                x: 200,
                y: 500,
                z: 200,
            },
        )
    }

    #[test]
    fn per_place_grid_ready() {
        let mut w = opaque_world();
        let place_a = Sigil::pack(LocusKind::Place, 0, 10).unwrap();
        let place_b = Sigil::pack(LocusKind::Place, 0, 11).unwrap();
        let in_a = relic(1);
        let in_b = relic(2);
        let free = relic(3);
        {
            let mut m = w.mutate();
            m.insert_locus(place_a, LocusKind::Place).unwrap();
            m.insert_locus(place_b, LocusKind::Place).unwrap();
            m.insert_locus(in_a, LocusKind::Relic).unwrap();
            m.insert_locus(in_b, LocusKind::Relic).unwrap();
            m.insert_locus(free, LocusKind::Relic).unwrap();
            for s in [in_a, in_b, free] {
                m.set_hull(s, box_mm(100), BlobId::ZERO).unwrap();
                m.set_pose(s, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
                    .unwrap();
            }
            m.add_rel(in_a, Rel::In, place_a).unwrap();
            m.add_rel(in_b, Rel::In, place_b).unwrap();
        }
        let swept = origin_swept();
        let ix_a = w.projection().packed(in_a).unwrap();
        let ix_b = w.projection().packed(in_b).unwrap();
        let ix_free = w.projection().packed(free).unwrap();
        let space = w.projection().space_ix();
        let ga = space.grid(place_a).unwrap().candidates(swept, false);
        assert!(ga.contains(&ix_a));
        assert!(!ga.contains(&ix_b) && !ga.contains(&ix_free));
        let gb = space.grid(place_b).unwrap().candidates(swept, false);
        assert!(gb.contains(&ix_b));
        assert!(!gb.contains(&ix_a) && !gb.contains(&ix_free));
        let unplaced = space.unplaced().candidates(swept, false);
        assert!(unplaced.contains(&ix_free));
        assert!(!unplaced.contains(&ix_a) && !unplaced.contains(&ix_b));
        let hits = w.view().space_candidates(swept, false);
        assert!(hits.contains(&in_a) && hits.contains(&in_b) && hits.contains(&free));
        assert_eq!(w.projection().space_ix().place_count(), 2);

        w.mutate().del_rel(in_a, Rel::In, place_a).unwrap();
        let space = w.projection().space_ix();
        assert!(space.grid(place_a).is_none());
        assert_eq!(space.place_count(), 1);
        assert!(space.unplaced().candidates(swept, false).contains(&ix_a));
        w.mutate().add_rel(in_a, Rel::In, place_a).unwrap();
        let space = w.projection().space_ix();
        assert!(
            space
                .grid(place_a)
                .unwrap()
                .candidates(swept, false)
                .contains(&ix_a)
        );
        assert!(!space.unplaced().candidates(swept, false).contains(&ix_a));
    }

    #[test]
    fn packed_ix_past_u16_max() {
        let n = usize::from(u16::MAX) + 2;
        let mut w = opaque_world_cap(n);
        {
            let mut m = w.mutate();
            for i in 0..n as u128 {
                m.insert_locus(relic(i + 1), LocusKind::Relic).unwrap();
            }
        }
        let last = relic(n as u128);
        let ix = w.projection().packed(last).unwrap();
        assert_eq!(ix, (n - 1) as PackedIx);
        assert!(ix > PackedIx::from(u16::MAX));
        assert_eq!(w.view().loci().count(), n);
    }

    #[test]
    fn publish_50k_rows() {
        const N: usize = 50_000;
        const HULLS: u128 = 64;
        let mut w = opaque_world_cap(N);
        {
            let mut m = w.mutate();
            for i in 0..N as u128 {
                let s = relic(i + 1);
                m.insert_locus(s, LocusKind::Relic).unwrap();
                m.set_pose(s, PoseMm::new(Mm(i as i32), Mm(0), Mm(0), YawMd(0)))
                    .unwrap();
                if i < HULLS {
                    m.set_hull(s, box_mm(100), BlobId::ZERO).unwrap();
                }
            }
        }
        let t0 = std::time::Instant::now();
        let snap = w.snapshot();
        let first_publish = t0.elapsed();
        assert_eq!(snap.view().loci().count(), N);
        let first = relic(1);
        w.mutate()
            .set_pose(first, PoseMm::new(Mm(7), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        let t1 = std::time::Instant::now();
        let snap2 = w.snapshot();
        let second_publish = t1.elapsed();
        assert_eq!(snap.view().pose(first).unwrap().x, Mm(0));
        assert_eq!(snap2.view().pose(first).unwrap().x, Mm(7));
        assert!(snap1_shares_later(&snap, &snap2));
        let _ = (first_publish, second_publish);
    }

    fn snap1_shares_later(a: &WorldSnapshot, b: &WorldSnapshot) -> bool {
        a.shares_pose_chunk(b, crate::cow::COW_CHUNK)
    }

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    #[test]
    fn apply_place_snap_inserts_and_indexes_opaque_closed() {
        let mut w = opaque_world();
        let p = place(1);
        let door = relic(2);
        let opaque = w.canon().affordance_id("Opaque").unwrap();
        let mut door_row = PlaceRow::new(door, LocusKind::Relic);
        door_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        door_row.hull = Some(box_mm(400));
        door_row.afford = 1u64 << opaque.0;
        door_row.rels = vec![(Rel::In, p), (Rel::LockedBy, door)];
        let mut place_row = PlaceRow::new(p, LocusKind::Place);
        place_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        let snap = PlaceSnap::new(
            p,
            w.canon_hash(),
            w.trace_prefix_hash(),
            vec![place_row, door_row],
        );
        let n = w.mutate().apply_place_snap(&snap).unwrap();
        assert_eq!(n, 2);
        assert!(w.view().contains(p));
        assert!(w.view().contains(door));
        assert!(w.view().has_rel(door, Rel::In, p));
        assert!(w.view().opaque_closed(door));
        let hits = w.view().space_candidates(
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
    fn apply_place_snap_rejects_oversize_and_duplicates_with_zero_rows() {
        let mut w = opaque_world();
        let p = place(1);
        let too_big = vec![PlaceRow::new(relic(1), LocusKind::Relic); MAX_PLACE_ROWS + 1];
        let snap = PlaceSnap::new(p, Hash::ZERO, Hash::ZERO, too_big);
        assert_eq!(
            w.mutate().apply_place_snap(&snap),
            Err(WorldError::PlaceSnap)
        );
        assert_eq!(w.view().loci().count(), 0);

        let row = PlaceRow::new(relic(1), LocusKind::Relic);
        let dup = PlaceSnap::new(p, Hash::ZERO, Hash::ZERO, vec![row.clone(), row]);
        assert_eq!(
            w.mutate().apply_place_snap(&dup),
            Err(WorldError::PlaceSnap)
        );
        assert_eq!(w.view().loci().count(), 0);
    }

    #[test]
    fn apply_place_snap_rejects_two_in_edges_with_zero_rows() {
        let mut w = opaque_world();
        let p = place(1);
        let q = place(2);
        let r = relic(1);
        let mut row = PlaceRow::new(r, LocusKind::Relic);
        row.rels = vec![(Rel::In, p), (Rel::In, q)];
        let snap = PlaceSnap::new(p, Hash::ZERO, Hash::ZERO, vec![row]);
        assert_eq!(
            w.mutate().apply_place_snap(&snap),
            Err(WorldError::PlaceSnap)
        );
        assert_eq!(w.view().loci().count(), 0);
    }

    #[test]
    fn apply_place_snap_replaces_existing_row_maps() {
        let mut w = opaque_world();
        let p = place(1);
        let r = relic(1);
        {
            let mut m = w.mutate();
            m.insert_locus(p, LocusKind::Place).unwrap();
            m.insert_locus(r, LocusKind::Relic).unwrap();
            m.set_qty(r, ResourceId(1), 9).unwrap();
            m.add_rel(r, Rel::LockedBy, r).unwrap();
            m.add_rel(r, Rel::In, p).unwrap();
        }
        let mut row = PlaceRow::new(r, LocusKind::Relic);
        row.pose = Some(PoseMm::new(Mm(4), Mm(0), Mm(0), YawMd(0)));
        row.rels = vec![(Rel::In, p)];
        row.qty = vec![(ResourceId(2), 3)];
        let mut place_row = PlaceRow::new(p, LocusKind::Place);
        place_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        let snap = PlaceSnap::new(
            p,
            w.canon_hash(),
            w.trace_prefix_hash(),
            vec![place_row, row],
        );
        w.mutate().apply_place_snap(&snap).unwrap();
        assert_eq!(w.view().qty(r, ResourceId(1)), 0);
        assert_eq!(w.view().qty(r, ResourceId(2)), 3);
        assert!(!w.view().has_rel(r, Rel::LockedBy, r));
        assert!(w.view().has_rel(r, Rel::In, p));
        assert_eq!(w.view().pose(r).unwrap().x, Mm(4));
    }

    #[test]
    fn apply_place_snap_cap_is_fail_closed() {
        let mut w = opaque_world_cap(4);
        let p = place(1);
        let rows: Vec<_> = (0..5u128)
            .map(|i| PlaceRow::new(relic(i + 1), LocusKind::Relic))
            .collect();
        let snap = PlaceSnap::new(p, Hash::ZERO, Hash::ZERO, rows);
        assert_eq!(
            w.mutate().apply_place_snap(&snap),
            Err(WorldError::LocusCap)
        );
        assert_eq!(w.view().loci().count(), 0);
    }

    #[test]
    fn evict_place_keeps_migrating_and_remaps_packed() {
        let mut w = opaque_world();
        let p = place(1);
        let a = relic(1);
        let b = relic(2);
        let c = relic(3);
        let host = relic(9);
        {
            let mut m = w.mutate();
            m.insert_locus(p, LocusKind::Place).unwrap();
            m.insert_locus(a, LocusKind::Relic).unwrap();
            m.insert_locus(b, LocusKind::Relic).unwrap();
            m.insert_locus(c, LocusKind::Relic).unwrap();
            m.insert_locus(host, LocusKind::Relic).unwrap();
            m.set_pose(a, PoseMm::new(Mm(1), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            m.set_pose(b, PoseMm::new(Mm(2), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            m.set_pose(c, PoseMm::new(Mm(3), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            m.add_rel(a, Rel::OwnedBy, c).unwrap();
            m.add_rel(b, Rel::In, p).unwrap();
            m.add_rel(c, Rel::In, p).unwrap();
            m.add_rel(c, Rel::AttachedTo, host).unwrap();
        }
        w.mutate().evict_place(p).unwrap();
        assert!(w.view().contains(p));
        assert!(w.view().contains(a));
        assert!(!w.view().contains(b));
        assert!(w.view().contains(c));
        assert_eq!(w.view().pose(a).unwrap().x, Mm(1));
        assert_eq!(w.view().pose(c).unwrap().x, Mm(3));
        assert!(w.view().has_rel(a, Rel::OwnedBy, c));
        assert!(w.view().has_rel(c, Rel::AttachedTo, host));
    }

    #[test]
    fn capture_place_round_trip() {
        let mut w = opaque_world();
        let p = place(1);
        let r = relic(2);
        {
            let mut m = w.mutate();
            m.insert_locus(p, LocusKind::Place).unwrap();
            m.insert_locus(r, LocusKind::Relic).unwrap();
            m.set_pose(r, PoseMm::new(Mm(8), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            m.set_qty(r, ResourceId(1), 4).unwrap();
            m.add_rel(r, Rel::In, p).unwrap();
        }
        let snap = w
            .view()
            .capture_place(p, w.canon_hash(), w.trace_prefix_hash())
            .unwrap();
        assert_eq!(snap.place, p);
        assert_eq!(snap.len(), 2);
        let mut w2 = opaque_world();
        w2.mutate().apply_place_snap(&snap).unwrap();
        assert_eq!(w2.view().pose(r).unwrap().x, Mm(8));
        assert_eq!(w2.view().qty(r, ResourceId(1)), 4);
        assert!(w2.view().has_rel(r, Rel::In, p));
    }

    #[test]
    fn apply_place_snap_10k_rows() {
        const N: usize = 10_000;
        let mut w = opaque_world_cap(N);
        let p = place(1);
        let mut rows = Vec::with_capacity(N);
        let mut place_row = PlaceRow::new(p, LocusKind::Place);
        place_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
        rows.push(place_row);
        for i in 1..N as u128 {
            let s = relic(i);
            let mut row = PlaceRow::new(s, LocusKind::Relic);
            row.pose = Some(PoseMm::new(Mm(i as i32), Mm(0), Mm(0), YawMd(0)));
            if i < 64 {
                row.hull = Some(box_mm(100));
            }
            row.rels = vec![(Rel::In, p)];
            rows.push(row);
        }
        let snap = PlaceSnap::new(p, w.canon_hash(), w.trace_prefix_hash(), rows);
        let t = std::time::Instant::now();
        let n = w.mutate().apply_place_snap(&snap).unwrap();
        let dt = t.elapsed();
        assert_eq!(n, N as u32);
        assert_eq!(w.view().loci().count(), N);
        assert!(w.view().contains(relic(1)));
        assert!(w.view().has_rel(relic(N as u128 - 1), Rel::In, p));
        if !cfg!(debug_assertions) {
            assert!(
                dt.as_secs_f64() * 1_000.0 <= 2.0,
                "10k-row apply took {dt:?}, gate is 2 ms"
            );
        }
    }

    #[test]
    fn snapshot_blob_round_trips_columns_via_world_view() {
        let mut w = opaque_world();
        let door = relic(1);
        let mind = actor(2);
        let opaque = w.canon().affordance_id("Opaque").unwrap();
        let mut pose = PoseMm::new(Mm(10), Mm(50), Mm(20), YawMd(30));
        pose.pitch = YawMd(1_000);
        pose.roll = YawMd(2_000);
        let vel = Vel3::new(VelFx(100), VelFx(200), VelFx(300));
        {
            let mut m = w.mutate();
            m.insert_locus(door, LocusKind::Relic).unwrap();
            m.insert_locus(mind, LocusKind::Actor).unwrap();
            m.set_hull(door, box_mm(400), BlobId::ZERO).unwrap();
            m.set_pose(door, pose).unwrap();
            m.set_vel(door, vel, 11).unwrap();
            m.set_rates(door, 11, 22, 33).unwrap();
            m.set_support(door, Some((0, 1, 0, 5))).unwrap();
            m.set_phys_req(
                door,
                PhysRequest {
                    lin: IVec3 { x: 3, y: 0, z: 0 },
                    ang: IVec3::ZERO,
                },
            )
            .unwrap();
            m.set_attach_local(door, Some(IVec3 { x: 8, y: 9, z: 10 }))
                .unwrap();
            m.set_island(door, 3, 7).unwrap();
            m.set_sim_lod(door, SimLod::Far).unwrap();
            m.set_qty(door, ResourceId(1), 7).unwrap();
            m.add_rel(door, Rel::LockedBy, door).unwrap();
            m.set_affordance(door, opaque, true).unwrap();
            m.append(TraceEvent::new(
                Tick(1),
                TraceBody::Learned { mind, fact: 3 },
            ));
            m.append(TraceEvent::new(
                Tick(1),
                TraceBody::RiteBegan {
                    actor: mind,
                    rite: 1,
                    target: Some(door),
                },
            ));
            m.append(TraceEvent::new(
                Tick(1),
                TraceBody::RiteAdvanced {
                    actor: mind,
                    rite: 1,
                    pc: 2,
                    wait_left: 4,
                },
            ));
        }
        let snap = w.snapshot();
        assert!(snap.view().opaque_closed(door));
        let bytes = snap.encode().unwrap();
        let back = WorldSnapshot::decode(&bytes).unwrap();
        assert_eq!(back.epoch, snap.epoch);
        assert_eq!(back.tick, snap.tick);
        assert_eq!(back.canon_hash, snap.canon_hash);
        assert_eq!(back.trace_prefix_hash, snap.trace_prefix_hash);
        let v = back.view();
        assert_eq!(v.pose(door), Some(pose));
        assert_eq!(v.vel(door), Some((vel, 11)));
        assert_eq!(v.rates(door), Some((11, 22, 33)));
        assert_eq!(v.support(door), Some((0, 1, 0, 5)));
        assert_eq!(
            v.phys_req(door),
            Some(PhysRequest {
                lin: IVec3 { x: 3, y: 0, z: 0 },
                ang: IVec3::ZERO,
            })
        );
        assert_eq!(v.attach_local(door), Some(IVec3 { x: 8, y: 9, z: 10 }));
        assert_eq!(v.island(door), Some((3, 7)));
        assert_eq!(v.sim_lod(door), SimLod::Far);
        assert_eq!(v.qty(door, ResourceId(1)), 7);
        assert!(v.has_rel(door, Rel::LockedBy, door));
        assert!(v.knows(mind, 3));
        let (rite, machine) = v.first_rite(mind).expect("rite");
        assert_eq!(rite.0, 1);
        assert_eq!(machine.pc, 2);
        assert_eq!(machine.wait_left, 4);
        assert_eq!(machine.target, Some(door));
        assert!(v.opaque_closed(door));
    }

    #[test]
    fn snapshot_blob_round_trips_pose() {
        let mut w = opaque_world();
        let s = relic(1);
        let pose = PoseMm::new(Mm(42), Mm(1), Mm(7), YawMd(9));
        {
            let mut m = w.mutate();
            m.insert_locus(s, LocusKind::Relic).unwrap();
            m.set_pose(s, pose).unwrap();
        }
        let snap = w.snapshot();
        let back = WorldSnapshot::decode(&snap.encode().unwrap()).unwrap();
        assert_eq!(back.view().pose(s), Some(pose));
        assert_eq!(snap.view().pose(s), Some(pose));
    }

    #[test]
    fn snapshot_blob_round_trips_constraint_state() {
        let mut w = opaque_world();
        let id = relic(9);
        let state = ConstraintState {
            impulse: 42,
            broken: true,
        };
        w.mutate().set_constraint_state(id, state);
        let back = WorldSnapshot::decode(&w.snapshot().encode().unwrap()).unwrap();
        assert_eq!(back.view().constraint_state(id), Some(state));
    }

    #[test]
    fn oversize_knows_and_rite_lists_refused() {
        let s = relic(1);
        let mut row = SnapRow::new(s, LocusKind::Relic);
        row.knows = vec![0; MAX_ROW_KNOWS + 1];
        let err = WorldSnapshot::from_snap_rows(
            klotho_core::Epoch::ZERO,
            Tick::ZERO,
            Hash::ZERO,
            Hash::ZERO,
            None,
            vec![row],
        )
        .unwrap_err();
        assert_eq!(
            err,
            SnapError::Oversize {
                size: MAX_ROW_KNOWS + 1,
                cap: MAX_ROW_KNOWS,
            }
        );
        let mut row = SnapRow::new(s, LocusKind::Relic);
        row.rites = vec![
            (
                1,
                RiteMachine {
                    contact_agency: 0,
                    contact_hit: false,
                    started_at: klotho_core::Tick::ZERO,
                    wait_at: klotho_core::Tick::ZERO,
                    pc: 0,
                    wait_left: 0,
                    target: None,
                    wait_ch: None,
                },
            );
            MAX_ROW_RITES + 1
        ];
        let err = WorldSnapshot::from_snap_rows(
            klotho_core::Epoch::ZERO,
            Tick::ZERO,
            Hash::ZERO,
            Hash::ZERO,
            None,
            vec![row],
        )
        .unwrap_err();
        assert_eq!(
            err,
            SnapError::Oversize {
                size: MAX_ROW_RITES + 1,
                cap: MAX_ROW_RITES,
            }
        );
    }
}
