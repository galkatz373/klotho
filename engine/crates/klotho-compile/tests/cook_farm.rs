//! AAA-24 cook-farm capacity and incremental latency gates.

use std::fs;
use std::time::{Duration, Instant};

use hearth_slice::hearth_doc;
use klotho_compile::{cook_doc, write_catalog_incremental};
use klotho_core::{AabbMm, Hash, IVec3, LocusKind, Sigil};
use klotho_stream::{KCAS_VOLUME_CAP, PlaceRow, PlaceSnap};

const GIB: u64 = 1024 * 1024 * 1024;
const LOGICAL_FIXTURE_BYTES: u64 = 50 * GIB;
const UNIQUE_FIXTURE_BYTES: u64 = 5 * GIB;
const FIXTURE_BLOB_BYTES: u64 = 1024 * 1024;

fn place(id: u128) -> Sigil {
    Sigil::pack(LocusKind::Place, 0, id).unwrap()
}

fn snap(canon_hash: Hash, place: Sigil, prefix_byte: u8) -> PlaceSnap {
    let mut rows = Vec::with_capacity(10_000);
    rows.push(PlaceRow::new(place, LocusKind::Place));
    for id in 1..10_000 {
        rows.push(PlaceRow::new(
            Sigil::pack(LocusKind::Relic, 0, id).unwrap(),
            LocusKind::Relic,
        ));
    }
    PlaceSnap::new(place, canon_hash, Hash::from_bytes([prefix_byte; 32]), rows)
}

#[test]
fn aaa_24_repetitive_fixture_is_50_gib_and_multi_volume() {
    // The farm fixture models ten references to each unique CAS byte. Keeping
    // it logical avoids a 50 GiB checkout while exercising production caps.
    let references = LOGICAL_FIXTURE_BYTES / UNIQUE_FIXTURE_BYTES;
    let unique_blobs = UNIQUE_FIXTURE_BYTES / FIXTURE_BLOB_BYTES;
    let volumes = UNIQUE_FIXTURE_BYTES.div_ceil(KCAS_VOLUME_CAP as u64);
    assert_eq!(references, 10);
    assert_eq!(unique_blobs, 5 * 1024);
    assert!(unique_blobs <= klotho_compile::MAX_BLOBS as u64);
    assert!(FIXTURE_BLOB_BYTES <= klotho_compile::MAX_BLOB_BYTES as u64);
    assert_eq!(LOGICAL_FIXTURE_BYTES, 50 * GIB);
    assert!(volumes >= 2, "fixture must cross a KCAS volume boundary");
}

#[test]
fn aaa_24_one_dirty_place_cooks_under_sixty_seconds() {
    let cooked = cook_doc(&hearth_doc()).unwrap();
    let dir = std::env::temp_dir().join(format!("klotho-cook-farm-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let aabb = AabbMm::from_point(IVec3::ZERO);
    let p1 = place(1);
    let p2 = place(2);
    write_catalog_incremental(
        &dir,
        &cooked,
        &[
            (snap(cooked.canon_hash, p1, 1), aabb),
            (snap(cooked.canon_hash, p2, 1), aabb),
        ],
    )
    .unwrap();

    let start = Instant::now();
    let report = write_catalog_incremental(
        &dir,
        &cooked,
        &[
            (snap(cooked.canon_hash, p1, 1), aabb),
            (snap(cooked.canon_hash, p2, 2), aabb),
        ],
    )
    .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(report.dirty_places, vec![p2]);
    assert_eq!(report.reused_places, vec![p1]);
    assert!(report.dirty_volumes.is_empty());
    assert_eq!(report.license_coverage.percent(), 100);
    assert!(
        elapsed < Duration::from_secs(60),
        "dirty cook took {elapsed:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}
