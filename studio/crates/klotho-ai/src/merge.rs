//! Three-way semantic merge using cells and the conflict matrix.

use crate::cells::declare;
use crate::conflict::{MergeClass, classify_pair};
use crate::error::AiError;
use crate::exec::apply_ops;
use crate::ids::{ChangeId, FieldId};
use crate::ops::AuthorOp;
use crate::workspace::AuthoringSnapshot;

/// Merge two change lists onto `base` using cells and the conflict matrix.
pub fn merge_ops(
    base: &AuthoringSnapshot,
    a_id: ChangeId,
    a_ops: &[AuthorOp],
    b_id: ChangeId,
    b_ops: &[AuthorOp],
) -> Result<AuthoringSnapshot, AiError> {
    for a in a_ops {
        for b in b_ops {
            match classify_pair(base, a_id, a, b_id, b) {
                MergeClass::Conflict(w) => return Err(AiError::Conflict(w)),
                MergeClass::Commute
                | MergeClass::Coalesce
                | MergeClass::Merge
                | MergeClass::Coexist => {}
            }
        }
    }
    let mut combined: Vec<(ChangeId, AuthorOp)> = a_ops
        .iter()
        .map(|op| (a_id, op.clone()))
        .chain(b_ops.iter().map(|op| (b_id, op.clone())))
        .collect();
    combined.sort_by(|(id_a, op_a), (id_b, op_b)| {
        let ca = primary_cell(op_a);
        let cb = primary_cell(op_b);
        ca.anchor
            .cmp(&cb.anchor)
            .then(ca.field.cmp(&cb.field))
            .then(id_a.cmp(id_b))
    });
    let mut snap = base.clone();
    let mut seen: Vec<(AuthorOp, ChangeId)> = Vec::new();
    for (id, op) in combined {
        if seen.iter().any(|(kept, _)| coalesce_eq(kept, &op)) {
            seen.push((op, id));
            continue;
        }
        apply_ops(&mut snap, id, std::slice::from_ref(&op))?;
        seen.push((op, id));
    }
    Ok(snap)
}

fn coalesce_eq(a: &AuthorOp, b: &AuthorOp) -> bool {
    match (a, b) {
        (AuthorOp::Remove { target: t1, .. }, AuthorOp::Remove { target: t2, .. }) => t1 == t2,
        _ => a == b,
    }
}

/// Three-way merge of snapshots: `base` / `current` / `proposed`.
pub fn merge_snapshots(
    base: &AuthoringSnapshot,
    current: &AuthoringSnapshot,
    proposed: &AuthoringSnapshot,
    current_id: ChangeId,
    proposed_id: ChangeId,
) -> Result<AuthoringSnapshot, AiError> {
    let current_ops = diff_ops(base, current);
    let proposed_ops = diff_ops(base, proposed);
    merge_ops(base, current_id, &current_ops, proposed_id, &proposed_ops)
}

/// Best-effort semantic diff from `base` to `head` as ops.
#[must_use]
pub fn diff_ops(base: &AuthoringSnapshot, head: &AuthoringSnapshot) -> Vec<AuthorOp> {
    let mut ops = Vec::new();
    for module in &head.modules {
        if !base.exists(module.anchor) {
            ops.push(AuthorOp::AddModule {
                module: module.clone(),
            });
        }
    }
    for module in &head.modules {
        let base_mod = base.modules.iter().find(|m| m.anchor == module.anchor);
        for object in &module.object_anchors {
            let existed = base.lookup(object.anchor).cloned();
            if existed.is_none() {
                if object.kind == klotho_ir::AnchorKind::Locus {
                    ops.push(AuthorOp::AddLocus {
                        module: module.anchor,
                        anchor: object.anchor,
                        name: object.name.clone(),
                        kind: locus_kind(module, &object.name),
                    });
                }
            } else if let Some(prev) = existed {
                if prev.name != object.name {
                    ops.push(AuthorOp::Rename {
                        target: object.anchor,
                        to: object.name.clone(),
                    });
                }
            }
        }
        for fact in &module.body.seed {
            match fact {
                klotho_ir::SeedFact::Qty { of, res, value } => {
                    if let Some(id) = name_anchor(module, of) {
                        let old = base.qty(id, res);
                        if old != Some(*value) {
                            ops.push(AuthorOp::AddFact {
                                module: module.anchor,
                                fact: klotho_author::AnchoredSeedFact::Qty {
                                    of: id,
                                    res: res.clone(),
                                    value: *value,
                                },
                            });
                        }
                    }
                }
                klotho_ir::SeedFact::Pose { of, pose } => {
                    if let Some(id) = name_anchor(module, of) {
                        let had = base_mod.is_some_and(|m| {
                            m.body.seed.iter().any(|f| {
                                matches!(f, klotho_ir::SeedFact::Pose { of: n, pose: p } if n == of && p == pose)
                            })
                        });
                        if !had {
                            ops.push(AuthorOp::AddFact {
                                module: module.anchor,
                                fact: klotho_author::AnchoredSeedFact::Pose {
                                    of: id,
                                    pose: *pose,
                                },
                            });
                        }
                    }
                }
                klotho_ir::SeedFact::Rel { a, rel, b } => {
                    let Some(aa) = name_anchor(module, a) else {
                        continue;
                    };
                    let Some(bb) = name_anchor(module, b) else {
                        continue;
                    };
                    let had = base.rels(aa).iter().any(|f| {
                        matches!(
                            f,
                            klotho_author::AnchoredSeedFact::Rel { a: x, rel: r, b: y }
                                if *x == aa && *y == bb && r == rel
                        )
                    });
                    if !had {
                        ops.push(AuthorOp::AddFact {
                            module: module.anchor,
                            fact: klotho_author::AnchoredSeedFact::Rel {
                                a: aa,
                                rel: *rel,
                                b: bb,
                            },
                        });
                    }
                }
                klotho_ir::SeedFact::Locus { .. } => {}
            }
        }
        let base_diffs = base_mod
            .map(|m| m.body.canon_diffs.as_slice())
            .unwrap_or(&[]);
        for diff in &module.body.canon_diffs {
            if !base_diffs.contains(diff) {
                ops.push(AuthorOp::AddCanonDiff {
                    module: module.anchor,
                    diff: diff.clone(),
                });
            }
        }
        for tomb in &module.tombstones {
            let already = base.tombstoned(tomb.anchor);
            if !already {
                ops.push(AuthorOp::Remove {
                    target: tomb.anchor,
                    reason: tomb.reason.clone(),
                });
            }
        }
    }
    for (locus, set) in &head.assets {
        let old = base.assets.get(locus);
        for req in set {
            if !old.is_some_and(|s| s.contains(req)) {
                ops.push(AuthorOp::BindAsset {
                    locus: *locus,
                    request: *req,
                });
            }
        }
    }
    for (target, set) in &head.references {
        let old = base.references.get(target);
        for reference in set {
            if !old.is_some_and(|s| s.contains(reference)) {
                ops.push(AuthorOp::AddReference {
                    target: *target,
                    reference: *reference,
                });
            }
        }
    }
    ops
}

fn primary_cell(op: &AuthorOp) -> crate::ids::Cell {
    let decl = declare(op, None);
    decl.writes.first().copied().unwrap_or(crate::ids::Cell {
        anchor: op.primary_anchor(),
        field: FieldId::SeedFact,
    })
}

fn name_anchor(
    module: &klotho_ir::IntentModule,
    name: &klotho_ir::Name,
) -> Option<klotho_ir::AnchorId> {
    module
        .object_anchors
        .iter()
        .find(|o| o.name == *name)
        .map(|o| o.anchor)
}

fn locus_kind(module: &klotho_ir::IntentModule, name: &klotho_ir::Name) -> klotho_core::LocusKind {
    module
        .body
        .seed
        .iter()
        .find_map(|f| match f {
            klotho_ir::SeedFact::Locus { name: n, kind } if n == name => Some(*kind),
            _ => None,
        })
        .unwrap_or(klotho_core::LocusKind::Relic)
}
