//! KAI-08 trusted review-policy gates.

use klotho_ir::{AnchorId, Name};
use klotho_prove::hash_bytes;

use crate::{
    ArtifactClass, BatchItem, ChangeId, FrozenBatch, OpKind, OwnerBudget, OwnerQueues, R0Rule,
    RequestId, RiskInput, RiskLevel, RiskPolicy, SampleDisposition, SamplingPolicy,
};

fn input(operation: OpKind) -> RiskInput {
    RiskInput {
        operation,
        operation_version: 1,
        policy_version: 7,
        artifact: ArtifactClass::None,
        classifier_evidence: true,
        semantic_delta: false,
        approved_art_delta: false,
        budget_delta: 0,
        proofs: [Name::from("byte_equal")].into_iter().collect(),
    }
}

#[test]
fn trusted_router_is_exact_and_unknown_routes_r3() {
    let policy = RiskPolicy {
        version: 7,
        r0: [R0Rule {
            operation: OpKind::Rename,
            operation_version: 1,
            required_proofs: [Name::from("byte_equal")].into_iter().collect(),
        }]
        .into_iter()
        .collect(),
    };
    assert_eq!(policy.route(&input(OpKind::Rename)), RiskLevel::R0);

    let mut wrong_version = input(OpKind::Rename);
    wrong_version.policy_version = 6;
    assert_eq!(policy.route(&wrong_version), RiskLevel::R3);

    let mut unknown = input(OpKind::Rename);
    unknown.artifact = ArtifactClass::Unknown;
    assert_eq!(policy.route(&unknown), RiskLevel::R3);

    let mut canon = input(OpKind::AddCanonDiff);
    canon.semantic_delta = true;
    assert_eq!(policy.route(&canon), RiskLevel::R3);
}

fn item(n: u8) -> BatchItem {
    BatchItem {
        anchor: AnchorId([n; 16]),
        operation_hash: hash_bytes(&[n, 1]),
        evidence_hash: hash_bytes(&[n, 2]),
        change: ChangeId::derive(&[n, 3]),
        request: RequestId::derive(&[n, 4]),
    }
}

#[test]
fn frozen_sampling_is_reproducible_and_one_failure_escalates_all() {
    let batch = FrozenBatch::freeze(
        vec![item(3), item(1), item(2)],
        SamplingPolicy {
            policy_version: 7,
            minimum: 2,
            rate_percent: 10,
            owner: Name::from("world_owner"),
            expires_at: 50,
        },
    )
    .unwrap();
    let nonce = b"reviewer-nonce-0001";
    let a = batch.select(Name::from("reviewer"), nonce, 10).unwrap();
    let b = batch.select(Name::from("reviewer"), nonce, 10).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.selected.len(), 2);

    let mut result = a;
    assert_eq!(
        result.decide(&[true, false]).unwrap(),
        SampleDisposition::Escalated
    );
    assert_eq!(result.disposition, SampleDisposition::Escalated);
    assert!(batch.select(Name::from("reviewer"), nonce, 51).is_err());
}

#[test]
fn owner_queue_cannot_exceed_declared_capacity() {
    let owner = Name::from("narrative_owner");
    let mut queues = OwnerQueues::default();
    queues.set_budget(OwnerBudget {
        owner: owner.clone(),
        max_items: 3,
        max_minutes: 60,
    });
    queues.reserve(&owner, 2, 40).unwrap();
    assert!(queues.reserve(&owner, 2, 10).is_err());
    assert!(queues.reserve(&owner, 1, 21).is_err());
    queues.release(&owner, 1, 20);
    queues.reserve(&owner, 1, 20).unwrap();
    assert_eq!(queues.usage(&owner), (2, 40));

    let no_budget = Name::from("absent_owner");
    assert!(queues.reserve(&no_budget, 1, 1).is_err());
}
