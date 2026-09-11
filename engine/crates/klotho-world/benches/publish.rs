//! 50k-row pose-column snapshot publish microbench. Not a Hearth fixture.
//!
//! Hulls are bound on a 64-row subset so `PlaceIndex` is non-empty; the timed
//! path is still `World::snapshot()` of the 50k pose column.

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use klotho_canon::cook_diffs;
use klotho_core::{AabbMm, BlobId, Hash, IVec3, LocusKind, Mm, PoseMm, Sigil, YawMd};
use klotho_ir::{CanonDiff, from_ron};
use klotho_world::World;

fn relic(id: u128) -> Sigil {
    Sigil::pack(LocusKind::Relic, 0, id).unwrap()
}

fn hull() -> AabbMm {
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
    )
}

fn world_n(n: usize) -> World {
    let diffs: Vec<CanonDiff> = from_ron(
        r#"[AddAffordance(Affordance(id: "Opaque", requires: [], grants: [], conflicts: []))]"#,
    )
    .unwrap();
    let canon = cook_diffs(&diffs).unwrap();
    let mut w = World::with_locus_cap(Arc::new(canon), Hash::ZERO, n);
    {
        let mut m = w.mutate();
        for i in 0..n as u128 {
            let s = relic(i + 1);
            m.insert_locus(s, LocusKind::Relic).unwrap();
            m.set_pose(s, PoseMm::new(Mm(i as i32), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            if i < 64 {
                m.set_hull(s, hull(), BlobId::ZERO).unwrap();
            }
        }
    }
    w
}

fn main() {
    const N: usize = 50_000;
    const ITERS: u32 = 50;
    let mut w = world_n(N);
    // Warm the first publish so insert cost is not in the sample.
    black_box(w.snapshot());
    let t = Instant::now();
    for _ in 0..ITERS {
        black_box(w.snapshot());
    }
    let clean = t.elapsed();
    w.mutate()
        .set_pose(relic(1), PoseMm::new(Mm(7), Mm(0), Mm(0), YawMd(0)))
        .unwrap();
    let t = Instant::now();
    for _ in 0..ITERS {
        black_box(w.snapshot());
    }
    let dirty = t.elapsed();
    eprintln!("pose-column publish 50k x{ITERS}: clean={clean:?} after-one-dirty-row={dirty:?}");
}
