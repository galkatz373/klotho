//! Hearth slice: Appendix A Canon, headless goldens (PR 07b), pixels (PR 12b).
//!
//! Gameplay does not import Manifest SoA columns (K2).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod visual;

pub use visual::{PixelScene, VisualHearth, boot_visual, golden_camera, stage, write_scene_bmp};

use std::sync::Arc;

use klotho_canon::cook;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{Budget, Hash, LocusKind, MAX_LOCI_HEARTH, PlayerId, Tick};
use klotho_ir::{
    CanonDiff, IntentDoc, MindFact, MindGoal, MindOperator, MindProgram, MindQuery, MindRef,
    MindSpec, MindTarget, Name, ProvenanceId, Rel, SeedFact, StyleIntent, Verb, from_ron,
};
use klotho_world::World;

/// Appendix A Canon diffs (parse-only fixture in `klotho-canon`).
pub const HEARTH_DIFFS: &str =
    include_str!("../../../crates/klotho-canon/fixtures/hearth_diffs.ron");

/// Cook Appendix A, seed the Hearth loci, bind player 0.
#[must_use]
pub fn boot() -> CommitKernel {
    boot_with_locus_cap(MAX_LOCI_HEARTH)
}

/// Cook Appendix A with an explicit runtime locus capacity.
#[must_use]
pub fn boot_with_locus_cap(locus_cap: usize) -> CommitKernel {
    let doc = hearth_doc();
    let canon = cook(&doc).expect("Appendix A must cook");
    let mut k = CommitKernel::new(World::with_locus_cap(
        Arc::new(canon),
        Hash::ZERO,
        locus_cap,
    ));
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
        style: StyleIntent {
            notes: "chunky readable silhouettes".into(),
            palettes: vec![
                Name::from("stone"),
                Name::from("metal"),
                Name::from("organic"),
            ],
            kitbash_tags: vec![
                Name::from("place.hearth.interior"),
                Name::from("door.oak.lockable"),
                Name::from("barrel.oak.portable.flammable"),
                Name::from("npc.human.biped"),
                Name::from("relic.hammer"),
                Name::from("relic.key"),
                Name::from("relic.lockpick"),
                Name::from("relic.bucket"),
                Name::from("relic.ingot"),
                Name::from("prop.anvil"),
                Name::from("prop.forge"),
                Name::from("prop.stool"),
            ],
        },
        canon_diffs,
        seed: hearth_seed(),
        minds: hearth_minds(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

fn hearth_minds() -> Vec<MindSpec> {
    vec![
        MindSpec {
            locus: Name::from("bran"),
            program: MindProgram {
                beat: Some(Name::from("forge_watch")),
                facts: vec![
                    fact(
                        "near_forge",
                        MindQuery::Near {
                            a: MindRef::This,
                            b: MindRef::Pin(Name::from("hearth")),
                            within: klotho_core::Mm(2_500),
                        },
                    ),
                    fact("investigated", MindQuery::Never),
                ],
                operators: vec![
                    op(
                        "return_to_forge",
                        &[],
                        &["near_forge"],
                        Verb::Move,
                        MindTarget::Ref(MindRef::Pin(Name::from("hearth"))),
                    ),
                    op(
                        "investigate",
                        &[],
                        &["investigated"],
                        Verb::Investigate,
                        MindTarget::None,
                    ),
                ],
                goals: vec![
                    goal("stay_near_forge", &["near_forge"], 40),
                    goal("investigate", &["investigated"], 10),
                ],
                far: Vec::new(),
            },
            templates: vec!["{name} won't sell that.".into()],
        },
        MindSpec {
            locus: Name::from("mira"),
            program: MindProgram {
                beat: Some(Name::from("forge_emergency")),
                facts: vec![
                    fact(
                        "burning",
                        MindQuery::AnyQtyAtLeast {
                            res: Name::from("heat"),
                            min: 400,
                        },
                    ),
                    fact(
                        "has_bucket",
                        MindQuery::Related {
                            a: MindRef::Pin(Name::from("bucket")),
                            rel: Rel::WieldedBy,
                            b: MindRef::This,
                        },
                    ),
                    fact("bellows_pumped", MindQuery::Never),
                ],
                operators: vec![
                    MindOperator {
                        id: Name::from("fetch_bucket"),
                        requires: vec![Name::from("burning")],
                        sets: vec![Name::from("has_bucket")],
                        clears: Vec::new(),
                        cost: 1,
                        verb: Verb::Carry,
                        target: MindTarget::Ref(MindRef::Pin(Name::from("bucket"))),
                    },
                    op(
                        "pump_bellows",
                        &[],
                        &["bellows_pumped"],
                        Verb::Investigate,
                        MindTarget::None,
                    ),
                ],
                goals: vec![
                    goal("fetch_bucket", &["has_bucket"], 80),
                    goal("pump_bellows", &["bellows_pumped"], 30),
                ],
                far: Vec::new(),
            },
            templates: Vec::new(),
        },
        MindSpec {
            locus: Name::from("kel"),
            program: MindProgram {
                beat: Some(Name::from("evening_trade")),
                facts: vec![fact("traded", MindQuery::Never)],
                operators: vec![op(
                    "trade",
                    &[],
                    &["traded"],
                    Verb::Talk,
                    MindTarget::Ref(MindRef::Pin(Name::from("ingot"))),
                )],
                goals: vec![goal("evening_trade", &["traded"], 70)],
                far: Vec::new(),
            },
            templates: Vec::new(),
        },
    ]
}

fn fact(id: &str, query: MindQuery) -> MindFact {
    MindFact {
        id: Name::from(id),
        query,
        far_safe: false,
    }
}

fn op(id: &str, requires: &[&str], sets: &[&str], verb: Verb, target: MindTarget) -> MindOperator {
    MindOperator {
        id: Name::from(id),
        requires: requires.iter().map(|n| Name::from(*n)).collect(),
        sets: sets.iter().map(|n| Name::from(*n)).collect(),
        clears: Vec::new(),
        cost: 1,
        verb,
        target,
    }
}

fn goal(id: &str, desired: &[&str], utility: u16) -> MindGoal {
    MindGoal {
        id: Name::from(id),
        desired: desired.iter().map(|n| Name::from(*n)).collect(),
        utility,
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

pub(crate) fn apply_seed(k: &mut CommitKernel, doc: &IntentDoc) {
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
