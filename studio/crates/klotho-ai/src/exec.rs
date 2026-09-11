//! Apply [`AuthorOp`]s to an isolated snapshot.

use klotho_author::{AnchoredSeedFact, SemanticEdit, apply_edit_unlocked, refresh_locks};
use klotho_core::Hash;
use klotho_ir::to_ron;
use klotho_prove::hash_bytes;

use crate::cells::{Precondition, declare};
use crate::error::AiError;
use crate::ids::ChangeId;
use crate::ops::{AuthorOp, OpKind};
use crate::workspace::AuthoringSnapshot;

/// One applied op with captured cells/preconditions.
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedOp {
    /// Operation.
    pub op: AuthorOp,
    /// Declaration at apply time.
    pub reads: Vec<crate::ids::Cell>,
    /// Written cells.
    pub writes: Vec<crate::ids::Cell>,
    /// Captured preconditions.
    pub preconditions: Vec<Precondition>,
    /// Hash of canonical RON.
    pub hash: Hash,
}

/// Apply `ops` in order. Fails closed on stub kinds, missing anchors, and preconditions.
pub fn apply_ops(
    snap: &mut AuthoringSnapshot,
    _change: ChangeId,
    ops: &[AuthorOp],
) -> Result<Vec<PreparedOp>, AiError> {
    let mut prepared = Vec::with_capacity(ops.len());
    for op in ops {
        prepared.push(apply_one_inner(snap, op)?);
    }
    if !ops.is_empty() {
        let mut bundle = snap.bundle();
        refresh_locks(&mut bundle)?;
        snap.project = bundle.project;
        snap.modules = bundle.modules;
    }
    Ok(prepared)
}

fn apply_one_inner(snap: &mut AuthoringSnapshot, op: &AuthorOp) -> Result<PreparedOp, AiError> {
    fail_closed(op)?;
    if let Some(id) = op.created_anchor() {
        if snap.occupied().contains(&id) {
            return Err(AiError::DuplicateAnchor(id.to_string()));
        }
    }
    let decl = declare(op, Some(snap));
    check_listed(snap, &decl.preconditions, op)?;
    match op {
        AuthorOp::BindAsset { locus, request } => {
            if !snap.exists(*locus) {
                return Err(AiError::Author(klotho_author::AuthorError::MissingAnchor(
                    locus.to_string(),
                )));
            }
            snap.assets.entry(*locus).or_default().insert(*request);
        }
        AuthorOp::AddReference { target, reference } => {
            if !snap.exists(*target) {
                return Err(AiError::Author(klotho_author::AuthorError::MissingAnchor(
                    target.to_string(),
                )));
            }
            snap.references
                .entry(*target)
                .or_default()
                .insert(*reference);
        }
        _ => {
            if let Some(edit) = to_edit(op) {
                let mut bundle = snap.bundle();
                apply_edit_unlocked(&mut bundle, edit)?;
                snap.project = bundle.project;
                snap.modules = bundle.modules;
            }
        }
    }
    Ok(PreparedOp {
        op: op.clone(),
        reads: decl.reads,
        writes: decl.writes,
        preconditions: decl.preconditions,
        hash: op_hash(op)?,
    })
}

/// Re-check captured preconditions, applying earlier ops onto a rolling copy of `snap`.
pub fn check_prepared(snap: &AuthoringSnapshot, prepared: &[PreparedOp]) -> Result<(), AiError> {
    let mut rolling = snap.clone();
    let id = ChangeId::derive(b"precheck");
    for item in prepared {
        fail_closed(&item.op)?;
        check_listed(&rolling, &item.preconditions, &item.op)?;
        apply_ops(&mut rolling, id, std::slice::from_ref(&item.op))?;
    }
    Ok(())
}

fn check_listed(
    snap: &AuthoringSnapshot,
    pres: &[Precondition],
    op: &AuthorOp,
) -> Result<(), AiError> {
    for pre in pres {
        match pre {
            Precondition::AnchorExists(id) => {
                if !snap.exists(*id) {
                    return Err(AiError::Precondition(format!("missing {id}")));
                }
            }
            Precondition::AnchorAbsent(id) => {
                if snap.exists(*id) {
                    return Err(AiError::Precondition(format!("already exists {id}")));
                }
            }
            Precondition::NameFree { module, name } => {
                if let AuthorOp::Rename { target, to, .. } = op {
                    if to == name {
                        if let Some(object) = snap.lookup(*target) {
                            if object.name == *name {
                                continue;
                            }
                        }
                    }
                }
                if snap.name_taken(*module, name) {
                    return Err(AiError::Precondition(format!(
                        "name taken {}",
                        name.as_str()
                    )));
                }
            }
            Precondition::FieldHash { cell, expected } => {
                let actual = snap.cell_hash(*cell);
                if actual != *expected {
                    if writes_identical(snap, op, *cell) {
                        continue;
                    }
                    return Err(AiError::Precondition(format!(
                        "stale cell {} {:?}",
                        cell.anchor, cell.field
                    )));
                }
            }
            Precondition::NotTombstoned(id) => {
                if snap.tombstoned(*id) {
                    return Err(AiError::Precondition(format!("tombstoned {id}")));
                }
            }
        }
    }
    Ok(())
}

fn writes_identical(snap: &AuthoringSnapshot, op: &AuthorOp, cell: crate::ids::Cell) -> bool {
    match op {
        AuthorOp::AddFact {
            fact: AnchoredSeedFact::Qty { of, res, value },
            ..
        } if cell.anchor == *of => snap.qty(*of, res) == Some(*value),
        AuthorOp::AddCanonDiff { module, diff, .. } if cell.anchor == *module => snap
            .modules
            .iter()
            .any(|m| m.anchor == *module && m.body.canon_diffs.iter().any(|d| d == diff)),
        AuthorOp::Rename { target, to, .. } if cell.anchor == *target => {
            snap.lookup(*target).is_some_and(|o| o.name == *to)
        }
        _ => false,
    }
}

fn fail_closed(op: &AuthorOp) -> Result<(), AiError> {
    match op.kind() {
        OpKind::AddJourney => Err(AiError::FailClosed(op.kind())),
        _ => Ok(()),
    }
}

fn to_edit(op: &AuthorOp) -> Option<SemanticEdit> {
    match op {
        AuthorOp::AddModule { module } => Some(SemanticEdit::AddModule {
            module: module.clone(),
        }),
        AuthorOp::AddLocus {
            module,
            anchor,
            name,
            kind,
        } => Some(SemanticEdit::AddLocus {
            module: *module,
            anchor: *anchor,
            name: name.clone(),
            kind: *kind,
        }),
        AuthorOp::AddFact { module, fact } => Some(SemanticEdit::AddFact {
            module: *module,
            fact: fact.clone(),
        }),
        AuthorOp::AddCanonDiff { module, diff } => Some(SemanticEdit::AddCanonDiff {
            module: *module,
            diff: diff.clone(),
        }),
        AuthorOp::Remove { target, reason } => Some(SemanticEdit::Remove {
            target: *target,
            reason: reason.clone(),
        }),
        AuthorOp::Rename { target, to } => Some(SemanticEdit::Rename {
            target: *target,
            to: to.clone(),
        }),
        AuthorOp::Instantiate { instance } => Some(SemanticEdit::Instantiate {
            instance: instance.clone(),
        }),
        AuthorOp::SetArgument {
            instance,
            key,
            value,
        } => Some(SemanticEdit::SetArgument {
            instance: *instance,
            key: key.clone(),
            value: value.clone(),
        }),
        AuthorOp::BindAsset { .. }
        | AuthorOp::AddReference { .. }
        | AuthorOp::AddJourney { .. } => None,
    }
}

/// Hash of canonical op RON.
pub fn op_hash(op: &AuthorOp) -> Result<Hash, AiError> {
    let text = to_ron(op).map_err(|e| AiError::Ser(e.to_string()))?;
    Ok(hash_bytes(text.as_bytes()))
}
