//! KAI-20: Tapestry multi-Place vertical-route acceptance.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use klotho_ai::{
    AgentRole, AuthorOp, DeterministicFakeBackend, KlothoAi, PatternArg, PatternInstance,
    RequestBudget,
};
use klotho_author::{flatten_bundle, load_file};
use klotho_commit::Proposal;
use klotho_compile::{
    AccessibilitySettings, CompileError, CreditsRoll, DESKTOP_SKUS, HudSpec, LocaleTable,
    PlacePlanInput, QualityTier, REQUIRED_LOCALE_KEYS, ShaderPerm, ShipContent, SkuPlanInput,
    TapestryStress, TraceRun, WholeTitleRequest, cook_doc, cook_whole_title, estimate_cost,
    install_package, pack_optimized_desktop, place_sigil, select_tier, uninstall_package,
};
use klotho_core::{Budget, PlayerId, Tick};
use klotho_eval::{
    AutomatedPlayer, BotManifest, BudgetTarget, CheckLayer, EvidenceBuilder, EvidenceContext,
    InvariantRef, JourneyHost, JourneySpec, QualityTarget, RouteHost, ScriptHost, SemanticClaim,
    reachability, run_journey, steps_are_public_input,
};
use klotho_ir::{
    Agency, Analog, CanonDiff, IntentDoc, IntentTarget, Name, ParameterValue, PlayerIntent,
    SeedFact, Verb, migrate_doc, to_ron,
};
use klotho_pattern::greybox_route;
use klotho_prove::hash_bytes;
use klotho_runtime::RuntimeProfile;
use serde::Deserialize;

use crate::{AcceptanceEditor, Assumption, RequestDraft};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TapestryRequest {
    outcome: String,
    request: String,
    assumption: String,
    duration_minutes: u32,
    auth_hz: u32,
    present_hz: (u16, u16),
    player: String,
    start_place: String,
    passage: String,
    key: String,
    foe: String,
    checkpoint: String,
    places: Vec<String>,
    systems: Vec<String>,
    locales: Vec<String>,
    skus: Vec<String>,
    journeys: Vec<String>,
    patterns: Vec<PatternRow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatternRow {
    instance: String,
    pattern: String,
    version: u32,
    args: Vec<(String, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseRecord {
    human_completion: HumanCompletion,
    reproduction: Reproduction,
    assets: Vec<AssetApproval>,
    gates: Vec<ReleaseGate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HumanCompletion {
    reviewer: String,
    journey: String,
    completed: bool,
    recording_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reproduction {
    team: String,
    from_brief_and_approved_refs: bool,
    locked_lane: String,
    completed: bool,
    elapsed_minutes: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetApproval {
    id: String,
    class: String,
    approval: String,
    rights: String,
    evidence: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseGate {
    id: String,
    passed: bool,
    used: i32,
    cap: i32,
    evidence: String,
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/tapestry-slice")
}

fn scratch() -> PathBuf {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "klotho-tapestry-{}-{nanos}-{serial}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn read<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> T {
    klotho_ir::from_ron(&fs::read_to_string(path).unwrap()).unwrap()
}

fn parameter(key: &str, value: &str) -> PatternArg {
    PatternArg {
        key: Name::from(key),
        value: ParameterValue::Name(Name::from(value)),
    }
}

fn no_rust_below(path: &Path) -> bool {
    fs::read_dir(path).unwrap().all(|entry| {
        let path = entry.unwrap().path();
        if path.is_dir() {
            no_rust_below(&path)
        } else {
            path.extension().and_then(|ext| ext.to_str()) != Some("rs")
        }
    })
}

fn no_placeholders(path: &Path) {
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            no_placeholders(&entry.unwrap().path());
        }
        return;
    }
    if path.file_name().and_then(|name| name.to_str()) == Some("README.md") {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let lower = text.to_ascii_lowercase();
    for needle in ["todo", "tbd", "fixme", "placeholder", "lorem ipsum", "xxxx"] {
        assert!(
            !lower.contains(needle),
            "{} contains {needle}",
            path.display()
        );
    }
}

fn runtime_runs(cooked: &klotho_compile::Cooked) -> Result<Vec<TraceRun>, CompileError> {
    let run = |with_input: bool| -> Result<TraceRun, CompileError> {
        let mut kernel =
            klotho_runtime::kernel_from_cooked_profile(cooked, RuntimeProfile::AaaAdventure)
                .map_err(CompileError::Optimization)?;
        if with_input {
            kernel.ingest(Proposal::Player(PlayerIntent {
                player: PlayerId(0),
                at: Tick::ZERO,
                verb: Verb::Time,
                target: IntentTarget::None,
                analog: Analog::default(),
                agency: Agency::none(),
            }));
        }
        let delta = kernel
            .step(Tick(1), Budget::AAA_ADVENTURE, &mut [])
            .map_err(|error| CompileError::Optimization(error.to_string()))?;
        Ok(TraceRun {
            case: Name::from(if with_input { "public-input" } else { "idle" }),
            deltas: vec![delta],
            terminal_prefix: kernel.world().trace_prefix_hash(),
        })
    };
    Ok(vec![run(false)?, run(true)?])
}

fn whole_title_request(doc: &IntentDoc) -> WholeTitleRequest {
    let cooked = cook_doc(doc).unwrap();
    let assets: Vec<_> = cooked
        .bindings
        .iter()
        .flat_map(|binding| [binding.mesh, binding.hull])
        .collect();
    let plan = greybox_route();
    WholeTitleRequest {
        places: plan
            .places
            .iter()
            .enumerate()
            .map(|(index, place)| PlacePlanInput {
                place: place_sigil(&place.name),
                aabb: place.envelope,
                assets: assets.clone(),
                access_group: u16::try_from(index / 2).unwrap(),
            })
            .collect(),
        skus: DESKTOP_SKUS
            .iter()
            .map(|sku| SkuPlanInput {
                sku: Name::from(sku.id),
                tier: QualityTier::High,
                used_permutations: vec![ShaderPerm::UNLIT],
            })
            .collect(),
        auxiliary_files: BTreeMap::from([
            ("runtime/tapestry-route.ron".into(), b"route-v1".to_vec()),
            ("studio/private-reference.png".into(), b"private".to_vec()),
        ]),
    }
}

fn ship_content(root: &Path) -> ShipContent {
    ShipContent {
        locales: ["en", "ja", "es"]
            .map(|id| read(root.join("locales").join(format!("{id}.ron"))))
            .into_iter()
            .collect::<Vec<LocaleTable>>(),
        credits: read::<CreditsRoll>(root.join("credits.ron")),
        accessibility: read::<AccessibilitySettings>(root.join("accessibility.ron")),
        hud: read::<HudSpec>(root.join("hud.ron")),
        play_recording: fs::read_to_string(root.join("play/human-critical.ron")).unwrap(),
        notice: "Release-cleared Klotho library and commissioned Tapestry media. No model weights."
            .into(),
    }
}

fn build_candidate(
    root: &Path,
) -> (
    TapestryRequest,
    klotho_ai::ChangeId,
    klotho_ai::ReviewPackage,
    IntentDoc,
) {
    let spec: TapestryRequest = read(root.join("request.ron"));
    let base_path = root.join("base.kdown");
    let base_doc = load_file(&base_path).unwrap();
    let bundle = migrate_doc(Name::from("session"), Name::from("main"), base_doc).unwrap();
    let module = bundle.modules[0].anchor;
    let operations = spec
        .patterns
        .iter()
        .map(|row| AuthorOp::Instantiate {
            instance: PatternInstance {
                anchor: module.child(format!("pattern:{}", row.instance).as_bytes()),
                module,
                instance: Name::from(row.instance.as_str()),
                pattern: Name::from(row.pattern.as_str()),
                version: row.version,
                args: row.args.iter().map(|(k, v)| parameter(k, v)).collect(),
            },
        })
        .collect();
    let temp = scratch();
    let backend = DeterministicFakeBackend::new(
        "tapestry-reproduction",
        vec![Ok(klotho_ai::BackendResponse {
            operations,
            summary: "Tapestry vertical-route candidate".into(),
            output_tokens: 96,
            cost_micro_usd: 0,
        })],
    );
    let mut ai = KlothoAi::new(&temp.join("workspace"), &base_path).unwrap();
    ai.models.register(backend);
    let journeys: Vec<JourneySpec> = spec
        .journeys
        .iter()
        .map(|name| read(root.join("journeys").join(name)))
        .collect();
    let request = RequestDraft {
        text: spec.request.clone(),
        assumptions: vec![Assumption {
            text: spec.assumption.clone(),
            affects_behavior: true,
            accepted: Some(true),
        }],
        acceptance: AcceptanceEditor {
            claims: vec![SemanticClaim {
                id: Name::from("tapestry_vertical_route"),
                text: "Eight-Place action-adventure route is evidence-complete.".into(),
            }],
            journeys: journeys.iter().map(|journey| journey.id.clone()).collect(),
            invariants: vec![InvariantRef {
                id: Name::from("K21.atomic_admission"),
            }],
            quality: vec![QualityTarget {
                id: Name::from("tapestry_style_constitution"),
                reference: Name::from("blue_stone_brass_v1"),
            }],
            budgets: vec![BudgetTarget {
                id: Name::from("title_rust_files"),
                cap: 0,
            }],
            non_regression: Vec::new(),
            scope: klotho_ai::ChangeScope {
                modules: [module].into_iter().collect(),
                anchors: BTreeSet::new(),
            },
        },
        role: AgentRole::Gameplay,
        budget: RequestBudget::default(),
    }
    .submit(&mut ai)
    .unwrap();
    let progress = ai.drive(request, 1).unwrap();
    let change = progress.change.unwrap();
    let review = ai.review(change).unwrap();
    let (_, snapshot) = ai.candidate_snapshots(change).unwrap();
    let doc = flatten_bundle(&snapshot.bundle()).unwrap().doc;
    drop(ai);
    let _ = fs::remove_dir_all(temp);
    (spec, change, review, doc)
}

#[test]
fn tapestry_is_a_data_only_eight_place_ai_candidate() {
    let root = fixture_root();
    assert!(no_rust_below(&root));
    no_placeholders(&root);
    let (spec, _change, review, doc) = build_candidate(&root);
    assert_eq!(spec.outcome, "KAI-TAPESTRY-001");
    assert_eq!(spec.duration_minutes, 30);
    assert_eq!(spec.auth_hz, RuntimeProfile::AaaAdventure.auth_hz());
    assert_eq!(spec.present_hz, (60, 120));
    assert_eq!(spec.places.len(), 8);
    assert_eq!(spec.start_place, "hub");
    assert_eq!(spec.foe, "kel");
    assert_eq!(spec.systems.len(), 11);
    assert_eq!(spec.locales, ["en", "ja", "es"]);
    assert_eq!(spec.skus, DESKTOP_SKUS.map(|sku| sku.id));
    assert!(!review.diff.ops.is_empty());
    assert!(
        !doc.minds.is_empty(),
        "compiled Mind input must be authored"
    );
    assert!(
        doc.canon_diffs
            .iter()
            .any(|diff| matches!(diff, CanonDiff::AddBeat(_))),
        "the cinematic must lower to a Beat"
    );
    assert!(
        doc.canon_diffs
            .iter()
            .any(|diff| matches!(diff, CanonDiff::AddRite(_))),
        "combat/traversal must lower to Rites"
    );
    let places: BTreeSet<_> = doc
        .seed
        .iter()
        .filter_map(|fact| match fact {
            SeedFact::Locus {
                name,
                kind: klotho_core::LocusKind::Place,
            } => Some(name.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(places, spec.places.into_iter().collect());
}

#[test]
fn route_gameplay_human_and_bounded_bot_complete_through_public_input() {
    let root = fixture_root();
    let (spec, change, _review, doc) = build_candidate(&root);
    let route = greybox_route();
    let reach = reachability(&route).unwrap();
    assert!(reach.critical_complete && reach.optional_reachable);
    assert_eq!(reach.reachable.len(), 8);

    let route_journey: JourneySpec = read(root.join("journeys/route.ron"));
    let mut route_host = RouteHost::from_plan(&route).unwrap();
    run_journey(&mut route_host, &route_journey, hash_bytes(&change.0)).unwrap();
    assert_eq!(route_host.last_state(), "shortcut");

    let gameplay: JourneySpec = read(root.join("journeys/gameplay.ron"));
    let mut gameplay_host = ScriptHost::from_intent(
        &doc,
        &Name::from(spec.player.as_str()),
        &Name::from(spec.passage.as_str()),
        &Name::from(spec.key.as_str()),
        &Name::from(spec.start_place.as_str()),
    )
    .unwrap()
    .with_checkpoint(&Name::from(spec.checkpoint.as_str()));
    run_journey(&mut gameplay_host, &gameplay, hash_bytes(&change.0)).unwrap();

    let human: JourneySpec = read(root.join("play/human-critical.ron"));
    assert!(steps_are_public_input(&human.steps));
    let mut human_host = RouteHost::from_plan(&route).unwrap();
    run_journey(&mut human_host, &human, hash_bytes(&change.0)).unwrap();
    assert_eq!(human_host.last_state(), route_host.last_state());

    let bot_spec: JourneySpec = read(root.join("journeys/bot-reachability.ron"));
    let actions: Vec<_> = bot_spec
        .steps
        .iter()
        .filter_map(|step| match step {
            klotho_eval::JourneyStep::Device { action } => Some(action.clone()),
            _ => None,
        })
        .collect();
    let bot = AutomatedPlayer::new(PlayerId(0));
    let manifest = BotManifest {
        capabilities: [bot_spec.id.clone()].into_iter().collect(),
    };
    let start = RouteHost::from_plan(&route).unwrap();
    let path = bot
        .search(
            &start,
            &actions,
            bot_spec.max_ticks,
            &bot_spec.id,
            &manifest,
            |host| host.last_state() == "shortcut",
        )
        .unwrap();
    assert_eq!(path.len(), 6);
}

#[test]
fn optimized_route_packages_with_exact_save_and_complete_release_evidence() {
    let root = fixture_root();
    let (_spec, change, review, doc) = build_candidate(&root);
    let release: ReleaseRecord = read(root.join("release.ron"));
    assert!(release.human_completion.completed);
    assert_eq!(release.human_completion.journey, "tapestry.human-critical");
    assert!(!release.human_completion.reviewer.is_empty());
    assert_eq!(release.human_completion.recording_hash.len(), 64);
    assert_eq!(
        release.human_completion.recording_hash,
        hash_bytes(&fs::read(root.join("play/human-critical.ron")).unwrap()).to_string()
    );
    assert!(release.reproduction.completed);
    assert!(release.reproduction.from_brief_and_approved_refs);
    assert_eq!(release.reproduction.locked_lane, "kai-dev-a");
    assert!(!release.reproduction.team.is_empty());
    assert!(release.reproduction.elapsed_minutes <= 8 * 60);
    assert_eq!(release.assets.len(), 7);
    let asset_classes: BTreeSet<_> = release
        .assets
        .iter()
        .map(|asset| asset.class.as_str())
        .collect();
    assert_eq!(
        asset_classes,
        BTreeSet::from([
            "final_animation",
            "final_audio",
            "final_character",
            "final_dialogue",
            "final_environment",
            "final_vfx",
        ])
    );
    assert!(release.assets.iter().all(|asset| {
        !asset.id.is_empty()
            && asset.class.starts_with("final_")
            && !asset.approval.is_empty()
            && asset.rights == "release-cleared"
            && !asset.evidence.is_empty()
    }));
    assert!(release.gates.iter().all(|gate| {
        !gate.id.is_empty() && gate.passed && !gate.evidence.is_empty() && gate.used <= gate.cap
    }));

    let request = whole_title_request(&doc);
    let cooked = cook_whole_title(&doc, &request, runtime_runs).unwrap();
    cooked.optimized.dag.exportable().unwrap();
    klotho_dialogue::observatory().validate_release().unwrap();
    assert_eq!(cooked.plan.places.len(), 8);
    assert_eq!(cooked.plan.skus.len(), 3);
    assert!(
        cooked
            .equivalence
            .cases
            .contains(&Name::from("public-input"))
    );
    assert!(
        cooked
            .plan
            .stripped_paths
            .contains(&"studio/private-reference.png".into())
    );
    assert_eq!(
        select_tier(TapestryStress::high_gate(), QualityTier::High),
        QualityTier::High
    );
    let present = estimate_cost(TapestryStress::high_gate(), QualityTier::High);
    assert!(present.us_present <= 11_000);

    let content = ship_content(&root);
    for locale in &content.locales {
        assert!(
            REQUIRED_LOCALE_KEYS
                .iter()
                .all(|key| locale.strings.contains_key(*key))
        );
    }
    let temp = scratch();
    for sku in &DESKTOP_SKUS {
        let package = pack_optimized_desktop(&cooked, sku, &content).unwrap();
        assert!(package.files.contains_key("runtime/whole-title.kopt"));
        assert!(package.files.keys().all(|path| !path.contains("studio")));
        let install = temp.join(sku.id);
        let record = install_package(&package, &install).unwrap();
        let installed =
            klotho_compile::unpack_warp(&fs::read(install.join("game.warp")).unwrap()).unwrap();
        let mut kernel =
            klotho_runtime::kernel_from_cooked_profile(&installed, RuntimeProfile::AaaAdventure)
                .unwrap();
        let snapshot = kernel.snapshot();
        let save = klotho_save::pause_save(&snapshot).unwrap();
        let bytes = klotho_save::encode(&save).unwrap();
        let decoded = klotho_save::decode(&bytes).unwrap();
        klotho_save::load(decoded, snapshot.trace_prefix_hash, snapshot.canon_hash).unwrap();
        uninstall_package(&install, &record).unwrap();
    }

    let expanded = to_ron(&doc).unwrap();
    let mut evidence = EvidenceBuilder::new(EvidenceContext {
        change: hash_bytes(&change.0),
        project_hash: review.diff.current_hash,
        toolchain_hash: hash_bytes(b"tapestry-toolchain-v1"),
        expanded_ir_hash: hash_bytes(expanded.as_bytes()),
        canon_hash: cooked.optimized.canon_hash,
        cas_root: cooked.optimized.cook_hash,
    });
    for gate in &release.gates {
        let layer = if gate.id.starts_with("package") {
            CheckLayer::Package
        } else {
            CheckLayer::Budget
        };
        evidence.record_check(
            layer,
            Name::from(gate.id.as_str()),
            gate.passed,
            hash_bytes(gate.evidence.as_bytes()),
        );
    }
    evidence.record_check(
        CheckLayer::Journey,
        Name::from("bounded_bot_envelope"),
        true,
        hash_bytes(b"tapestry.bot-reachability"),
    );
    evidence.record_approval(
        Name::from(release.human_completion.reviewer.as_str()),
        Name::from("critical_path_completion"),
    );
    let bundle = evidence.seal(None).unwrap();
    bundle.verify_signature().unwrap();
    assert!(bundle.checks().iter().all(|check| check.passed));
    assert!(!bundle.approvals.is_empty());
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn title_release_wave_is_funded_before_tapestry_entry() {
    #[derive(Deserialize)]
    struct Plan {
        entries: Vec<PlanEntry>,
    }
    #[derive(Deserialize)]
    struct PlanEntry {
        id: String,
        funding: String,
        roles: Vec<Role>,
    }
    #[derive(Deserialize)]
    struct Role {
        estimated_fte_weeks: u16,
        available_fte_weeks: u16,
    }
    let root = fixture_root().join("../../..");
    let plan: Plan = read(root.join("planning/kai-program.ron"));
    for id in ["KAI-20", "KAI-21", "KAI-22"] {
        let entry = plan.entries.iter().find(|entry| entry.id == id).unwrap();
        assert_eq!(entry.funding, "approved", "{id} blocks KAI-20 entry");
        assert!(
            entry
                .roles
                .iter()
                .all(|role| role.available_fte_weeks >= role.estimated_fte_weeks)
        );
    }
}
