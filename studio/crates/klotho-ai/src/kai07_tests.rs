//! KAI-07 hostile-output, routing, index, memory, evidence, and workflow gates.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use klotho_author::write_bundle;
use klotho_core::{Hash, LocusKind};
use klotho_eval::{
    AcceptanceContract as EvaluationContract, ChangeScope as EvaluationScope, EvidenceBuilder,
    EvidenceContext, SemanticClaim,
};
use klotho_ir::{IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent, migrate_doc, to_ron};
use klotho_prove::hash_bytes;

use crate::agent::{AgentRole, CreativeRequest, RequestBudget, RequestState};
use crate::context::{ContextBuilder, ContextRequest};
use crate::error::AiError;
use crate::evaluation::EvaluationBroker;
use crate::index::{
    DistanceMetric, EmbeddingIndex, EmbeddingKey, EmbeddingRow, SemanticProjectIndex,
};
use crate::memory::{ApprovedMemory, MemoryApproval};
use crate::model::{
    BackendId, BackendKind, BackendResponse, BackendSpec, DeterministicFakeBackend,
    ModelCapability, ModelRouter,
};
use crate::ops::{AuthorOp, ChangeScope, TxBudget};
use crate::policy::{Capability, ContextClass, DisclosurePolicy, ToolProfile};
use crate::service::KlothoAi;
use crate::tools::ToolRegistry;
use crate::workspace::AuthoringSnapshot;

fn scratch(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("klotho-kai07-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn bundle() -> klotho_ir::ProjectBundle {
    migrate_doc(
        Name::from("kai"),
        Name::from("main"),
        IntentDoc {
            style: StyleIntent::default(),
            canon_diffs: Vec::new(),
            seed: vec![SeedFact::Locus {
                name: Name::from("hero"),
                kind: LocusKind::Actor,
            }],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        },
    )
    .unwrap()
}

fn service(tag: &str) -> (KlothoAi, PathBuf) {
    let root = scratch(tag);
    let base = root.join("base");
    let workspace = root.join("workspace");
    write_bundle(&bundle(), &base).unwrap();
    (
        KlothoAi::new(&workspace, &base.join("project.ron")).unwrap(),
        root,
    )
}

fn request(anchor: klotho_ir::AnchorId, backend: &str) -> CreativeRequest {
    CreativeRequest {
        text: "Rename the selected hero for the benchmark evidence candidate.".into(),
        acceptance: EvaluationContract {
            allowed_scope: EvaluationScope {
                modules: Vec::new(),
                anchors: vec![anchor],
            },
            claims: vec![SemanticClaim {
                id: Name::from("identity"),
                text: "selected hero retains identity".into(),
            }],
            ..EvaluationContract::default()
        },
        scope: ChangeScope {
            modules: BTreeSet::new(),
            anchors: [anchor].into_iter().collect(),
        },
        transaction_budget: TxBudget { max_ops: 2 },
        budget: RequestBudget::default(),
        role: AgentRole::Gameplay,
        model_capability: ModelCapability::Reasoning,
        disclosure: DisclosurePolicy::LocalOnly,
        preferred_backend: Some(BackendId::from(backend)),
        dependencies: Vec::new(),
    }
}

fn hero(ai: &KlothoAi) -> klotho_ir::AnchorId {
    ai.project
        .entries
        .iter()
        .find(|entry| entry.label == "hero")
        .expect("hero index row")
        .anchor
}

#[test]
fn hostile_tool_payloads_have_no_escape_surface() {
    let corpus: Vec<String> = ron::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../benchmarks/kai/v1/faults/model-hostile.ron"
    )))
    .unwrap();
    for payload in corpus {
        assert!(ToolRegistry::decode(payload.as_bytes(), 4_096).is_err());
    }
    assert_eq!(Capability::ALL.len(), ToolRegistry::names().len());
    assert!(matches!(
        ToolRegistry::decode(&[b'x'; 32], 8),
        Err(AiError::BackendPayload)
    ));
}

#[test]
fn least_privilege_profiles_reject_mutation() {
    let profile = ToolProfile::standard(AgentRole::Test);
    assert_eq!(
        profile.require(Capability::ChangeApply),
        Err(AiError::CapabilityDenied(Capability::ChangeApply))
    );
    assert!(profile.require(Capability::EvidenceRead).is_ok());
}

#[test]
fn remote_route_requires_explicit_full_disclosure() {
    let remote = BackendSpec {
        id: BackendId::from("remote"),
        kind: BackendKind::Remote,
        model_hash: hash_bytes(b"remote-model"),
        parameters_hash: hash_bytes(b"remote-params"),
        capabilities: [ModelCapability::Reasoning].into_iter().collect(),
        credential_key: Some("provider".into()),
    };
    struct EmptyRemote(BackendSpec);
    impl crate::model::ModelBackend for EmptyRemote {
        fn spec(&self) -> &BackendSpec {
            &self.0
        }
        fn invoke(
            &mut self,
            _request: &crate::model::BackendRequest,
        ) -> Result<BackendResponse, AiError> {
            Ok(BackendResponse::default())
        }
    }
    let mut router = ModelRouter::default();
    router.register(EmptyRemote(remote));
    let (ai, _) = service("disclosure");
    let backend_request = crate::model::BackendRequest {
        request: crate::agent::RequestId::derive(b"r"),
        prompt: "request".into(),
        context: ContextBuilder::compile(
            &ai.project,
            &ai.catalog,
            &ai.memory,
            &ContextRequest {
                max_entries: 4,
                ..ContextRequest::default()
            },
        ),
        capability: ModelCapability::Reasoning,
        max_tokens: 8,
    };
    assert!(matches!(
        router.invoke(&backend_request, &DisclosurePolicy::LocalOnly, None),
        Err(AiError::NoBackend)
    ));
    let disclosure = DisclosurePolicy::RemoteAllow(
        [
            ContextClass::Schema,
            ContextClass::ProjectStructure,
            ContextClass::ApprovedMemory,
            ContextClass::RequestText,
        ]
        .into_iter()
        .collect(),
    );
    assert!(router.invoke(&backend_request, &disclosure, None).is_ok());
}

#[test]
fn secrets_never_enter_context_or_serialized_backend_request() {
    let (mut ai, _) = service("secret");
    let secret = b"KLOTHO_TEST_PROVIDER_SECRET".to_vec();
    ai.secrets.insert("provider", secret.clone());
    let context = ContextBuilder::compile(
        &ai.project,
        &ai.catalog,
        &ai.memory,
        &ContextRequest {
            max_entries: 32,
            include_memory: true,
            ..ContextRequest::default()
        },
    );
    let encoded = to_ron(&context).unwrap();
    assert!(
        !encoded
            .as_bytes()
            .windows(secret.len())
            .any(|w| w == secret)
    );
    assert_eq!(ai.secrets.len(), 1);
}

#[test]
fn embedding_keys_are_exact_and_refresh_invalidates_cache() {
    let snap = AuthoringSnapshot::from_bundle(bundle());
    let mut index = SemanticProjectIndex::build(&snap, 1).unwrap();
    let row = index.entries[0].clone();
    let key = EmbeddingKey {
        project_hash: index.project_hash,
        schema_version: 1,
        chunker_hash: hash_bytes(b"chunker"),
        backend_hash: hash_bytes(b"backend"),
        model_hash: hash_bytes(b"model"),
        distance: DistanceMetric::Cosine,
    };
    index
        .install_embeddings(EmbeddingIndex {
            key: key.clone(),
            rows: vec![EmbeddingRow {
                anchor: row.anchor,
                source_hash: row.source_hash,
                vector: vec![1, -2, 3],
            }],
        })
        .unwrap();
    let mut wrong = key.clone();
    wrong.model_hash = Hash::ZERO;
    assert!(matches!(
        index.embedding_rows(&wrong),
        Err(AiError::EmbeddingKeyMismatch)
    ));
    let mut changed = snap;
    changed.modules[0].id = Name::from("renamed_module");
    index.refresh(&changed).unwrap();
    assert!(index.embeddings.is_none());
}

#[test]
fn forged_evidence_cannot_enter_broker() {
    let ctx = EvidenceContext {
        change: hash_bytes(b"change"),
        project_hash: hash_bytes(b"project"),
        toolchain_hash: hash_bytes(b"toolchain"),
        expanded_ir_hash: hash_bytes(b"ir"),
        canon_hash: hash_bytes(b"canon"),
        cas_root: hash_bytes(b"cas"),
    };
    let valid = EvidenceBuilder::new(ctx).seal(None).unwrap();
    let mut broker = EvaluationBroker::default();
    let hash = broker.register_trusted(valid.clone()).unwrap();
    assert_eq!(broker.get(hash).unwrap().bundle, valid);
    let mut forged = valid;
    forged.project_hash = Hash::ZERO;
    assert!(matches!(
        broker.register_trusted(forged),
        Err(AiError::Evidence(_))
    ));
}

#[test]
fn locked_style_request_completes_without_shell_or_live_write() {
    let (mut ai, root) = service("workflow");
    let before = fs::read(root.join("base/project.ron")).unwrap();
    let anchor = hero(&ai);
    ai.models.register(DeterministicFakeBackend::new(
        "fake",
        vec![Ok(BackendResponse {
            operations: vec![AuthorOp::Rename {
                target: anchor,
                to: Name::from("player_hero"),
            }],
            summary: "renamed selected locus by immutable identity".into(),
            output_tokens: 12,
            cost_micro_usd: 0,
        })],
    ));
    let id = ai.request(request(anchor, "fake")).unwrap();
    assert_eq!(ai.poll(id).unwrap().state, RequestState::Queued);
    let progress = ai.drive(id, 10).unwrap();
    assert_eq!(progress.state, RequestState::Candidate);
    let change = progress.change.unwrap();
    assert_eq!(ai.review(change).unwrap().diff.ops.len(), 1);
    assert_eq!(fs::read(root.join("base/project.ron")).unwrap(), before);
    assert_eq!(ai.models.records().len(), 1);
}

#[test]
fn timeout_can_resume_on_a_different_backend() {
    let (mut ai, _) = service("resume");
    let anchor = hero(&ai);
    ai.models.register(DeterministicFakeBackend::new(
        "timeout",
        vec![Err(AiError::Timeout)],
    ));
    ai.models.register(DeterministicFakeBackend::new(
        "fallback",
        vec![Ok(BackendResponse {
            operations: vec![AuthorOp::Rename {
                target: anchor,
                to: Name::from("resumed_hero"),
            }],
            summary: "resumed".into(),
            output_tokens: 1,
            cost_micro_usd: 0,
        })],
    ));
    let id = ai.request(request(anchor, "timeout")).unwrap();
    assert_eq!(ai.drive(id, 0).unwrap().state, RequestState::TimedOut);
    ai.resume(id, Some(BackendId::from("fallback"))).unwrap();
    assert_eq!(ai.drive(id, 1).unwrap().state, RequestState::Candidate);
    assert_eq!(ai.models.records().len(), 2);
}

#[test]
fn scheduler_refuses_overlapping_anchor_ownership() {
    let (mut ai, _) = service("ownership");
    let anchor = hero(&ai);
    let first = ai.request(request(anchor, "fake")).unwrap();
    assert!(matches!(
        ai.request(request(anchor, "fake")),
        Err(AiError::Ownership { by, .. }) if by == first
    ));
}

#[test]
fn approved_memory_persists_and_unapproved_rows_fail_closed() {
    let (mut ai, root) = service("memory");
    let anchor = hero(&ai);
    assert!(matches!(
        ai.memory.approve(ApprovedMemory {
            anchor,
            summary: String::new(),
            source_hashes: Vec::new(),
            approval: MemoryApproval {
                by: Name::from("design_owner"),
                record_hash: hash_bytes(b"approval"),
            },
        }),
        Err(AiError::UnapprovedMemory)
    ));
    ai.memory
        .approve(ApprovedMemory {
            anchor,
            summary: "The hero silhouette stays readable against stone.".into(),
            source_hashes: vec![hash_bytes(b"approved-reference")],
            approval: MemoryApproval {
                by: Name::from("design_owner"),
                record_hash: hash_bytes(b"approval"),
            },
        })
        .unwrap();
    drop(ai);
    let reopened = KlothoAi::new(&root.join("workspace"), &root.join("base/project.ron")).unwrap();
    assert_eq!(reopened.memory.rows().len(), 1);
}

#[test]
fn fake_backend_is_also_subject_to_protocol_size_caps() {
    let (mut ai, _) = service("response-cap");
    let anchor = hero(&ai);
    ai.policy.max_response_bytes = 64;
    ai.models.register(DeterministicFakeBackend::new(
        "fake",
        vec![Ok(BackendResponse {
            operations: Vec::new(),
            summary: "x".repeat(1_024),
            output_tokens: 1,
            cost_micro_usd: 0,
        })],
    ));
    let id = ai.request(request(anchor, "fake")).unwrap();
    assert_eq!(ai.drive(id, 0), Err(AiError::BackendPayload));
    assert!(matches!(
        ai.poll(id).unwrap().state,
        RequestState::Failed(_)
    ));
}
