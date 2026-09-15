//! Chorus headless slice: 2,000 Far + 200 Full SimLod crowd.
//! Same `CommitKernel` as Hearth/Ash/Ember/Drift.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use klotho_canon::cook;
use klotho_commit::CommitKernel;
use klotho_core::{
    AabbMm, BlobId, Hash, IVec3, LocusKind, Mm, NO_ISLAND, PlayerId, PoseMm, Sigil, YawMd,
};
use klotho_interest::{InterestConfig, classify};
use klotho_ir::{CanonDiff, IntentDoc, Name, ProvenanceId, Rel, SeedFact, StyleIntent, from_ron};
use klotho_world::World;

/// Chorus Canon sketch.
pub const CHORUS_DIFFS: &str = include_str!("../../../crates/klotho-canon/fixtures/chorus.ron");

/// Full-ring crowd relics (Chebyshev XZ inside 20 m of the player).
pub const FULL_COUNT: usize = 200;
/// Far-ring crowd relics (inside 80 m, outside 20 m).
pub const FAR_COUNT: usize = 2000;

/// Packed-id base for Full-ring crowd relics.
const FULL_BASE: u128 = 1_000;
/// Packed-id base for Far-ring crowd relics.
const FAR_BASE: u128 = 10_000;
/// Full-ring X, millimetres.
const FULL_X_MM: i32 = 1_000;
/// Far-ring X, millimetres.
const FAR_X_MM: i32 = 40_000;
/// Far OpaqueClosed door Z (overlaps the known 1400→1900 sweep).
const DOOR_Z_MM: i32 = 1_850;

/// Cook Chorus and seed the plaza, player, Far door, and crowd.
#[must_use]
pub fn boot() -> CommitKernel {
    let doc = chorus_doc();
    let canon = cook(&doc).expect("Chorus must cook");
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    apply_seed(&mut k, &doc);
    apply_hulls(&mut k);
    plant_crowd(&mut k);
    apply_lod(&mut k);
    let player = k.canon().pin("player").expect("player pin");
    k.bind_player(PlayerId(0), player);
    k
}

/// Intent document for the Chorus plaza.
#[must_use]
pub fn chorus_doc() -> IntentDoc {
    let canon_diffs: Vec<CanonDiff> = from_ron(CHORUS_DIFFS).expect("Chorus diffs");
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs,
        seed: chorus_seed(),
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

/// Packed pin.
#[must_use]
pub fn pin(k: &CommitKernel, name: &str) -> Sigil {
    k.canon().pin(name).unwrap_or_else(|| panic!("pin {name}"))
}

/// Packed sigil for Full-ring crowd relic `i` (`0..FULL_COUNT`).
#[must_use]
pub fn full_relic(i: usize) -> Sigil {
    Sigil::pack(LocusKind::Relic, 0, FULL_BASE + i as u128).expect("full relic")
}

/// Packed sigil for Far-ring crowd relic `i` (`0..FAR_COUNT`).
#[must_use]
pub fn far_relic(i: usize) -> Sigil {
    Sigil::pack(LocusKind::Relic, 0, FAR_BASE + i as u128).expect("far relic")
}

/// Write SimLod from a pure interest pass.
pub fn apply_lod(k: &mut CommitKernel) {
    let interest = classify(&k.world().view(), &InterestConfig::default());
    let mut w = k.world_mut();
    for (s, lod) in interest.lod {
        w.set_sim_lod(s, lod).expect("lod");
    }
}

/// Plant 200 Full + 2000 Far crowd relics (Relic: Actors are observers).
pub fn plant_crowd(k: &mut CommitKernel) {
    let plaza = pin(k, "plaza");
    let mut w = k.world_mut();
    for i in 0..FULL_COUNT {
        let s = full_relic(i);
        w.insert_locus(s, LocusKind::Relic).expect("plant");
        w.add_rel(s, Rel::In, plaza).expect("in");
        w.set_pose(s, PoseMm::new(Mm(FULL_X_MM), Mm(0), Mm(0), YawMd::ZERO))
            .expect("pose");
        w.set_hull(s, actor_hull(), BlobId::ZERO).expect("hull");
        // id 0 island-wakes; plant at NO_ISLAND.
        w.set_island(s, NO_ISLAND, 0).expect("island");
    }
    for i in 0..FAR_COUNT {
        let s = far_relic(i);
        w.insert_locus(s, LocusKind::Relic).expect("plant");
        w.add_rel(s, Rel::In, plaza).expect("in");
        w.set_pose(s, PoseMm::new(Mm(FAR_X_MM), Mm(0), Mm(0), YawMd::ZERO))
            .expect("pose");
        w.set_hull(s, actor_hull(), BlobId::ZERO).expect("hull");
        w.set_island(s, NO_ISLAND, 0).expect("island");
    }
}

fn chorus_seed() -> Vec<SeedFact> {
    vec![
        locus("plaza", LocusKind::Place),
        locus("player", LocusKind::Actor),
        locus("door", LocusKind::Relic),
        SeedFact::Rel {
            a: Name::from("player"),
            rel: Rel::In,
            b: Name::from("plaza"),
        },
        SeedFact::Rel {
            a: Name::from("door"),
            rel: Rel::In,
            b: Name::from("plaza"),
        },
        SeedFact::Rel {
            a: Name::from("door"),
            rel: Rel::LockedBy,
            b: Name::from("door"),
        },
    ]
}

fn apply_seed(k: &mut CommitKernel, doc: &IntentDoc) {
    for fact in &doc.seed {
        match fact {
            SeedFact::Physics { .. } | SeedFact::ContactTrack { .. } => {} // Configuration was bound by Canon cook.

            SeedFact::Locus { name, kind } => {
                let s = pin(k, name.as_str());
                k.world_mut().insert_locus(s, *kind).expect("locus");
            }
            SeedFact::Rel { a, rel, b } => {
                let sa = pin(k, a.as_str());
                let sb = pin(k, b.as_str());
                k.world_mut().add_rel(sa, *rel, sb).expect("rel");
            }
            SeedFact::Qty { of, res, value } => {
                let s = pin(k, of.as_str());
                let r = k
                    .canon()
                    .resource_id(res.as_str())
                    .unwrap_or_else(|| panic!("resource {}", res.as_str()));
                k.world_mut().set_qty(s, r, *value).expect("qty");
            }
            SeedFact::Pose { of, pose } => {
                let s = pin(k, of.as_str());
                k.world_mut().set_pose(s, *pose).expect("pose");
            }
        }
    }
    on(k, "door", "Opaque");
}

fn apply_hulls(k: &mut CommitKernel) {
    let plaza = pin(k, "plaza");
    k.world_mut()
        .set_pose(plaza, PoseMm::default())
        .expect("plaza pose");
    k.world_mut()
        .set_hull(plaza, plaza_floor(), BlobId::ZERO)
        .expect("plaza hull");

    let player = pin(k, "player");
    k.world_mut()
        .set_pose(player, PoseMm::default())
        .expect("player pose");
    k.world_mut()
        .set_hull(player, actor_hull(), BlobId::ZERO)
        .expect("player hull");

    let door = pin(k, "door");
    k.world_mut()
        .set_pose(
            door,
            PoseMm::new(Mm(FAR_X_MM), Mm(0), Mm(DOOR_Z_MM), YawMd::ZERO),
        )
        .expect("door pose");
    k.world_mut()
        .set_hull(door, door_hull(), BlobId::ZERO)
        .expect("door hull");
    k.world_mut()
        .set_island(door, NO_ISLAND, 0)
        .expect("door island");
}

fn plaza_floor() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -5_000,
            y: -200,
            z: -5_000,
        },
        IVec3 {
            x: 50_000,
            y: 0,
            z: 5_000,
        },
    )
}

fn actor_hull() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -200,
            y: 0,
            z: -200,
        },
        IVec3 {
            x: 200,
            y: 1800,
            z: 200,
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

fn on(k: &mut CommitKernel, locus: &str, aff: &str) {
    let s = pin(k, locus);
    let a = k
        .canon()
        .affordance_id(aff)
        .unwrap_or_else(|| panic!("affordance {aff}"));
    k.world_mut()
        .set_affordance(s, a, true)
        .expect("affordance");
}

fn locus(name: &str, kind: LocusKind) -> SeedFact {
    SeedFact::Locus {
        name: Name::from(name),
        kind,
    }
}
