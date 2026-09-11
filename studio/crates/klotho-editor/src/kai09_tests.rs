//! KAI-09 Spindle: one data-only request through repair, evidence, review, and Pin.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use klotho_ai::{
    AgentRole, AuthorOp, BackendId, BackendKind, BackendResponse, BackendSpec, BackendStatus,
    KlothoAi, ModelBackend, ModelCapability, RequestBudget, RiskPolicy,
};
use klotho_author::{flatten_bundle, load_file};
use klotho_core::Hash;
use klotho_eval::{
    BudgetTarget, CheckLayer, EvidenceBuilder, EvidenceContext, InvariantRef, JourneyHost,
    JourneyId, JourneySpec, QualityTarget, ScriptHost, SemanticClaim, run_journey,
    steps_are_public_input,
};
use klotho_ir::{
    Diagnostic, DiagnosticCode, Name, ParameterValue, PatternArg, PatternInstance, Rel, SeedFact,
    migrate_doc, to_ron,
};
use klotho_prove::hash_bytes;
use serde::Deserialize;

use crate::{
    AcceptanceEditor, Assumption, CaptureComparison, DistaffReview, EditorSession, RequestDraft,
    ReviewState, conservative_risk_inputs,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpindleRequest {
    outcome: String,
    request: String,
    assumption: String,
    pattern: String,
    version: u32,
    instance: String,
    player: String,
    place: String,
    passage: String,
    intended_key: String,
    seeded_wrong_key: String,
    journeys: Vec<String>,
    approved_bindings: Vec<(String, String)>,
}

struct InspectingBackend {
    spec: BackendSpec,
    responses: VecDeque<BackendResponse>,
    seen: Arc<Mutex<Vec<Vec<Diagnostic>>>>,
}

impl InspectingBackend {
    fn new(responses: Vec<BackendResponse>, seen: Arc<Mutex<Vec<Vec<Diagnostic>>>>) -> Self {
        Self {
            spec: BackendSpec {
                id: BackendId::from("spindle-fixture"),
                kind: BackendKind::Fake,
                model_hash: hash_bytes(b"spindle-fixture-model-v1"),
                parameters_hash: hash_bytes(b"spindle-fixture-params-v1"),
                capabilities: [ModelCapability::Reasoning].into_iter().collect(),
                credential_key: None,
            },
            responses: responses.into(),
            seen,
        }
    }
}

impl ModelBackend for InspectingBackend {
    fn spec(&self) -> &BackendSpec {
        &self.spec
    }

    fn invoke(
        &mut self,
        request: &klotho_ai::BackendRequest,
    ) -> Result<BackendResponse, klotho_ai::AiError> {
        self.seen.lock().unwrap().push(request.diagnostics.clone());
        self.responses
            .pop_front()
            .ok_or_else(|| klotho_ai::AiError::BackendTransport("fixture exhausted".into()))
    }
}

fn scratch() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("klotho-spindle-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/spindle-slice")
}

fn read_request(root: &Path) -> SpindleRequest {
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

#[test]
fn spindle_is_created_repaired_evidenced_and_pinned_without_title_rust() {
    let fixture = fixture_root();
    let spec = read_request(&fixture);
    let journeys = read_journeys(&fixture, &spec.journeys);
    assert_eq!(spec.outcome, "KAI-SPINDLE-001");
    assert!(no_rust_below(&fixture));
    assert!(
        journeys
            .iter()
            .all(|journey| steps_are_public_input(&journey.steps))
    );

    let base_path = fixture.join("base.kdown");
    let base_doc = load_file(&base_path).unwrap();
    let bundle = migrate_doc(Name::from("session"), Name::from("main"), base_doc.clone()).unwrap();
    let module = bundle.modules[0].anchor;
    let instance = module.child(format!("pattern:{}", spec.instance).as_bytes());
    let bad = AuthorOp::Instantiate {
        instance: PatternInstance {
            anchor: instance,
            module,
            instance: Name::from(spec.instance.as_str()),
            pattern: Name::from(spec.pattern.as_str()),
            version: spec.version,
            args: vec![
                parameter("passage", &spec.passage),
                parameter("key", &spec.seeded_wrong_key),
            ],
        },
    };
    let repair = AuthorOp::SetArgument {
        instance,
        key: Name::from("key"),
        value: parameter("key", &spec.intended_key),
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let backend = InspectingBackend::new(
        vec![
            BackendResponse {
                operations: vec![bad],
                summary: "observatory lock candidate".into(),
                output_tokens: 24,
                cost_micro_usd: 0,
            },
            BackendResponse {
                operations: vec![repair],
                summary: "key relation repaired from journey witness".into(),
                output_tokens: 8,
                cost_micro_usd: 0,
            },
        ],
        seen.clone(),
    );
    let temp = scratch();
    let mut ai = KlothoAi::new(&temp.join("workspace"), &base_path).unwrap();
    ai.models.register(backend);

    let draft = RequestDraft {
        text: spec.request.clone(),
        assumptions: vec![Assumption {
            text: spec.assumption,
            affects_behavior: true,
            accepted: Some(true),
        }],
        acceptance: AcceptanceEditor {
            claims: vec![SemanticClaim {
                id: Name::from("brass_key_opens_observatory"),
                text: "Only the approved brass key satisfies the observatory passage.".into(),
            }],
            journeys: journeys.iter().map(|journey| journey.id.clone()).collect(),
            invariants: vec![InvariantRef {
                id: Name::from("K21.atomic_admission"),
            }],
            quality: vec![QualityTarget {
                id: Name::from("approved_observatory_kit"),
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
    let (_, broken_snapshot) = ai.candidate_snapshots(change).unwrap();
    let broken_doc = flatten_bundle(&broken_snapshot.bundle()).unwrap().doc;

    let unlock = journeys
        .iter()
        .find(|journey| journey.id == JourneyId::from("spindle-unlock"))
        .unwrap();
    let mut broken_host = ScriptHost::from_intent(
        &broken_doc,
        &Name::from(spec.player.as_str()),
        &Name::from(spec.passage.as_str()),
        &Name::from(spec.intended_key.as_str()),
        &Name::from(spec.place.as_str()),
    )
    .unwrap();
    let failure = run_journey(&mut broken_host, unlock, hash_bytes(&change.0)).unwrap_err();
    let diagnostic = failure.to_diagnostic();
    assert_eq!(diagnostic.code.0, DiagnosticCode::JOURNEY);
    assert!(!diagnostic.legal_repairs.is_empty());

    let repaired = ai.repair(change, vec![diagnostic.clone()]).unwrap();
    assert!(repaired.diagnostics.is_empty());
    assert_eq!(repaired.progress.usage.repairs, 1);
    assert!(repaired.progress.usage.repairs <= 3);
    let observed = seen.lock().unwrap();
    assert!(observed[0].is_empty());
    assert_eq!(observed[1], vec![diagnostic]);
    drop(observed);
    assert_eq!(ai.models.records().len(), 2);
    assert!(
        ai.models
            .records()
            .iter()
            .all(|record| record.status == BackendStatus::Completed)
    );

    let review_package = ai.review(change).unwrap();
    let (_, repaired_snapshot) = ai.candidate_snapshots(change).unwrap();
    let repaired_doc = flatten_bundle(&repaired_snapshot.bundle()).unwrap().doc;
    let cooked = klotho_author::cook_validated(&repaired_doc).unwrap();
    let actual_bindings: BTreeSet<_> = cooked
        .bindings
        .iter()
        .map(|binding| (binding.locus.0.clone(), binding.tag.0.clone()))
        .collect();
    let expected_bindings: BTreeSet<_> = spec.approved_bindings.into_iter().collect();
    assert!(expected_bindings.is_subset(&actual_bindings));

    let expanded = to_ron(&repaired_doc).unwrap();
    let mut evidence = EvidenceBuilder::new(EvidenceContext {
        change: hash_bytes(&change.0),
        project_hash: review_package.diff.current_hash,
        toolchain_hash: hash_bytes(b"spindle-toolchain-v1"),
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
        let mut host = ScriptHost::from_intent(
            &repaired_doc,
            &Name::from(spec.player.as_str()),
            &Name::from(spec.passage.as_str()),
            &Name::from(spec.intended_key.as_str()),
            &Name::from(spec.place.as_str()),
        )
        .unwrap();
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
    assert_eq!(review.plan.len(), 2);
    let mut session = EditorSession::new(base_doc);
    review
        .pin(
            &mut session,
            "approve Spindle observatory mechanic and evidence",
        )
        .unwrap();
    assert_eq!(review.state, ReviewState::Pinned);
    assert!(session.doc().seed.iter().any(|fact| matches!(
        fact,
        SeedFact::Rel { a, rel: Rel::KeyedBy, b }
            if a.as_str() == "oak_door" && b.as_str() == "iron_key"
    )));
    assert!(!session.doc().seed.iter().any(|fact| matches!(
        fact,
        SeedFact::Rel { a, rel: Rel::KeyedBy, b }
            if a.as_str() == "oak_door" && b.as_str() == "lockpick_tool"
    )));
}
