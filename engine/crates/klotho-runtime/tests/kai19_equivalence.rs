//! KAI-19 finite slice-corpus reference/optimized Trace equality.

use klotho_commit::Proposal;
use klotho_compile::{CompileError, TraceRun, WholeTitleRequest, cook_whole_title};
use klotho_core::{Budget, Hash, Tick};
use klotho_ir::{
    Agency, Analog, IntentDoc, IntentTarget, Name, PlayerId, PlayerIntent, SeedFact, Verb,
};
use klotho_world::World;
use std::sync::Arc;

fn run(cooked: &klotho_compile::Cooked) -> Result<Vec<TraceRun>, CompileError> {
    let mut idle = klotho_runtime::kernel_from_cooked_profile(
        cooked,
        klotho_runtime::RuntimeProfile::AaaAdventure,
    )
    .map_err(CompileError::Optimization)?;
    let idle_delta = idle
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [])
        .map_err(|error| CompileError::Optimization(error.to_string()))?;

    let mut input = klotho_runtime::kernel_from_cooked_profile(
        cooked,
        klotho_runtime::RuntimeProfile::AaaAdventure,
    )
    .map_err(CompileError::Optimization)?;
    input.ingest(Proposal::Player(PlayerIntent {
        player: PlayerId(0),
        at: Tick(0),
        verb: Verb::Time,
        target: IntentTarget::None,
        analog: Analog::default(),
        agency: Agency::none(),
    }));
    let input_delta = input
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [])
        .map_err(|error| CompileError::Optimization(error.to_string()))?;

    Ok(vec![
        TraceRun {
            case: Name::from("idle"),
            deltas: vec![idle_delta],
            terminal_prefix: idle.world().trace_prefix_hash(),
        },
        TraceRun {
            case: Name::from("public-input"),
            deltas: vec![input_delta],
            terminal_prefix: input.world().trace_prefix_hash(),
        },
    ])
}

#[test]
fn locked_release_slices_are_trace_equivalent_under_optimized_cook() {
    let corpus: [(&str, IntentDoc); 4] = [
        ("hearth", hearth_slice::hearth_doc()),
        ("ash", ash_slice::ash_doc()),
        ("drift", drift_slice::drift_doc()),
        ("chorus", chorus_slice::chorus_doc()),
    ];
    for (name, doc) in corpus {
        let cooked = cook_whole_title(&doc, &WholeTitleRequest::default(), run)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(cooked.equivalence.cases.len(), 2, "{name}");
        assert_eq!(
            cooked.reference.canon.laws.len(),
            cooked.optimized.canon.laws.len(),
            "{name}: stable Law ids"
        );
        assert_eq!(
            cooked.reference.canon.rites.len(),
            cooked.optimized.canon.rites.len(),
            "{name}: stable Rite ids"
        );
    }
}

fn run_canon(doc: &IntentDoc, intern: bool) -> Vec<TraceRun> {
    let mut canon = klotho_canon::cook(doc).unwrap();
    if intern {
        canon.intern_predicates();
    }
    let mut kernel = klotho_commit::CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    for fact in &doc.seed {
        match fact {
            SeedFact::Physics { .. } | SeedFact::ContactTrack { .. } => {} // Configuration was bound by Canon cook.

            SeedFact::Locus { name, kind } => {
                let sigil = kernel.canon().pin(name.as_str()).unwrap();
                kernel.world_mut().insert_locus(sigil, *kind).unwrap();
            }
            SeedFact::Rel { a, rel, b } => {
                let a = kernel.canon().pin(a.as_str()).unwrap();
                let b = kernel.canon().pin(b.as_str()).unwrap();
                kernel.world_mut().add_rel(a, *rel, b).unwrap();
            }
            SeedFact::Qty { of, res, value } => {
                let of = kernel.canon().pin(of.as_str()).unwrap();
                let res = kernel.canon().resource_id(res.as_str()).unwrap();
                kernel.world_mut().set_qty(of, res, *value).unwrap();
            }
            SeedFact::Pose { of, pose } => {
                let of = kernel.canon().pin(of.as_str()).unwrap();
                kernel.world_mut().set_pose(of, *pose).unwrap();
            }
        }
    }
    kernel.bind_player(PlayerId(0), kernel.canon().pin("player").unwrap());
    let delta = kernel
        .step(Tick(1), Budget::AAA_ADVENTURE, &mut [])
        .unwrap();
    vec![TraceRun {
        case: Name::from("ember-idle"),
        deltas: vec![delta],
        terminal_prefix: kernel.world().trace_prefix_hash(),
    }]
}

#[test]
fn ember_predicate_interning_preserves_trace_with_large_collapse_rite() {
    let doc = ember_slice::ember_doc();
    klotho_compile::compare_trace_runs(&run_canon(&doc, false), &run_canon(&doc, true)).unwrap();
}
