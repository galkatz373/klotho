//! Hearth headless slice (PR 07b). Appendix A Canon, recorded `PlayerIntent`s.
//!
//! No GPU. Golden 8 (idle locked door sweep) waits on PR 10.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use klotho_canon::cook;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{Budget, Hash, LocusKind, PlayerId, Tick};
use klotho_ir::{CanonDiff, IntentDoc, Name, ProvenanceId, Rel, SeedFact, StyleIntent, from_ron};
use klotho_world::World;

/// Appendix A Canon diffs (parse-only fixture in `klotho-canon`).
pub const HEARTH_DIFFS: &str =
    include_str!("../../../crates/klotho-canon/fixtures/hearth_diffs.ron");

/// Cook Appendix A, seed the Hearth loci, bind player 0.
#[must_use]
pub fn boot() -> CommitKernel {
    let doc = hearth_doc();
    let canon = cook(&doc).expect("Appendix A must cook");
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    apply_seed(&mut k, &doc);
    let player = k.canon().pin("player").expect("player pin");
    k.bind_player(PlayerId(0), player);
    k
}

/// Intent document: Appendix A diffs plus the seed Trace prefix.
#[must_use]
pub fn hearth_doc() -> IntentDoc {
    let canon_diffs: Vec<CanonDiff> = from_ron(HEARTH_DIFFS).expect("Appendix A diffs");
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs,
        seed: hearth_seed(),
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

/// Replay recorded player packets, one tick each. Stamps `at` to the live tick.
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

fn hearth_seed() -> Vec<SeedFact> {
    let mut seed = vec![
        locus("hearth", LocusKind::Place),
        locus("player", LocusKind::Actor),
        locus("bran", LocusKind::Actor),
        locus("mira", LocusKind::Actor),
        locus("kel", LocusKind::Actor),
        locus("oak_door", LocusKind::Relic),
        locus("iron_key", LocusKind::Relic),
        locus("lockpick_tool", LocusKind::Relic),
        locus("fathers_hammer", LocusKind::Relic),
        locus("ingot", LocusKind::Relic),
        locus("bucket", LocusKind::Relic),
        SeedFact::Rel {
            a: Name::from("oak_door"),
            rel: Rel::LockedBy,
            b: Name::from("oak_door"),
        },
        SeedFact::Rel {
            a: Name::from("oak_door"),
            rel: Rel::KeyedBy,
            b: Name::from("iron_key"),
        },
        SeedFact::Rel {
            a: Name::from("lockpick_tool"),
            rel: Rel::WieldedBy,
            b: Name::from("player"),
        },
        SeedFact::Rel {
            a: Name::from("fathers_hammer"),
            rel: Rel::OwnedBy,
            b: Name::from("bran"),
        },
        SeedFact::Rel {
            a: Name::from("ingot"),
            rel: Rel::OwnedBy,
            b: Name::from("player"),
        },
        qty("player", "stamina", 100),
        qty("player", "hands_free", 2),
        qty("player", "copper", 50),
        qty("fathers_hammer", "mass_g", 3_000),
        qty("ingot", "mass_g", 2_000),
        qty("ingot", "copper", 50),
    ];
    for i in 0..9 {
        let n = format!("barrel_{i}");
        seed.push(locus(&n, LocusKind::Relic));
        seed.push(qty(&n, "mass_g", 12_000));
        seed.push(qty(&n, "heat", 0));
        seed.push(SeedFact::Rel {
            a: Name::from(n.as_str()),
            rel: Rel::In,
            b: Name::from("hearth"),
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
    on(k, "oak_door", "Opaque");
    on(k, "oak_door", "Lockable");
    on(k, "fathers_hammer", "Portable");
    on(k, "ingot", "Portable");
    on(k, "lockpick_tool", "Portable");
    for i in 0..9 {
        let n = format!("barrel_{i}");
        on(k, &n, "Portable");
        on(k, &n, "Flammable");
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
