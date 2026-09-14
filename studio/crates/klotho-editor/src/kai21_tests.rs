//! KAI-21: Brocade production-scale orchestration acceptance.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use klotho_ai::{
    AuthorOp, BatchItem, ChangeId, EvidenceItem, FrozenBatch, ImpactGraph, OwnershipScheduler,
    RequestId, ReviewArrival, ReviewCapacity, RiskLevel, SampleAudit, SamplingPolicy, ScaleWork,
    merge_ops, rollup_evidence, simulate_capacity,
};
use klotho_author::AnchoredSeedFact;
use klotho_compile::{PlaceBundlePlan, ScaleLimits, WholeTitlePlan, check_scale, measure_scale};
use klotho_core::{AabbMm, Hash, IVec3, LocusKind, Sigil};
use klotho_ir::{
    AnchorId, IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent, from_ron, migrate_doc,
};
use klotho_prove::{Cas, hash_bytes};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrocadeSpec {
    places: usize,
    changes_per_place: usize,
    shared_assets: usize,
    maximum_unique_bytes: usize,
    maximum_package_bytes: usize,
    review_periods: u32,
    capacities: Vec<ReviewCapacity>,
    arrivals: Vec<ReviewArrival>,
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/brocade-scale")
}

fn spec() -> BrocadeSpec {
    let text = fs::read_to_string(fixture().join("scale.ron")).unwrap();
    from_ron(&text).unwrap()
}

fn anchor(index: usize) -> AnchorId {
    AnchorId::derive(b"brocade", &index.to_le_bytes())
}

fn name(value: &str) -> Name {
    Name::from(value)
}

#[test]
fn hundred_place_ten_thousand_change_schedule_owns_disjoint_anchors() {
    let spec = spec();
    assert_eq!(spec.places, 100);
    assert_eq!(spec.places * spec.changes_per_place, 10_000);

    let mut scheduler = OwnershipScheduler::default();
    for place in 0..spec.places {
        scheduler
            .add(ScaleWork {
                id: name(&format!("place-{place:03}")),
                dependencies: BTreeSet::new(),
                reads: BTreeSet::new(),
                writes: [anchor(place)].into(),
                semantic_changes: u32::try_from(spec.changes_per_place).unwrap(),
            })
            .unwrap();
    }
    let ready = scheduler.ready();
    assert_eq!(ready.len(), 100);
    for id in &ready {
        scheduler.start(id).unwrap();
    }
    for id in ready.iter().rev() {
        scheduler.complete(id).unwrap();
    }
    assert_eq!(scheduler.completed_len(), 100);
    assert_eq!(scheduler.completed_changes(), 10_000);

    let mut contended = OwnershipScheduler::default();
    for id in ["first", "second"] {
        contended
            .add(ScaleWork {
                id: name(id),
                dependencies: BTreeSet::new(),
                reads: BTreeSet::new(),
                writes: [anchor(999)].into(),
                semantic_changes: 1,
            })
            .unwrap();
    }
    contended.start(&name("first")).unwrap();
    assert!(!contended.ready().contains(&name("second")));
}

fn merge_world() -> (klotho_ai::AuthoringSnapshot, AnchorId, AnchorId, AnchorId) {
    let doc = IntentDoc {
        style: StyleIntent::default(),
        canon_diffs: Vec::new(),
        seed: vec![
            SeedFact::Locus {
                name: name("left"),
                kind: LocusKind::Relic,
            },
            SeedFact::Locus {
                name: name("right"),
                kind: LocusKind::Relic,
            },
        ],
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    };
    let snap = klotho_ai::AuthoringSnapshot::from_bundle(
        migrate_doc(name("brocade"), name("base"), doc).unwrap(),
    );
    let module = snap.modules[0].anchor;
    let left = snap.modules[0]
        .object_anchors
        .iter()
        .find(|row| row.name.as_str() == "left")
        .unwrap()
        .anchor;
    let right = snap.modules[0]
        .object_anchors
        .iter()
        .find(|row| row.name.as_str() == "right")
        .unwrap()
        .anchor;
    (snap, module, left, right)
}

fn qty(module: AnchorId, target: AnchorId, value: i32) -> AuthorOp {
    AuthorOp::AddFact {
        module,
        fact: AnchoredSeedFact::Qty {
            of: target,
            res: name("brocade_value"),
            value,
        },
    }
}

#[test]
fn semantic_merge_commutes_and_conflicts_never_last_writer_win() {
    let (base, module, left, right) = merge_world();
    let a = ChangeId::derive(b"worker-a");
    let b = ChangeId::derive(b"worker-b");
    let left_op = qty(module, left, 1);
    let right_op = qty(module, right, 2);
    let forward = merge_ops(
        &base,
        a,
        std::slice::from_ref(&left_op),
        b,
        std::slice::from_ref(&right_op),
    )
    .unwrap();
    let reverse = merge_ops(&base, b, &[right_op], a, &[left_op]).unwrap();
    assert_eq!(forward, reverse);
    assert!(
        merge_ops(
            &base,
            a,
            &[qty(module, left, 1)],
            b,
            &[qty(module, left, 2)]
        )
        .is_err()
    );
}

#[test]
fn one_module_change_avoids_unrelated_recook() {
    let spec = spec();
    let content: BTreeMap<_, _> = (0..spec.places)
        .map(|place| {
            (
                name(&format!("place-{place:03}")),
                hash_bytes(format!("v1-{place}").as_bytes()),
            )
        })
        .collect();
    let graph = ImpactGraph {
        content: content.clone(),
        dependents: BTreeMap::new(),
    };
    let mut next = content;
    next.insert(name("place-042"), hash_bytes(b"v2-42"));
    assert_eq!(graph.affected(&next), [name("place-042")].into());
}

#[test]
fn burst_and_owner_absence_stay_inside_checked_in_capacity() {
    let spec = spec();
    let report = simulate_capacity(spec.review_periods, &spec.capacities, &spec.arrivals).unwrap();
    assert!(
        spec.capacities
            .iter()
            .all(|row| report.peak_backlog[&row.owner] <= row.maximum_backlog)
    );
    assert!(report.peak_backlog[&name("art.owner")] >= 200);
}

#[test]
fn frozen_sample_and_ten_thousand_item_evidence_rollup_reproduce() {
    let spec = spec();
    let items: Vec<_> = (0..100)
        .map(|index| BatchItem {
            anchor: anchor(index),
            operation_hash: hash_bytes(format!("op-{index}").as_bytes()),
            evidence_hash: hash_bytes(format!("evidence-{index}").as_bytes()),
            change: ChangeId::derive(format!("change-{index}").as_bytes()),
            request: RequestId::derive(format!("request-{index}").as_bytes()),
        })
        .collect();
    let batch = FrozenBatch::freeze(
        items,
        SamplingPolicy {
            policy_version: 1,
            minimum: 5,
            rate_percent: 10,
            owner: name("art.owner"),
            expires_at: 100,
        },
    )
    .unwrap();
    let nonce = b"fresh-reviewer-nonce-kai21";
    let mut record = batch.select(name("art.owner"), nonce, 10).unwrap();
    record
        .reproduce(&batch, name("art.owner"), nonce, 10)
        .unwrap();
    record.decide(&vec![true; record.selected.len()]).unwrap();
    let audit = SampleAudit::seal(&batch, &record, nonce, 10).unwrap();
    audit.verify(&batch).unwrap();

    let evidence = (0..spec.places * spec.changes_per_place)
        .map(|index| EvidenceItem {
            anchor: anchor(index + 10_000),
            risk: match index % 4 {
                0 => RiskLevel::R0,
                1 => RiskLevel::R1,
                2 => RiskLevel::R2,
                _ => RiskLevel::R3,
            },
            evidence: hash_bytes(format!("sealed-{index}").as_bytes()),
        })
        .collect();
    let rollup = rollup_evidence(evidence).unwrap();
    assert_eq!(rollup.by_risk.values().sum::<u32>(), 10_000);
    assert_ne!(rollup.root, Hash::ZERO);
}

#[test]
fn shared_cas_assets_hold_unique_and_package_caps() {
    let spec = spec();
    let mut cas = Cas::new();
    let assets: Vec<_> = (0..spec.shared_assets)
        .map(|index| {
            cas.put(format!("brocade-shared-asset-{index}").as_bytes())
                .unwrap()
        })
        .collect();
    let plan = WholeTitlePlan {
        tables: Vec::new(),
        places: (0..spec.places)
            .map(|place| PlaceBundlePlan {
                place: Sigil::pack(LocusKind::Place, 0, place as u128 + 1).unwrap(),
                aabb: AabbMm::from_point(IVec3 {
                    x: i32::try_from(place).unwrap() * 1_000,
                    y: 0,
                    z: 0,
                }),
                assets: assets.clone(),
                access_group: u16::try_from(place / 10).unwrap(),
            })
            .collect(),
        skus: Vec::new(),
        runtime_files: BTreeMap::new(),
        stripped_paths: Vec::new(),
        hash: Hash::ZERO,
    };
    let report = measure_scale(&cas, &plan).unwrap();
    assert_eq!(report.places, 100);
    assert_eq!(report.references, 2_000);
    assert_eq!(report.unique_blobs, 20);
    assert_eq!(report.instancing_ratio_milli, 100_000);
    check_scale(
        &report,
        &ScaleLimits {
            places: spec.places,
            unique_blobs: spec.shared_assets,
            unique_bytes: spec.maximum_unique_bytes,
            package_bytes: spec.maximum_package_bytes,
        },
    )
    .unwrap();
}
