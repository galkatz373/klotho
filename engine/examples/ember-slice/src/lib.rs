//! Ember headless slice: melee WAIT, hit hulls, Cap, SPAWN collapse.
//! Same `CommitKernel` as Hearth/Ash.

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

/// Ember Canon sketch.
pub const EMBER_DIFFS: &str = include_str!("../../../crates/klotho-canon/fixtures/ember.ron");

/// Dummy count the slice must seed.
pub const DUMMY_COUNT: u32 = 32;

/// Cook Ember and seed the arena.
#[must_use]
pub fn boot() -> CommitKernel {
    let doc = ember_doc();
    let canon = cook(&doc).expect("Ember must cook");
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    apply_seed(&mut k, &doc);
    let player = k.canon().pin("player").expect("player pin");
    k.bind_player(PlayerId(0), player);
    k
}

/// Intent document for the Ember arena.
#[must_use]
pub fn ember_doc() -> IntentDoc {
    let canon_diffs: Vec<CanonDiff> = from_ron(EMBER_DIFFS).expect("Ember diffs");
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs,
        seed: ember_seed(),
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
        out.push(k.step(Tick(1), Budget::HEARTH, &mut []).expect("kernel"));
    }
    out
}

/// Packed pin.
#[must_use]
pub fn pin(k: &CommitKernel, name: &str) -> klotho_core::Sigil {
    k.canon().pin(name).unwrap_or_else(|| panic!("pin {name}"))
}

/// Plant `n` live projectile relics (Cap mark).
pub fn plant_projectiles(k: &mut CommitKernel, n: usize) {
    plant_marked(k, n, "Projectile", 10_000);
}

/// Plant `n` live fragment relics (Cap mark).
pub fn plant_fragments(k: &mut CommitKernel, n: usize) {
    plant_marked(k, n, "Fragment", 20_000);
}

fn plant_marked(k: &mut CommitKernel, n: usize, aff: &str, base: u128) {
    let kind = LocusKind::Relic;
    let aff = k
        .canon()
        .affordance_id(aff)
        .unwrap_or_else(|| panic!("{aff}"));
    for i in 0..n {
        let s = klotho_core::Sigil::pack(kind, 0, base + i as u128).expect("sigil");
        k.world_mut().insert_locus(s, kind).expect("plant");
        k.world_mut().set_affordance(s, aff, true).expect("mark");
    }
}

fn ember_seed() -> Vec<SeedFact> {
    let mut seed = vec![
        locus("arena", LocusKind::Place),
        locus("player", LocusKind::Actor),
        locus("dummy_0_hitbox", LocusKind::Relic),
        locus("crate", LocusKind::Relic),
    ];
    for i in 0..DUMMY_COUNT {
        seed.push(locus(&format!("dummy_{i}"), LocusKind::Actor));
    }
    seed.push(qty("player", "ammo", 10));
    seed.push(qty("player", "health", 100));
    seed.push(qty("crate", "integrity", 25));
    for i in 0..DUMMY_COUNT {
        seed.push(qty(&format!("dummy_{i}"), "health", 100));
    }
    seed.extend([
        SeedFact::Rel {
            a: Name::from("crate"),
            rel: Rel::PartOf,
            b: Name::from("crate"),
        },
        SeedFact::Rel {
            a: Name::from("dummy_0_hitbox"),
            rel: Rel::PartOf,
            b: Name::from("dummy_0"),
        },
        SeedFact::Rel {
            a: Name::from("crate"),
            rel: Rel::In,
            b: Name::from("arena"),
        },
        SeedFact::Rel {
            a: Name::from("dummy_0_hitbox"),
            rel: Rel::In,
            b: Name::from("arena"),
        },
    ]);
    for i in 0..DUMMY_COUNT {
        let n = format!("dummy_{i}");
        seed.push(SeedFact::Rel {
            a: Name::from(n.as_str()),
            rel: Rel::In,
            b: Name::from("arena"),
        });
    }
    seed
}

fn apply_seed(k: &mut CommitKernel, doc: &IntentDoc) {
    for fact in &doc.seed {
        match fact {
            SeedFact::Physics { .. } => {} // Configuration was bound by Canon cook.

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
    on(k, "player", "Armed");
    on(k, "crate", "Destructible");
    on(k, "crate", "Hittable");
    on(k, "dummy_0_hitbox", "Hittable");
    for i in 0..DUMMY_COUNT {
        on(k, &format!("dummy_{i}"), "Hittable");
    }
    apply_hulls(k);
}

fn apply_hulls(k: &mut CommitKernel) {
    let player = pin(k, "player");
    k.world_mut()
        .set_pose(player, PoseMm::default())
        .expect("player pose");
    k.world_mut()
        .set_hull(
            player,
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
            ),
            BlobId::ZERO,
        )
        .expect("player hull");

    let dummy = pin(k, "dummy_0");
    k.world_mut()
        .set_pose(dummy, PoseMm::default())
        .expect("dummy pose");
    k.world_mut()
        .set_hull(
            dummy,
            AabbMm::new(
                IVec3 {
                    x: 8000,
                    y: 0,
                    z: -200,
                },
                IVec3 {
                    x: 8400,
                    y: 1800,
                    z: 200,
                },
            ),
            BlobId::ZERO,
        )
        .expect("dummy hull");

    let hitbox = pin(k, "dummy_0_hitbox");
    k.world_mut()
        .set_pose(hitbox, PoseMm::new(Mm(2000), Mm(0), Mm(0), YawMd::ZERO))
        .expect("hitbox pose");
    k.world_mut()
        .set_hull(
            hitbox,
            AabbMm::new(
                IVec3 {
                    x: -200,
                    y: 1200,
                    z: -200,
                },
                IVec3 {
                    x: 200,
                    y: 1600,
                    z: 200,
                },
            ),
            BlobId::ZERO,
        )
        .expect("hitbox hull");
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

fn qty(of: &str, res: &str, value: i32) -> SeedFact {
    SeedFact::Qty {
        of: Name::from(of),
        res: Name::from(res),
        value,
    }
}
