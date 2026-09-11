//! Drift headless slice: two Places, one Driveable, `PilotedBy` driver.
//! Same `CommitKernel` as Hearth/Ash/Ember.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use klotho_canon::cook;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{
    AabbMm, BlobId, Budget, Hash, IVec3, LocusKind, Mm, PlayerId, PoseMm, Tick, YawMd,
};
use klotho_ir::{CanonDiff, IntentDoc, Name, ProvenanceId, Rel, SeedFact, StyleIntent, from_ron};
use klotho_world::World;

/// Drift Canon sketch.
pub const DRIFT_DIFFS: &str = include_str!("../../../crates/klotho-canon/fixtures/drift.ron");

/// Cook Drift and seed Place A, the driver, and one vehicle.
#[must_use]
pub fn boot() -> CommitKernel {
    let doc = drift_doc();
    let canon = cook(&doc).expect("Drift must cook");
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    apply_seed(&mut k, &doc);
    let player = k.canon().pin("player").expect("player pin");
    k.bind_player(PlayerId(0), player);
    k
}

/// Intent document for the Drift strip.
#[must_use]
pub fn drift_doc() -> IntentDoc {
    let canon_diffs: Vec<CanonDiff> = from_ron(DRIFT_DIFFS).expect("Drift diffs");
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs,
        seed: drift_seed(),
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

/// Replay recorded player packets.
pub fn replay(
    k: &mut CommitKernel,
    intents: &[klotho_ir::PlayerIntent],
) -> Vec<klotho_trace::TraceDelta> {
    let mut out = Vec::with_capacity(intents.len());
    for pi in intents {
        let mut p = pi.clone();
        p.at = k.world().tick();
        k.ingest(Proposal::Player(p));
        out.push(
            k.step(Tick(1), Budget::AAA_ADVENTURE, &mut [])
                .expect("kernel"),
        );
    }
    out
}

/// Packed pin.
#[must_use]
pub fn pin(k: &CommitKernel, name: &str) -> klotho_core::Sigil {
    k.canon().pin(name).unwrap_or_else(|| panic!("pin {name}"))
}

fn drift_seed() -> Vec<SeedFact> {
    vec![
        locus("place_a", LocusKind::Place),
        locus("place_b", LocusKind::Place),
        locus("player", LocusKind::Actor),
        locus("vehicle", LocusKind::Relic),
        SeedFact::Rel {
            a: Name::from("player"),
            rel: Rel::In,
            b: Name::from("place_a"),
        },
        SeedFact::Rel {
            a: Name::from("vehicle"),
            rel: Rel::In,
            b: Name::from("place_a"),
        },
    ]
}

fn apply_seed(k: &mut CommitKernel, doc: &IntentDoc) {
    for fact in &doc.seed {
        match fact {
            SeedFact::Locus { name, kind } => {
                // Place B arrives through Residency, not the live prefix.
                if name.as_str() == "place_b" {
                    continue;
                }
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
    on(k, "vehicle", "Driveable");
    apply_hulls(k);
}

fn apply_hulls(k: &mut CommitKernel) {
    let place_a = pin(k, "place_a");
    k.world_mut()
        .set_pose(place_a, PoseMm::default())
        .expect("place_a pose");
    k.world_mut()
        .set_hull(place_a, place_a_floor(), BlobId::ZERO)
        .expect("place_a hull");

    let player = pin(k, "player");
    k.world_mut()
        .set_pose(player, PoseMm::new(Mm(1_000), Mm(0), Mm(0), YawMd::ZERO))
        .expect("player pose");
    k.world_mut()
        .set_hull(player, actor_hull(), BlobId::ZERO)
        .expect("player hull");

    let vehicle = pin(k, "vehicle");
    k.world_mut()
        .set_pose(vehicle, PoseMm::default())
        .expect("vehicle pose");
    k.world_mut()
        .set_hull(vehicle, vehicle_hull(), BlobId::ZERO)
        .expect("vehicle hull");
}

fn place_a_floor() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -50_000,
            y: -200,
            z: -50_000,
        },
        IVec3 {
            x: 50_000,
            y: 0,
            z: 0,
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

fn vehicle_hull() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -400,
            y: 0,
            z: -800,
        },
        IVec3 {
            x: 400,
            y: 400,
            z: 800,
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
