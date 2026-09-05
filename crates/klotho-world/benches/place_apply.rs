//! 10k-row PlaceSnap apply microbench. Gate: ≤ 2 ms after warmup.
//!
//! Hulls are bound on a 64-row subset so `PlaceIndex` is non-empty; the timed
//! path is still apply of the 10k pose + `Rel::In` rows.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use klotho_canon::cook_diffs;
use klotho_core::{AabbMm, BlobId, Hash, IVec3, LocusKind, Mm, PoseMm, Sigil, YawMd};
use klotho_ir::{CanonDiff, Rel, from_ron};
use klotho_world::{PlaceRow, PlaceSnap, World};

fn relic(id: u128) -> Sigil {
    Sigil::pack(LocusKind::Relic, 0, id).unwrap()
}

fn place(id: u128) -> Sigil {
    Sigil::pack(LocusKind::Place, 0, id).unwrap()
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

fn empty_world(n: usize) -> World {
    let diffs: Vec<CanonDiff> = from_ron(
        r#"[AddAffordance(Affordance(id: "Opaque", requires: [], grants: [], conflicts: []))]"#,
    )
    .unwrap();
    let canon = cook_diffs(&diffs).unwrap();
    World::with_locus_cap(Arc::new(canon), Hash::ZERO, n)
}

fn snap_n(n: usize) -> PlaceSnap {
    let p = place(1);
    let mut rows = Vec::with_capacity(n);
    let mut place_row = PlaceRow::new(p, LocusKind::Place);
    place_row.pose = Some(PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)));
    rows.push(place_row);
    for i in 1..n as u128 {
        let s = relic(i);
        let mut row = PlaceRow::new(s, LocusKind::Relic);
        row.pose = Some(PoseMm::new(Mm(i as i32), Mm(0), Mm(0), YawMd(0)));
        if i < 64 {
            row.hull = Some(hull());
            row.hull_id = BlobId::ZERO;
        }
        row.rels = vec![(Rel::In, p)];
        rows.push(row);
    }
    PlaceSnap::new(p, Hash::ZERO, Hash::ZERO, rows)
}

fn main() -> ExitCode {
    const N: usize = 10_000;
    const WARM: u32 = 8;
    const ITERS: u32 = 16;
    let snap = snap_n(N);
    for _ in 0..WARM {
        let mut w = empty_world(N);
        black_box(w.mutate().apply_place_snap(&snap).unwrap());
    }
    let mut samples = Vec::with_capacity(ITERS as usize);
    for _ in 0..ITERS {
        let mut w = empty_world(N);
        let t = Instant::now();
        black_box(w.mutate().apply_place_snap(&snap).unwrap());
        samples.push(t.elapsed());
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    eprintln!("place-snap apply 10k x{ITERS}: samples={samples:?} median={median:?}");
    // `cargo test --all-targets` executes bench binaries without optimization;
    // the wall-time gate is meaningful only under `cargo bench`'s release profile.
    if cfg!(not(debug_assertions)) && median.as_secs_f64() * 1_000.0 > 2.0 {
        eprintln!("FAIL: 10k-row apply median {median:?} exceeds 2 ms");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
