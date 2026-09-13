//! KAI-18 review-scaling gates: signed samples, adversarial freeze attacks.

use klotho_ir::{AnchorId, Name};
use klotho_prove::hash_bytes;

use crate::{
    ArtifactClass, BatchItem, ChangeId, FrozenBatch, OpKind, R0Rule, RequestId, RiskInput,
    RiskLevel, RiskPolicy, SampleAudit, SampleDisposition, SamplingPolicy, assigned_level,
    refuse_lower, refuse_replace, refuse_split,
};

fn item(n: u8) -> BatchItem {
    BatchItem {
        anchor: AnchorId([n; 16]),
        operation_hash: hash_bytes(&[n, 1]),
        evidence_hash: hash_bytes(&[n, 2]),
        change: ChangeId::derive(&[n, 3]),
        request: RequestId::derive(&[n, 4]),
    }
}

fn batch(items: Vec<BatchItem>) -> FrozenBatch {
    FrozenBatch::freeze(
        items,
        SamplingPolicy {
            policy_version: 7,
            minimum: 2,
            rate_percent: 10,
            owner: Name::from("world_owner"),
            expires_at: 50,
        },
    )
    .unwrap()
}

#[test]
fn signed_sample_reproduces_and_rejects_hand_edits() {
    let frozen = batch(vec![item(1), item(2), item(3)]);
    let nonce = b"reviewer-nonce-0001";
    let record = frozen.select(Name::from("reviewer"), nonce, 10).unwrap();
    let audit = SampleAudit::seal(&frozen, &record, nonce, 10).unwrap();
    audit.verify(&frozen).unwrap();

    let mut forged = record.clone();
    forged.selected = vec![0];
    assert!(
        forged
            .reproduce(&frozen, Name::from("reviewer"), nonce, 10)
            .is_err()
    );

    let mut mutated = frozen.clone();
    mutated.items[0].evidence_hash = hash_bytes(b"replaced");
    assert!(mutated.verify().is_err());
    assert!(refuse_replace(&frozen, &mutated.items).is_err());
    refuse_replace(&frozen, &frozen.items).unwrap();
}

#[test]
fn split_relabel_and_predictable_nonce_fail() {
    let left = batch(vec![item(1), item(2), item(3)]);
    let right = batch(vec![item(1), item(4), item(5)]);
    assert!(refuse_split(&left, &right, None).is_err());
    refuse_split(&left, &right, Some("trusted-policy:locale-shard")).unwrap();

    let disjoint = batch(vec![item(8), item(9), item(10)]);
    refuse_split(&left, &disjoint, None).unwrap();

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
    let input = RiskInput {
        operation: OpKind::AddCanonDiff,
        operation_version: 1,
        policy_version: 7,
        artifact: ArtifactClass::Critical,
        classifier_evidence: true,
        semantic_delta: true,
        approved_art_delta: false,
        budget_delta: 0,
        proofs: Default::default(),
    };
    assert_eq!(
        assigned_level(&policy, &input, Some(RiskLevel::R0)),
        RiskLevel::R3
    );
    assert!(refuse_lower(RiskLevel::R3, RiskLevel::R0).is_err());
    refuse_lower(RiskLevel::R3, RiskLevel::R3).unwrap();

    let frozen = batch(vec![item(1), item(2), item(3)]);
    let predictable = frozen.batch_root.as_bytes();
    let record = frozen
        .select(Name::from("reviewer"), predictable, 10)
        .unwrap();
    assert!(SampleAudit::seal(&frozen, &record, predictable, 10).is_err());
}

#[test]
fn one_failed_sample_still_escalates_the_frozen_batch() {
    let frozen = batch(vec![item(1), item(2), item(3)]);
    let nonce = b"reviewer-nonce-0001";
    let mut record = frozen.select(Name::from("reviewer"), nonce, 10).unwrap();
    let audit = SampleAudit::seal(&frozen, &record, nonce, 10).unwrap();
    assert_eq!(audit.disposition, SampleDisposition::Pending);
    assert_eq!(
        record.decide(&[true, false]).unwrap(),
        SampleDisposition::Escalated
    );
}
