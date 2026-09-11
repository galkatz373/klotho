//! KAI-11 Mini-Tapestry: AI-authored ship increment, package, and reforecast.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use klotho_ai::{
    AgentRole, AuthorOp, DeterministicFakeBackend, KlothoAi, PatternArg, PatternInstance,
    RequestBudget, RiskPolicy,
};
use klotho_author::{flatten_bundle, load_file};
use klotho_compile::{
    AccessibilitySettings, CreditEntry, CreditsRoll, DESKTOP_SKUS, HudSpec, LocaleTable,
    REQUIRED_LOCALE_KEYS, ShipContent, install_package, pack_desktop, uninstall_package,
    unpack_warp,
};
use klotho_core::{Epoch, Hash};
use klotho_eval::{
    BudgetTarget, CheckLayer, EvidenceBuilder, EvidenceContext, InvariantRef, JourneyHost,
    JourneyId, JourneySpec, QualityTarget, ScriptHost, SemanticClaim, run_journey,
    steps_are_public_input,
};
use klotho_ir::{
    FeelContract, IntentDoc, Name, ParameterValue, Rel, SeedFact, migrate_doc, to_ron,
};
use klotho_kai_bench::{default_root, validate_repository};
use klotho_manifest::{UiManifest, Widget, WidgetKind};
use klotho_prove::hash_bytes;
use klotho_ui::{HudSkin, HudSlot, HudViewport, skin_hud};
use serde::Deserialize;

use crate::{
    AcceptanceEditor, Assumption, CaptureComparison, DistaffReview, EditorSession, RequestDraft,
    ReviewState, conservative_risk_inputs,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MiniTapestryRequest {
    outcome: String,
    request: String,
    assumption: String,
    player: String,
    place: String,
    passage: String,
    key: String,
    foe: String,
    checkpoint: String,
    locales: Vec<String>,
    skus: Vec<String>,
    journeys: Vec<String>,
    patterns: Vec<PatternRow>,
    approved_bindings: Vec<(String, String)>,
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
struct ProgramPlan {
    entries: Vec<PlanEntry>,
}

#[derive(Deserialize)]
struct PlanEntry {
    id: String,
    wave: String,
    funding: String,
    actuals: String,
    roles: Vec<RoleCapacity>,
}

#[derive(Deserialize)]
struct RoleCapacity {
    estimated_fte_weeks: u16,
    available_fte_weeks: u16,
}

fn scratch() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("klotho-mini-tapestry-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/mini-tapestry-slice")
}

fn read_request(root: &Path) -> MiniTapestryRequest {
    let text = fs::read_to_string(root.join("request.ron")).unwrap();
    klotho_ir::from_ron(&text).unwrap()
}

fn read_journeys(root: &Path, names: &[String]) -> Vec<JourneySpec> {
    names
        .iter()
        .map(|name| {
            let text = fs::read_to_string(root.join("journeys").join(name)).unwrap();
            klotho_ir::from_ron(&text).unwrap()
        })
        .collect()
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
    let skip = path.file_name().and_then(|n| n.to_str()) == Some("README.md");
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            no_placeholders(&entry.unwrap().path());
        }
        return;
    }
    if skip {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let lower = text.to_ascii_lowercase();
    for needle in ["todo", "tbd", "fixme", "placeholder", "lorem ipsum", "xxxx"] {
        assert!(
            !lower.contains(needle),
            "{} contains placeholder `{needle}`",
            path.display()
        );
    }
}

fn host_for(doc: &IntentDoc, spec: &MiniTapestryRequest) -> ScriptHost {
    ScriptHost::from_intent(
        doc,
        &Name::from(spec.player.as_str()),
        &Name::from(spec.passage.as_str()),
        &Name::from(spec.key.as_str()),
        &Name::from(spec.place.as_str()),
    )
    .unwrap()
    .with_checkpoint(&Name::from(spec.checkpoint.as_str()))
}

fn ship_content(root: &Path) -> ShipContent {
    let locales = ["en", "ja", "es"]
        .into_iter()
        .map(|id| {
            let text = fs::read_to_string(root.join("locales").join(format!("{id}.ron"))).unwrap();
            klotho_ir::from_ron::<LocaleTable>(&text).unwrap()
        })
        .collect();
    let credits: CreditsRoll =
        klotho_ir::from_ron(&fs::read_to_string(root.join("credits.ron")).unwrap()).unwrap();
    let accessibility: AccessibilitySettings =
        klotho_ir::from_ron(&fs::read_to_string(root.join("accessibility.ron")).unwrap()).unwrap();
    let hud: HudSpec =
        klotho_ir::from_ron(&fs::read_to_string(root.join("hud.ron")).unwrap()).unwrap();
    let play = fs::read_to_string(root.join("play/critical-path.ron")).unwrap();
    ShipContent {
        locales,
        credits,
        accessibility,
        hud,
        play_recording: play,
        notice: "Approved kitbash. No model weights.".into(),
    }
}

#[test]
fn mini_tapestry_is_authored_packaged_and_pinned_without_title_rust() {
    let fixture = fixture_root();
    let spec = read_request(&fixture);
    let journeys = read_journeys(&fixture, &spec.journeys);
    assert_eq!(spec.outcome, "KAI-MINI-TAPESTRY-001");
    assert_eq!(spec.foe, "kel");
    assert!(no_rust_below(&fixture));
    no_placeholders(&fixture);
    assert_eq!(spec.locales, vec!["en", "ja", "es"]);
    assert_eq!(
        spec.skus,
        vec!["win-d3d12-high", "linux-vulkan-high", "mac-metal-high"]
    );
    assert!(
        journeys
            .iter()
            .all(|journey| steps_are_public_input(&journey.steps))
    );

    let base_path = fixture.join("base.kdown");
    let base_doc = load_file(&base_path).unwrap();
    let bundle = migrate_doc(Name::from("session"), Name::from("main"), base_doc.clone()).unwrap();
    let module = bundle.modules[0].anchor;
    let ops: Vec<AuthorOp> = spec
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
    let backend = DeterministicFakeBackend::new(
        "mini-tapestry-fixture",
        vec![Ok(klotho_ai::BackendResponse {
            operations: ops,
            summary: "Mini-Tapestry keep candidate".into(),
            output_tokens: 48,
            cost_micro_usd: 0,
        })],
    );
    let temp = scratch();
    let mut ai = KlothoAi::new(&temp.join("workspace"), &base_path).unwrap();
    ai.models.register(backend);

    let draft = RequestDraft {
        text: spec.request.clone(),
        assumptions: vec![Assumption {
            text: spec.assumption.clone(),
            affects_behavior: true,
            accepted: Some(true),
        }],
        acceptance: AcceptanceEditor {
            claims: vec![SemanticClaim {
                id: Name::from("mini_tapestry_keep"),
                text: "Single-Place keep is playable, saveable, localized, and packable.".into(),
            }],
            journeys: journeys.iter().map(|journey| journey.id.clone()).collect(),
            invariants: vec![InvariantRef {
                id: Name::from("K21.atomic_admission"),
            }],
            quality: vec![QualityTarget {
                id: Name::from("approved_keep_kit"),
                reference: Name::from("kitbash.lock.v1"),
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
    };
    let request = draft.submit(&mut ai).unwrap();
    let initial = ai.drive(request, 1).unwrap();
    let change = initial.change.unwrap();

    let review_package = ai.review(change).unwrap();
    let (_, snapshot) = ai.candidate_snapshots(change).unwrap();
    let repaired_doc = flatten_bundle(&snapshot.bundle()).unwrap().doc;
    let cooked = klotho_author::cook_validated(&repaired_doc).unwrap();
    let recooked = klotho_author::cook_validated(&repaired_doc).unwrap();
    assert_eq!(cooked.cook_hash, recooked.cook_hash);
    let actual_bindings: BTreeSet<_> = cooked
        .bindings
        .iter()
        .map(|binding| (binding.locus.0.clone(), binding.tag.0.clone()))
        .collect();
    let expected_bindings: BTreeSet<_> = spec.approved_bindings.iter().cloned().collect();
    assert!(
        expected_bindings.is_subset(&actual_bindings),
        "missing bindings: {:?}",
        expected_bindings.difference(&actual_bindings)
    );

    let expanded = to_ron(&repaired_doc).unwrap();
    let mut evidence = EvidenceBuilder::new(EvidenceContext {
        change: hash_bytes(&change.0),
        project_hash: review_package.diff.current_hash,
        toolchain_hash: hash_bytes(b"mini-tapestry-toolchain-v1"),
        expanded_ir_hash: hash_bytes(expanded.as_bytes()),
        canon_hash: cooked.canon_hash,
        cas_root: hash_bytes(b"approved-kitbash-v1"),
    });
    evidence.record_check(
        CheckLayer::Schema,
        Name::from("pattern_expansion"),
        true,
        hash_bytes(expanded.as_bytes()),
    );
    for journey in &journeys {
        let mut host = host_for(&repaired_doc, &spec);
        let result = run_journey(&mut host, journey, hash_bytes(&change.0)).unwrap();
        evidence.record_check(
            CheckLayer::Journey,
            Name::from(journey.id.as_str()),
            true,
            result.evidence.signature,
        );
        for point in &journey.capture_points {
            host.check(&klotho_eval::JourneyAssertion::Capture {
                point: point.name.clone(),
            })
            .unwrap();
            evidence.record_capture(
                point.name.clone(),
                hash_bytes(point.name.as_str().as_bytes()),
            );
        }
    }
    evidence.record_check(
        CheckLayer::Budget,
        Name::from("zero_title_rust"),
        true,
        hash_bytes(b"0"),
    );
    let evidence_hash = ai
        .evaluation
        .register_trusted(evidence.seal(None).unwrap())
        .unwrap();
    ai.attach_evidence(change, evidence_hash).unwrap();

    let content = ship_content(&fixture);
    assert_eq!(content.locales.len(), 3);
    for locale in &content.locales {
        for key in REQUIRED_LOCALE_KEYS {
            assert!(
                locale.strings.contains_key(key),
                "{} missing {key}",
                locale.id
            );
        }
    }
    for sku in &DESKTOP_SKUS {
        assert!(spec.skus.iter().any(|id| id == sku.id));
        let package = pack_desktop(&cooked, sku, &content).unwrap();
        let dest = temp.join(format!("install-{}", sku.id));
        let record = install_package(&package, &dest).unwrap();
        let warp = fs::read(dest.join("game.warp")).unwrap();
        let installed = unpack_warp(&warp).unwrap();
        assert_eq!(installed.cook_hash, cooked.cook_hash);
        let play: JourneySpec =
            klotho_ir::from_ron(&fs::read_to_string(dest.join("play/critical-path.ron")).unwrap())
                .unwrap();
        assert_eq!(play.id, JourneyId::from("mini-tapestry-critical-path"));
        let mut host = host_for(&installed.doc, &spec);
        run_journey(&mut host, &play, hash_bytes(&change.0)).unwrap();
        uninstall_package(&dest, &record).unwrap();
    }

    let policy = RiskPolicy::default();
    let risk = conservative_risk_inputs(&review_package.diff, &policy);
    let captures = journeys
        .iter()
        .flat_map(|journey| journey.capture_points.iter())
        .map(|point| CaptureComparison {
            checkpoint: point.name.clone(),
            before: Hash::ZERO,
            after: hash_bytes(point.name.as_str().as_bytes()),
        })
        .collect();
    let mut review =
        DistaffReview::open(&ai, request, change, &policy, &risk, captures, 0).unwrap();
    assert_eq!(review.state, ReviewState::ReadyToPin);
    let mut session = EditorSession::new(base_doc);
    review
        .pin(
            &mut session,
            "approve Mini-Tapestry keep, evidence, and desktop package",
        )
        .unwrap();
    assert_eq!(review.state, ReviewState::Pinned);
    assert!(session.doc().seed.iter().any(|fact| matches!(
        fact,
        SeedFact::Rel { a, rel: Rel::KeyedBy, b }
            if a.as_str() == "oak_door" && b.as_str() == "iron_key"
    )));
}

#[test]
fn production_hud_skins_status_prompt_and_notice() {
    let ui = UiManifest::from_widgets(
        Epoch::ZERO,
        [
            Widget {
                kind: WidgetKind::Bar {
                    value: 80,
                    cap: 100,
                },
                body: "Stamina".into(),
            },
            Widget {
                kind: WidgetKind::Prompt,
                body: "Use".into(),
            },
            Widget {
                kind: WidgetKind::Text,
                body: "The keep is clear".into(),
            },
        ],
    );
    let frame = skin_hud(&ui, HudViewport::new(1920, 1080), HudSkin::default());
    assert!(frame.elements.iter().any(|e| e.slot == HudSlot::Status));
    assert!(frame.elements.iter().any(|e| e.slot == HudSlot::Prompt));
    assert!(frame.elements.iter().any(|e| e.slot == HudSlot::Notice));
    let contrast = skin_hud(&ui, HudViewport::new(1920, 1080), HudSkin::high_contrast());
    assert_ne!(
        contrast.elements[0].foreground,
        frame.elements[0].foreground
    );
}

#[test]
fn kai00_gates_and_tapestry_fan_in_are_funded() {
    let root = default_root();
    let summary = validate_repository(&root).expect("KAI-00 contract");
    assert_eq!(summary.total_outcomes, 350);
    assert_eq!(summary.planned_prs, 25);
    let plan: ProgramPlan =
        klotho_ir::from_ron(&fs::read_to_string(root.join("planning/kai-program.ron")).unwrap())
            .unwrap();
    let mut funded = BTreeMap::new();
    for entry in &plan.entries {
        if matches!(
            entry.id.as_str(),
            "KAI-12"
                | "KAI-13"
                | "KAI-14"
                | "KAI-15"
                | "KAI-16"
                | "KAI-17"
                | "KAI-18"
                | "KAI-19"
                | "KAI-20"
                | "KAI-21"
                | "KAI-22"
        ) {
            assert!(
                entry.actuals.contains("Mini-Tapestry"),
                "{} was not reforecast from Mini-Tapestry",
                entry.id
            );
            for role in &entry.roles {
                assert!(
                    role.available_fte_weeks >= role.estimated_fte_weeks,
                    "{} lacks funded capacity",
                    entry.id
                );
            }
            funded.insert(entry.id.as_str(), entry.funding.as_str());
        }
    }
    assert_eq!(funded.len(), 11);
    let eleven = plan
        .entries
        .iter()
        .find(|e| e.id == "KAI-11")
        .expect("KAI-11");
    assert_eq!(eleven.wave, "first_value");
    assert!(eleven.actuals.contains("Mini-Tapestry"));
}

#[test]
fn feel_fixture_matches_typed_contract() {
    let text = fs::read_to_string(fixture_root().join("feel.ron")).unwrap();
    let loaded: FeelContract = klotho_ir::from_ron(&text).unwrap();
    assert_eq!(loaded.action.as_str(), "use");
    assert!(loaded.accessibility.reduce_shake);
    assert!(loaded.accessibility.hold_to_toggle);
}

#[test]
fn credits_name_engine_and_kit() {
    let credits: CreditsRoll =
        klotho_ir::from_ron(&fs::read_to_string(fixture_root().join("credits.ron")).unwrap())
            .unwrap();
    assert!(credits.entries.iter().any(|e| e.role.contains("engine")));
    assert!(
        credits
            .entries
            .iter()
            .any(|e| e.name.contains("kitbash") || e.role.contains("art"))
    );
    assert!(
        credits
            .entries
            .iter()
            .any(|e: &CreditEntry| e.role == "engine")
    );
}
