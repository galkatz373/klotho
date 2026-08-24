//! Ash headless slice (PR 07c / K26). Same `CommitKernel` as Hearth.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use klotho_canon::cook;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{Budget, Hash, LocusKind, PlayerId, Tick};
use klotho_ir::{CanonDiff, IntentDoc, Name, ProvenanceId, Rel, SeedFact, StyleIntent, from_ron};
use klotho_world::World;

/// Appendix B Canon sketch.
pub const ASH_DIFFS: &str = include_str!("../../../crates/klotho-canon/fixtures/ash.ron");

/// Cook Appendix B and seed the arena.
#[must_use]
pub fn boot() -> CommitKernel {
    let doc = ash_doc();
    let canon = cook(&doc).expect("Appendix B must cook");
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    apply_seed(&mut k, &doc);
    let player = k.canon().pin("player").expect("player pin");
    k.bind_player(PlayerId(0), player);
    k
}

/// Intent document for the Ash arena.
#[must_use]
pub fn ash_doc() -> IntentDoc {
    let canon_diffs: Vec<CanonDiff> = from_ron(ASH_DIFFS).expect("Appendix B diffs");
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs,
        seed: ash_seed(),
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
    let kind = LocusKind::Relic;
    let aff = k
        .canon()
        .affordance_id("Projectile")
        .expect("Projectile affordance");
    for i in 0..n {
        // Pins are cook-time; runtime extras use packed sigils.
        let s = klotho_core::Sigil::pack(kind, 0, 10_000 + i as u128).expect("sigil");
        k.world_mut().insert_locus(s, kind).expect("bolt");
        k.world_mut().set_affordance(s, aff, true).expect("mark");
    }
}

fn ash_seed() -> Vec<SeedFact> {
    let mut seed = vec![
        locus("arena", LocusKind::Place),
        locus("player", LocusKind::Actor),
        qty("player", "ammo", 10),
        qty("player", "health", 100),
    ];
    for i in 0..4 {
        let n = format!("dummy_{i}");
        seed.push(locus(&n, LocusKind::Actor));
        seed.push(qty(&n, "health", 100));
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
    for i in 0..4 {
        on(k, &format!("dummy_{i}"), "Hittable");
    }
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
