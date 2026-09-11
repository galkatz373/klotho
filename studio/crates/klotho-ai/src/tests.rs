//! Isolation, commute, conflict matrix, merge, rebase, and lease gates.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use klotho_author::{AnchoredSeedFact, write_bundle};
use klotho_core::{Hash, LocusKind, Mm, PoseMm, YawMd};
use klotho_ir::{
    AnchorId, CanonDiff, IntentDoc, IntentModule, Law, LawBody, Name, Pred, ProjectBundle,
    ProvenanceId, Rel, SeedFact, Slot, StyleIntent, migrate_doc,
};
use klotho_prove::hash_bytes;

use crate::conflict::{ConflictReason, MergeClass, classify_pair, matrix_rule};
use crate::error::AiError;
use crate::exec::apply_ops;
use crate::ids::{AssetRequestId, ChangeId, ReferenceId, derive_op_anchor};
use crate::merge::merge_ops;
use crate::ops::{
    AuthorOp, ChangeScope, JourneySpec, OpKind, PatternArg, PatternInstance, TxBudget,
};
use crate::store::TransactionStore;
use crate::workspace::AuthoringSnapshot;

fn name(s: &str) -> Name {
    Name::from(s)
}

fn empty_doc() -> IntentDoc {
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs: Vec::new(),
        seed: Vec::new(),
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

fn origin() -> PoseMm {
    PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0))
}

fn scratch(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let mut bytes = tag.as_bytes().to_vec();
    bytes.extend_from_slice(&nanos.to_le_bytes());
    let h = hash_bytes(&bytes);
    let dir = std::env::temp_dir().join(format!("klotho-kai03-{tag}-{h}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn doc_loci(names: &[&str]) -> IntentDoc {
    IntentDoc {
        seed: names
            .iter()
            .map(|n| SeedFact::Locus {
                name: name(n),
                kind: if *n == "hero" {
                    LocusKind::Actor
                } else {
                    LocusKind::Relic
                },
            })
            .collect(),
        ..empty_doc()
    }
}

fn hearth_bundle() -> ProjectBundle {
    migrate_doc(
        name("hearth"),
        name("main"),
        doc_loci(&["oak_door", "hero", "barrel"]),
    )
    .unwrap()
}

struct World {
    snap: AuthoringSnapshot,
    module: AnchorId,
    oak: AnchorId,
    hero: AnchorId,
    barrel: AnchorId,
}

fn world() -> World {
    let bundle = hearth_bundle();
    let snap = AuthoringSnapshot::from_bundle(bundle);
    let module = snap.modules[0].anchor;
    let mut by_name = BTreeMap::new();
    for object in &snap.modules[0].object_anchors {
        by_name.insert(object.name.as_str().to_owned(), object.anchor);
    }
    World {
        module,
        oak: by_name["oak_door"],
        hero: by_name["hero"],
        barrel: by_name["barrel"],
        snap,
    }
}

fn law(id: &str) -> CanonDiff {
    CanonDiff::AddLaw(Law {
        id: name(id),
        when: Pred::SelfIs(Slot::This),
        body: LawBody::Pred {
            must: Pred::SelfIs(Slot::This),
            ought: None,
        },
    })
}

fn change(tag: &str) -> ChangeId {
    ChangeId::derive(tag.as_bytes())
}

fn qty(module: AnchorId, of: AnchorId, value: i32) -> AuthorOp {
    AuthorOp::AddFact {
        module,
        fact: AnchoredSeedFact::Qty {
            of,
            res: name("mass_g"),
            value,
        },
    }
}

fn pose(module: AnchorId, of: AnchorId) -> AuthorOp {
    AuthorOp::AddFact {
        module,
        fact: AnchoredSeedFact::Pose { of, pose: origin() },
    }
}

fn rel(module: AnchorId, a: AnchorId, b: AnchorId) -> AuthorOp {
    AuthorOp::AddFact {
        module,
        fact: AnchoredSeedFact::Rel {
            a,
            rel: Rel::KeyedBy,
            b,
        },
    }
}

fn add_locus(module: AnchorId, change: ChangeId, token: &str, n: &str) -> AuthorOp {
    AuthorOp::AddLocus {
        module,
        anchor: derive_op_anchor(&name("hearth"), change, token.as_bytes()),
        name: name(n),
        kind: LocusKind::Relic,
    }
}

fn extra_module(token: &str) -> IntentModule {
    let bundle = migrate_doc(name("hearth"), name(token), doc_loci(&[token])).unwrap();
    let mut module = bundle.modules.into_iter().next().unwrap();
    module.anchor = AnchorId::derive(b"hearth", format!("module:{token}").as_bytes());
    module
}

fn sample_op(kind: OpKind, slot: u8, w: &World) -> AuthorOp {
    let c = change(&format!("slot-{slot}"));
    match kind {
        OpKind::AddModule => AuthorOp::AddModule {
            module: extra_module(&format!("mod{slot}")),
        },
        OpKind::Instantiate => AuthorOp::Instantiate {
            instance: PatternInstance {
                anchor: derive_op_anchor(&name("hearth"), c, b"inst"),
                module: w.module,
                instance: name(&format!("inst{slot}")),
                pattern: name("traversal.door_key"),
                version: 1,
                args: Vec::new(),
            },
        },
        OpKind::SetArgument => AuthorOp::SetArgument {
            instance: if slot == 0 { w.oak } else { w.barrel },
            key: name("scale"),
            value: PatternArg {
                key: name("scale"),
                value: klotho_ir::ParameterValue::I32(i32::from(slot)),
            },
        },
        OpKind::AddLocus => add_locus(w.module, c, &format!("l{slot}"), &format!("prop{slot}")),
        OpKind::AddFact => {
            if slot == 0 {
                qty(w.module, w.oak, 1)
            } else {
                qty(w.module, w.barrel, 2)
            }
        }
        OpKind::AddCanonDiff => AuthorOp::AddCanonDiff {
            module: w.module,
            diff: law(&format!("law{slot}")),
        },
        OpKind::BindAsset => AuthorOp::BindAsset {
            locus: if slot == 0 { w.oak } else { w.barrel },
            request: AssetRequestId::derive(&[slot]),
        },
        OpKind::AddJourney => AuthorOp::AddJourney {
            journey: JourneySpec {
                id: name(&format!("j{slot}")),
            },
        },
        OpKind::AddReference => AuthorOp::AddReference {
            target: if slot == 0 { w.oak } else { w.barrel },
            reference: ReferenceId::derive(&[slot]),
        },
        OpKind::Remove => AuthorOp::Remove {
            target: if slot == 0 { w.oak } else { w.barrel },
            reason: "gone".into(),
        },
        OpKind::Rename => AuthorOp::Rename {
            target: if slot == 0 { w.oak } else { w.barrel },
            to: name(&format!("renamed{slot}")),
        },
    }
}

fn dir_bytes(root: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    walk(root, root, &mut out);
    out
}

#[test]
fn cancel_leaves_base_byte_identical() {
    let base_dir = scratch("base-cancel");
    let ws = scratch("ws-cancel");
    let bundle = hearth_bundle();
    write_bundle(&bundle, &base_dir).unwrap();
    let before = dir_bytes(&base_dir);
    let mut store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
    let id = store
        .create(ChangeScope::unrestricted(), TxBudget::default())
        .unwrap();
    let w = world();
    store
        .apply(
            id,
            vec![
                qty(w.module, w.oak, 9),
                add_locus(w.module, change("n"), "stool", "stool"),
            ],
        )
        .unwrap();
    store.cancel(id).unwrap();
    assert_eq!(dir_bytes(&base_dir), before);
    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn drop_without_cancel_does_not_mutate_base() {
    let base_dir = scratch("base-drop");
    let ws = scratch("ws-drop");
    write_bundle(&hearth_bundle(), &base_dir).unwrap();
    let before = dir_bytes(&base_dir);
    {
        let mut store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
        let id = store
            .create(ChangeScope::unrestricted(), TxBudget::default())
            .unwrap();
        let w = world();
        store.apply(id, vec![qty(w.module, w.oak, 4)]).unwrap();
    }
    assert_eq!(dir_bytes(&base_dir), before);
    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn resumable_after_reopen() {
    let base_dir = scratch("base-resume");
    let ws = scratch("ws-resume");
    write_bundle(&hearth_bundle(), &base_dir).unwrap();
    let id = {
        let mut store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
        let id = store
            .create(ChangeScope::unrestricted(), TxBudget::default())
            .unwrap();
        let w = world();
        store.apply(id, vec![qty(w.module, w.oak, 7)]).unwrap();
        id
    };
    let store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
    let snap = store.snapshot(id).unwrap();
    assert_eq!(snap.qty(world().oak, &name("mass_g")), Some(7));
    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn thousand_disjoint_writes_commute() {
    let mut seed = Vec::with_capacity(1000);
    for i in 0..1000 {
        seed.push(SeedFact::Locus {
            name: name(&format!("l{i:04}")),
            kind: LocusKind::Relic,
        });
    }
    let bundle = migrate_doc(
        name("hearth"),
        name("main"),
        IntentDoc {
            seed,
            ..empty_doc()
        },
    )
    .unwrap();
    let snap = AuthoringSnapshot::from_bundle(bundle);
    let module = snap.modules[0].anchor;
    let mut anchors: Vec<AnchorId> = snap.modules[0]
        .object_anchors
        .iter()
        .map(|o| o.anchor)
        .collect();
    anchors.sort();
    assert_eq!(anchors.len(), 1000);
    let ops: Vec<AuthorOp> = anchors
        .iter()
        .enumerate()
        .map(|(i, id)| qty(module, *id, i as i32))
        .collect();
    let mut a = snap.clone();
    apply_ops(&mut a, change("a"), &ops).unwrap();
    let mut reversed = ops.clone();
    reversed.reverse();
    let mut b = snap.clone();
    apply_ops(&mut b, change("b"), &reversed).unwrap();
    let mut shuffled = ops;
    shuffled.sort_by_key(|op| match op {
        AuthorOp::AddFact {
            fact: AnchoredSeedFact::Qty { of, .. },
            ..
        } => of.0[0] ^ of.0[15],
        _ => 0,
    });
    let mut c = snap;
    apply_ops(&mut c, change("c"), &shuffled).unwrap();
    assert_eq!(a.content_hash().unwrap(), b.content_hash().unwrap());
    assert_eq!(a.content_hash().unwrap(), c.content_hash().unwrap());
    assert_eq!(
        a.project.flatten(&a.modules).unwrap().doc,
        b.project.flatten(&b.modules).unwrap().doc
    );
}

#[test]
fn conflict_matrix_every_pair() {
    let w = world();
    let a_id = change("left");
    let b_id = change("right");
    for a in OpKind::ALL {
        for b in OpKind::ALL {
            let oa = sample_op(a, 0, &w);
            let ob = sample_op(b, 1, &w);
            let class = classify_pair(&w.snap, a_id, &oa, b_id, &ob);
            let rule = matrix_rule(a, b);
            match rule {
                crate::conflict::MatrixRule::CanonNoBulk => {
                    assert!(
                        matches!(class, MergeClass::Conflict(ref w) if w.reason == ConflictReason::CanonConcurrent),
                        "{a} vs {b}: {class:?}"
                    );
                    let same = sample_op(a, 0, &w);
                    let class = classify_pair(&w.snap, a_id, &same, b_id, &same);
                    assert!(matches!(class, MergeClass::Coalesce), "{a} identical");
                }
                crate::conflict::MatrixRule::PatternUnlessIdentical => {
                    assert!(
                        matches!(class, MergeClass::Conflict(ref w) if w.reason == ConflictReason::PatternConcurrent),
                        "{a} vs {b}: {class:?}"
                    );
                    let same = sample_op(a, 0, &w);
                    let class = classify_pair(&w.snap, a_id, &same, b_id, &same);
                    assert!(matches!(class, MergeClass::Coalesce), "{a} identical");
                }
                crate::conflict::MatrixRule::AssetCoexist => {
                    assert!(
                        matches!(class, MergeClass::Coexist),
                        "{a} vs {b}: {class:?}"
                    );
                }
                crate::conflict::MatrixRule::RemoveVsDependent => {
                    assert!(
                        matches!(
                            class,
                            MergeClass::Commute | MergeClass::Coalesce | MergeClass::Conflict(_)
                        ),
                        "{a} vs {b}: {class:?}"
                    );
                    let remove = sample_op(OpKind::Remove, 0, &w);
                    let edit = qty(w.module, w.oak, 3);
                    let neg = classify_pair(&w.snap, a_id, &remove, b_id, &edit);
                    assert!(
                        matches!(neg, MergeClass::Conflict(ref w) if w.reason == ConflictReason::RemoveDependent),
                        "remove vs oak qty: {neg:?}"
                    );
                    assert!(
                        matches!(neg, MergeClass::Conflict(ref w) if w.dependent.is_some()),
                        "witness must name a dependent"
                    );
                }
                crate::conflict::MatrixRule::RenameVsRename => {
                    assert!(
                        matches!(class, MergeClass::Commute),
                        "{a} vs {b}: {class:?}"
                    );
                    let r1 = AuthorOp::Rename {
                        target: w.oak,
                        to: name("brass_door"),
                    };
                    let r2 = AuthorOp::Rename {
                        target: w.oak,
                        to: name("iron_door"),
                    };
                    let neg = classify_pair(&w.snap, a_id, &r1, b_id, &r2);
                    assert!(
                        matches!(neg, MergeClass::Conflict(ref w) if w.reason == ConflictReason::RenameClash)
                    );
                    let pos = classify_pair(&w.snap, a_id, &r1, b_id, &r1);
                    assert!(matches!(pos, MergeClass::Coalesce));
                }
                crate::conflict::MatrixRule::RenameVsEdit => {
                    assert!(
                        matches!(class, MergeClass::Merge | MergeClass::Commute),
                        "{a} vs {b}: {class:?}"
                    );
                }
                crate::conflict::MatrixRule::MergeByChildId
                | crate::conflict::MatrixRule::CommuteIfDisjoint
                | crate::conflict::MatrixRule::CoalesceIfIdentical => {
                    assert!(
                        matches!(
                            class,
                            MergeClass::Commute | MergeClass::Coalesce | MergeClass::Merge
                        ),
                        "{a} vs {b} disjoint: {class:?}"
                    );
                }
            }
            if let Some((x, y)) = conflict_sample(a, b, &w) {
                let neg = classify_pair(&w.snap, a_id, &x, b_id, &y);
                assert!(
                    matches!(neg, MergeClass::Conflict(_)),
                    "{a} vs {b} overlapping: {neg:?}"
                );
            }
        }
    }
}

fn oak_write(kind: OpKind, w: &World) -> Option<AuthorOp> {
    match kind {
        OpKind::AddFact => Some(qty(w.module, w.oak, 3)),
        OpKind::BindAsset => Some(AuthorOp::BindAsset {
            locus: w.oak,
            request: AssetRequestId::derive(b"oak"),
        }),
        OpKind::AddReference => Some(AuthorOp::AddReference {
            target: w.oak,
            reference: ReferenceId::derive(b"oak"),
        }),
        OpKind::Rename => Some(AuthorOp::Rename {
            target: w.oak,
            to: name("brass_door"),
        }),
        OpKind::Remove => Some(AuthorOp::Remove {
            target: w.oak,
            reason: "also".into(),
        }),
        OpKind::SetArgument => Some(sample_op(OpKind::SetArgument, 0, w)),
        _ => None,
    }
}

fn conflict_sample(a: OpKind, b: OpKind, w: &World) -> Option<(AuthorOp, AuthorOp)> {
    use crate::conflict::MatrixRule;
    match matrix_rule(a, b) {
        MatrixRule::AssetCoexist | MatrixRule::RenameVsEdit => None,
        MatrixRule::RemoveVsDependent => {
            if a == OpKind::Remove && b == OpKind::Remove {
                return None;
            }
            let remove = AuthorOp::Remove {
                target: w.oak,
                reason: "gone".into(),
            };
            let other = if a == OpKind::Remove {
                oak_write(b, w)?
            } else {
                oak_write(a, w)?
            };
            Some((remove, other))
        }
        MatrixRule::RenameVsRename => Some((
            AuthorOp::Rename {
                target: w.oak,
                to: name("brass_door"),
            },
            AuthorOp::Rename {
                target: w.oak,
                to: name("iron_door"),
            },
        )),
        MatrixRule::CanonNoBulk => Some((
            sample_op(OpKind::AddCanonDiff, 0, w),
            sample_op(OpKind::AddCanonDiff, 1, w),
        )),
        MatrixRule::PatternUnlessIdentical => Some((sample_op(a, 0, w), sample_op(b, 1, w))),
        MatrixRule::MergeByChildId => {
            let c = change("same-locus");
            let anchor = derive_op_anchor(&name("hearth"), c, b"same");
            Some((
                AuthorOp::AddLocus {
                    module: w.module,
                    anchor,
                    name: name("alpha"),
                    kind: LocusKind::Relic,
                },
                AuthorOp::AddLocus {
                    module: w.module,
                    anchor,
                    name: name("beta"),
                    kind: LocusKind::Relic,
                },
            ))
        }
        MatrixRule::CoalesceIfIdentical | MatrixRule::CommuteIfDisjoint => {
            if a != b {
                return None;
            }
            match a {
                OpKind::AddFact => Some((qty(w.module, w.oak, 1), qty(w.module, w.oak, 9))),
                OpKind::AddModule => {
                    let m1 = extra_module("dup");
                    let mut m2 = m1.clone();
                    m2.version = 2;
                    Some((
                        AuthorOp::AddModule { module: m1 },
                        AuthorOp::AddModule { module: m2 },
                    ))
                }
                OpKind::AddCanonDiff => Some((
                    sample_op(OpKind::AddCanonDiff, 0, w),
                    sample_op(OpKind::AddCanonDiff, 1, w),
                )),
                _ => None,
            }
        }
    }
}

#[test]
fn rename_and_field_edit_merge_by_identity() {
    let w = world();
    let rename = AuthorOp::Rename {
        target: w.oak,
        to: name("brass_door"),
    };
    let edit = qty(w.module, w.oak, 42);
    let merged = merge_ops(&w.snap, change("ren"), &[rename], change("qty"), &[edit]).unwrap();
    let object = merged.lookup(w.oak).unwrap();
    assert_eq!(object.name.as_str(), "brass_door");
    assert_eq!(object.anchor, w.oak);
    assert_eq!(merged.qty(w.oak, &name("mass_g")), Some(42));
}

#[test]
fn rename_vs_rename_conflicts_unless_identical() {
    let w = world();
    let a = AuthorOp::Rename {
        target: w.oak,
        to: name("brass_door"),
    };
    let b = AuthorOp::Rename {
        target: w.oak,
        to: name("iron_door"),
    };
    let err = merge_ops(
        &w.snap,
        change("a"),
        std::slice::from_ref(&a),
        change("b"),
        &[b],
    )
    .unwrap_err();
    assert!(matches!(err, AiError::Conflict(w) if w.reason == ConflictReason::RenameClash));
    let merged = merge_ops(
        &w.snap,
        change("a"),
        std::slice::from_ref(&a),
        change("b"),
        std::slice::from_ref(&a),
    )
    .unwrap();
    assert_eq!(merged.lookup(w.oak).unwrap().name.as_str(), "brass_door");
}

#[test]
fn remove_vs_dependent_edit_returns_witness() {
    let w = world();
    let remove = AuthorOp::Remove {
        target: w.oak,
        reason: "scrapped".into(),
    };
    let edit = rel(w.module, w.hero, w.oak);
    let err = merge_ops(
        &w.snap,
        change("rm"),
        std::slice::from_ref(&remove),
        change("rel"),
        &[edit],
    )
    .unwrap_err();
    match err {
        AiError::Conflict(witness) => {
            assert_eq!(witness.reason, ConflictReason::RemoveDependent);
            assert_eq!(witness.target, w.oak);
            assert_eq!(witness.dependent, Some(w.hero));
        }
        other => panic!("expected witness, got {other}"),
    }
    let qty_edit = qty(w.module, w.oak, 3);
    let err = merge_ops(&w.snap, change("rm"), &[remove], change("qty"), &[qty_edit]).unwrap_err();
    assert!(matches!(err, AiError::Conflict(w) if w.reason == ConflictReason::RemoveDependent));
}

#[test]
fn rebase_stale_precondition_fails_closed() {
    let base_dir = scratch("base-rebase");
    let ws = scratch("ws-rebase");
    write_bundle(&hearth_bundle(), &base_dir).unwrap();
    let mut store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
    let id = store
        .create_from_bundle(
            hearth_bundle(),
            ChangeScope::unrestricted(),
            TxBudget::default(),
        )
        .unwrap();
    let w = world();
    store.apply(id, vec![qty(w.module, w.oak, 5)]).unwrap();
    let mut gone = hearth_bundle();
    klotho_author::apply_edit(
        &mut gone,
        klotho_author::SemanticEdit::Remove {
            target: w.oak,
            reason: "removed in current".into(),
        },
    )
    .unwrap();
    let err = store.rebase(id, &gone).unwrap_err();
    assert!(
        matches!(err, AiError::Precondition(_)) || matches!(err, AiError::Conflict(_)),
        "{err}"
    );
    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn rebase_create_then_edit_onto_unchanged_base() {
    let base_dir = scratch("base-rebase-create");
    let ws = scratch("ws-rebase-create");
    write_bundle(&hearth_bundle(), &base_dir).unwrap();
    let mut store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
    let bundle = hearth_bundle();
    let id = store
        .create_from_bundle(
            bundle.clone(),
            ChangeScope::unrestricted(),
            TxBudget::default(),
        )
        .unwrap();
    let w = world();
    let locus = add_locus(w.module, change("new"), "stool", "stool");
    let anchor = match &locus {
        AuthorOp::AddLocus { anchor, .. } => *anchor,
        _ => panic!("locus"),
    };
    store
        .apply(id, vec![locus, qty(w.module, anchor, 3)])
        .unwrap();
    let before = store.snapshot(id).unwrap().content_hash().unwrap();
    let after = store.rebase(id, &bundle).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        store.snapshot(id).unwrap().qty(anchor, &name("mass_g")),
        Some(3)
    );
    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn scoped_add_locus_requires_parent_module() {
    let base_dir = scratch("base-scope");
    let ws = scratch("ws-scope");
    let mut bundle = hearth_bundle();
    let extra = extra_module("other");
    let other = extra.anchor;
    klotho_author::apply_edit(
        &mut bundle,
        klotho_author::SemanticEdit::AddModule { module: extra },
    )
    .unwrap();
    write_bundle(&bundle, &base_dir).unwrap();
    let mut store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
    let main = bundle
        .modules
        .iter()
        .find(|m| m.id.as_str() == "main")
        .unwrap()
        .anchor;
    let mut scope = ChangeScope::unrestricted();
    scope.modules = BTreeSet::from([main]);
    scope.anchors = BTreeSet::new();
    let id = store
        .create_from_bundle(bundle, scope, TxBudget::default())
        .unwrap();
    store
        .apply(
            id,
            vec![add_locus(main, change("in-main"), "stool", "stool")],
        )
        .unwrap();
    let err = store
        .apply(
            id,
            vec![add_locus(other, change("in-other"), "crate", "crate")],
        )
        .unwrap_err();
    assert!(matches!(err, AiError::Scope(_)), "{err}");
    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn coalesce_removes_with_different_reasons() {
    let w = world();
    let a = AuthorOp::Remove {
        target: w.oak,
        reason: "one".into(),
    };
    let b = AuthorOp::Remove {
        target: w.oak,
        reason: "two".into(),
    };
    let merged = merge_ops(&w.snap, change("a"), &[a], change("b"), &[b]).unwrap();
    assert!(merged.tombstoned(w.oak));
    assert!(merged.lookup(w.oak).is_none());
}

#[test]
fn lease_exclusivity_and_expiry_does_not_overwrite() {
    let base_dir = scratch("base-lease");
    let ws = scratch("ws-lease");
    write_bundle(&hearth_bundle(), &base_dir).unwrap();
    let mut store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
    let a = store
        .create_from_bundle(
            hearth_bundle(),
            ChangeScope::unrestricted(),
            TxBudget::default(),
        )
        .unwrap();
    let b = store
        .create_from_bundle(
            hearth_bundle(),
            ChangeScope::unrestricted(),
            TxBudget::default(),
        )
        .unwrap();
    let w = world();
    store.acquire_lease(a, w.oak).unwrap();
    let err = store.acquire_lease(b, w.oak).unwrap_err();
    assert!(matches!(err, AiError::LeaseHeld { .. }), "{err}");
    store.advance_clock(64);
    assert!(store.lease_expired(store.transaction(a).unwrap().lease.unwrap()));
    store.acquire_lease(b, w.oak).unwrap();
    store.apply(b, vec![qty(w.module, w.oak, 1)]).unwrap();
    let b_bundle = store.snapshot(b).unwrap().bundle();
    store.apply(a, vec![qty(w.module, w.oak, 2)]).unwrap();
    let err = store.rebase(a, &b_bundle).unwrap_err();
    assert!(
        matches!(err, AiError::Conflict(_) | AiError::Precondition(_)),
        "expired lease must not overwrite: {err}"
    );
    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn tombstone_and_alias_rules() {
    let mut snap = world().snap;
    let w = world();
    apply_ops(
        &mut snap,
        change("rm"),
        &[AuthorOp::Remove {
            target: w.oak,
            reason: "gone".into(),
        }],
    )
    .unwrap();
    let err = apply_ops(
        &mut snap,
        change("reuse"),
        &[add_locus(w.module, change("reuse"), "x", "oak_door")],
    )
    .unwrap_err();
    assert!(err.to_string().contains("tombstone") || err.to_string().contains("oak_door"));
    let mut snap = world().snap;
    apply_ops(
        &mut snap,
        change("ren"),
        &[AuthorOp::Rename {
            target: w.oak,
            to: name("brass_door"),
        }],
    )
    .unwrap();
    let err = apply_ops(
        &mut snap,
        change("alias"),
        &[AuthorOp::Rename {
            target: w.barrel,
            to: name("oak_door"),
        }],
    )
    .unwrap_err();
    assert!(err.to_string().contains("alias") || err.to_string().contains("oak_door"));
}

#[test]
fn submit_does_not_write_live_project() {
    let base_dir = scratch("base-submit");
    let ws = scratch("ws-submit");
    write_bundle(&hearth_bundle(), &base_dir).unwrap();
    let before = dir_bytes(&base_dir);
    let mut store = TransactionStore::open(&ws, &base_dir.join("project.ron")).unwrap();
    let id = store
        .create(ChangeScope::unrestricted(), TxBudget::default())
        .unwrap();
    let w = world();
    store.apply(id, vec![qty(w.module, w.oak, 8)]).unwrap();
    store.submit(id).unwrap();
    assert_eq!(dir_bytes(&base_dir), before);
    assert_eq!(store.review_queue().len(), 1);
    let _ = fs::remove_dir_all(&base_dir);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn instantiate_records_pattern_and_journey_stays_fail_closed() {
    let w = world();
    let mut snap = w.snap.clone();
    apply_ops(
        &mut snap,
        change("p"),
        &[sample_op(OpKind::Instantiate, 0, &w)],
    )
    .unwrap();
    assert!(
        snap.modules[0]
            .patterns
            .iter()
            .any(|p| p.pattern.as_str() == "traversal.door_key")
    );
    let err = apply_ops(
        &mut snap,
        change("j"),
        &[sample_op(OpKind::AddJourney, 0, &w)],
    )
    .unwrap_err();
    assert!(matches!(err, AiError::FailClosed(OpKind::AddJourney)));
}

#[test]
fn assets_coexist() {
    let w = world();
    let a = AuthorOp::BindAsset {
        locus: w.oak,
        request: AssetRequestId::derive(b"one"),
    };
    let b = AuthorOp::BindAsset {
        locus: w.oak,
        request: AssetRequestId::derive(b"two"),
    };
    let merged = merge_ops(&w.snap, change("a"), &[a], change("b"), &[b]).unwrap();
    assert_eq!(merged.assets.get(&w.oak).map(|s| s.len()), Some(2));
}

#[test]
fn pose_and_rel_are_disjoint_from_qty() {
    let w = world();
    let merged = merge_ops(
        &w.snap,
        change("q"),
        &[qty(w.module, w.oak, 1)],
        change("p"),
        &[pose(w.module, w.barrel)],
    )
    .unwrap();
    assert_eq!(merged.qty(w.oak, &name("mass_g")), Some(1));
}
