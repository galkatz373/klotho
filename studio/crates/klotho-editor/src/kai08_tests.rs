//! KAI-08 Distaff benchmark workflow gates.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use klotho_ai::{
    AgentRole, AuthorOp, BackendResponse, ChangeScope, DeterministicFakeBackend, KlothoAi,
    RequestBudget, RiskPolicy,
};
use klotho_author::{flatten_bundle, write_bundle};
use klotho_core::{Hash, LocusKind};
use klotho_eval::{CheckLayer, EvidenceBuilder, EvidenceContext, SemanticClaim};
use klotho_ir::{IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent, migrate_doc};
use klotho_prove::hash_bytes;

use crate::{
    AcceptanceEditor, Assumption, CaptureComparison, DistaffReview, EditorSession, RequestDraft,
    ReviewState, conservative_risk_inputs,
};

fn scratch(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("klotho-kai08-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn doc() -> IntentDoc {
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs: Vec::new(),
        seed: vec![SeedFact::Locus {
            name: Name::from("hero"),
            kind: LocusKind::Actor,
        }],
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

fn setup(tag: &str, operations: Vec<AuthorOp>) -> (KlothoAi, EditorSession, PathBuf) {
    let root = scratch(tag);
    let bundle = migrate_doc(Name::from("kai08"), Name::from("main"), doc()).unwrap();
    let base = root.join("base");
    write_bundle(&bundle, &base).unwrap();
    let session = EditorSession::new(flatten_bundle(&bundle).unwrap().doc);
    let mut ai = KlothoAi::new(&root.join("workspace"), &base.join("project.ron")).unwrap();
    ai.models.register(DeterministicFakeBackend::new(
        "fake",
        vec![Ok(BackendResponse {
            operations,
            summary: "semantic candidate ready".into(),
            output_tokens: 24,
            cost_micro_usd: 10,
        })],
    ));
    (ai, session, root)
}

fn draft(anchor: klotho_ir::AnchorId) -> RequestDraft {
    RequestDraft {
        text: "Rename the selected hero and show playable evidence.".into(),
        assumptions: vec![Assumption {
            text: "The hero identity must remain stable.".into(),
            affects_behavior: true,
            accepted: Some(true),
        }],
        acceptance: AcceptanceEditor {
            claims: vec![SemanticClaim {
                id: Name::from("identity"),
                text: "The selected hero retains semantic identity.".into(),
            }],
            scope: ChangeScope {
                modules: BTreeSet::new(),
                anchors: [anchor].into_iter().collect(),
            },
            ..AcceptanceEditor::default()
        },
        role: AgentRole::Gameplay,
        budget: RequestBudget::default(),
    }
}

fn evidence(change: Hash, project_hash: Hash) -> klotho_eval::EvidenceBundle {
    let mut builder = EvidenceBuilder::new(EvidenceContext {
        change,
        project_hash,
        toolchain_hash: hash_bytes(b"toolchain"),
        expanded_ir_hash: hash_bytes(b"expanded"),
        canon_hash: hash_bytes(b"canon"),
        cas_root: hash_bytes(b"cas"),
    });
    builder.record_check(
        CheckLayer::Journey,
        Name::from("rename_journey"),
        true,
        hash_bytes(b"pass"),
    );
    builder.seal(None).unwrap()
}

#[test]
fn designer_completes_change_without_source_view_and_unpinned_review_disappears() {
    let placeholder = klotho_ir::AnchorId::ZERO;
    let (mut ai, mut session, _) = setup("workflow", vec![]);
    let hero = ai
        .project
        .entries
        .iter()
        .find(|row| row.label == "hero")
        .unwrap()
        .anchor;
    ai.models = Default::default();
    ai.models.register(DeterministicFakeBackend::new(
        "fake",
        vec![Ok(BackendResponse {
            operations: vec![AuthorOp::Rename {
                target: hero,
                to: Name::from("astronomer"),
            }],
            summary: "identity-preserving rename".into(),
            output_tokens: 8,
            cost_micro_usd: 0,
        })],
    ));
    assert_ne!(hero, placeholder);
    let request = draft(hero).submit(&mut ai).unwrap();
    let progress = ai.drive(request, 1).unwrap();
    let change = progress.change.unwrap();
    let diff = ai.review(change).unwrap().diff;
    let sealed = evidence(hash_bytes(&change.0), diff.current_hash);
    let evidence_hash = ai.evaluation.register_trusted(sealed).unwrap();
    ai.attach_evidence(change, evidence_hash).unwrap();
    let policy = RiskPolicy::default();
    let inputs = conservative_risk_inputs(&diff, &policy);
    let mut review = DistaffReview::open(
        &ai,
        request,
        change,
        &policy,
        &inputs,
        vec![CaptureComparison {
            checkpoint: Name::from("observatory_entry"),
            before: hash_bytes(b"before"),
            after: hash_bytes(b"after"),
        }],
        0,
    )
    .unwrap();
    assert_eq!(review.state, ReviewState::ReadyToPin);
    let visible = format!("{:?}{:?}{:?}", review.plan, review.groups, review.captures);
    assert!(!visible.contains("RON"));
    assert!(!visible.contains("Rust"));

    review
        .reject_groups(&[Name::from("change_1")], "wording needs another pass")
        .unwrap();
    assert!(matches!(review.state, ReviewState::Rejected(_)));
    drop(review);
    assert!(
        session
            .doc()
            .seed
            .iter()
            .any(|fact| matches!(fact, SeedFact::Locus { name, .. } if name.as_str() == "hero"))
    );
    assert!(
        !session.doc().seed.iter().any(
            |fact| matches!(fact, SeedFact::Locus { name, .. } if name.as_str() == "astronomer")
        )
    );

    let mut review =
        DistaffReview::open(&ai, request, change, &policy, &inputs, Vec::new(), 0).unwrap();
    review
        .pin(&mut session, "approve semantic identity change")
        .unwrap();
    assert_eq!(review.state, ReviewState::Pinned);
    assert!(
        session.doc().seed.iter().any(
            |fact| matches!(fact, SeedFact::Locus { name, .. } if name.as_str() == "astronomer")
        )
    );
}

#[test]
fn unresolved_assumption_blocks_generation() {
    let (mut ai, _, _) = setup("assumption", Vec::new());
    let hero = ai
        .project
        .entries
        .iter()
        .find(|row| row.label == "hero")
        .unwrap()
        .anchor;
    let mut request = draft(hero);
    request.assumptions[0].accepted = None;
    assert!(request.submit(&mut ai).is_err());

    let mut rejected = draft(hero);
    rejected.assumptions[0].accepted = Some(false);
    assert!(rejected.submit(&mut ai).is_err());
}

#[test]
fn partial_approval_invalidates_and_requires_rerun_before_pin() {
    let root = scratch("partial");
    let bundle = migrate_doc(Name::from("kai08"), Name::from("main"), doc()).unwrap();
    let module = bundle.modules[0].anchor;
    let hero = bundle.modules[0].object_anchors[0].anchor;
    let new_anchor = klotho_ir::AnchorId::derive(b"kai08", b"guide");
    let base = root.join("base");
    write_bundle(&bundle, &base).unwrap();
    let mut session = EditorSession::new(flatten_bundle(&bundle).unwrap().doc);
    let mut ai = KlothoAi::new(&root.join("workspace"), &base.join("project.ron")).unwrap();
    ai.models.register(DeterministicFakeBackend::new(
        "fake",
        vec![Ok(BackendResponse {
            operations: vec![
                AuthorOp::Rename {
                    target: hero,
                    to: Name::from("astronomer"),
                },
                AuthorOp::AddLocus {
                    module,
                    anchor: new_anchor,
                    name: Name::from("guide"),
                    kind: LocusKind::Actor,
                },
            ],
            summary: "two semantic groups".into(),
            output_tokens: 8,
            cost_micro_usd: 0,
        })],
    ));
    let mut request_draft = draft(hero);
    request_draft.acceptance.scope.modules.insert(module);
    let request = request_draft.submit(&mut ai).unwrap();
    let change = ai.drive(request, 1).unwrap().change.unwrap();
    let diff = ai.review(change).unwrap().diff;
    let full = evidence(hash_bytes(&change.0), diff.current_hash);
    let full_hash = ai.evaluation.register_trusted(full).unwrap();
    ai.attach_evidence(change, full_hash).unwrap();
    let policy = RiskPolicy::default();
    let inputs = conservative_risk_inputs(&diff, &policy);
    let mut review =
        DistaffReview::open(&ai, request, change, &policy, &inputs, Vec::new(), 0).unwrap();
    let partial_hash = review.select_groups(&[Name::from("change_1")]).unwrap();
    assert_eq!(review.state, ReviewState::EvidenceRequired);
    assert!(review.evidence().is_empty());
    assert!(review.pin(&mut session, "too early").is_err());

    let rerun = evidence(partial_hash, partial_hash);
    let rerun_hash = ai.evaluation.register_trusted(rerun).unwrap();
    review.attach_rerun(&ai, rerun_hash).unwrap();
    review
        .pin(&mut session, "approve only the identity-preserving rename")
        .unwrap();
    assert!(
        session.doc().seed.iter().any(
            |fact| matches!(fact, SeedFact::Locus { name, .. } if name.as_str() == "astronomer")
        )
    );
    assert!(
        !session
            .doc()
            .seed
            .iter()
            .any(|fact| matches!(fact, SeedFact::Locus { name, .. } if name.as_str() == "guide"))
    );
}
