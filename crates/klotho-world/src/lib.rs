//! World is a view of `(canon_hash, trace_prefix_hash)` plus the live Intent
//! heap. Projection columns (including `VelTable`, `IslandTable`, `space_ix`)
//! are derived. Snapshots are checkpoints of Trace, not a second world.
//!
//! Write path is `WorldMut` behind feature `mutate` (crate unit tests also see
//! it). `klotho-commit` is the only runtime crate that enables the feature.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod grid;
mod heap;
mod mutate;
mod proj;
mod spec;
mod view;
mod world;

pub use error::WorldError;
pub use grid::{CELL_MM, GridIndex, world_aabb};
pub use heap::IntentHeap;
#[cfg(any(test, feature = "mutate"))]
pub use mutate::WorldMut;
pub use proj::{Projection, RiteMachine};
#[cfg(any(test, feature = "mutate"))]
pub use spec::SpecDelta;
pub use view::WorldView;
pub use world::{World, WorldSnapshot};

/// Hard locus cap (HLD v1 stand-in). Admission still goes through `space_ix`.
pub const MAX_LOCI: usize = 4_096;
/// Snapshot blob cap. Hearth is ~1–2 MB.
pub const SNAPSHOT_CAP: usize = 16 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{
        AabbMm, AffordanceId, BlobId, Hash, IVec3, LocusKind, Mm, PoseMm, ResourceId, Sigil, Tick,
        YawMd,
    };
    use klotho_ir::{CanonDiff, Rel, from_ron};
    use klotho_trace::{TraceBody, TraceEvent};

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

    fn opaque_world() -> World {
        let diffs: Vec<CanonDiff> = from_ron(
            r#"[AddAffordance(Affordance(id: "Opaque", requires: [], grants: [], conflicts: []))]"#,
        )
        .unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        World::new(Arc::new(canon), Hash::ZERO)
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
}
