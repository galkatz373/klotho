//! Checked-in operation conflict matrix and pairwise classification.

use core::fmt;

use serde::{Deserialize, Serialize};

use klotho_author::AnchoredSeedFact;
use klotho_ir::AnchorId;

use crate::cells::declare;
use crate::ids::{Cell, ChangeId, FieldId};
use crate::ops::{AuthorOp, OpKind};
use crate::workspace::AuthoringSnapshot;

/// Coarse matrix rule for an op-kind pair.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum MatrixRule {
    /// Apply both when write cells do not overlap.
    CommuteIfDisjoint,
    /// Same cell, same value: keep one row, both provenance edges.
    CoalesceIfIdentical,
    /// Ordered collection merged by child [`AnchorId`].
    MergeByChildId,
    /// Rename plus a field edit of the same identity.
    RenameVsEdit,
    /// Two renames of the same object.
    RenameVsRename,
    /// Remove versus any read/write of the target or a dependent.
    RemoveVsDependent,
    /// Pattern version/args unless the expanded diff is identical.
    PatternUnlessIdentical,
    /// Asset candidates coexist.
    AssetCoexist,
    /// Canon / geometry never bulk-automerge across writers.
    CanonNoBulk,
}

/// Outcome of classifying two ops.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum MergeClass {
    /// Disjoint writes; apply in canonical order.
    Commute,
    /// Identical values; retain both provenance edges.
    Coalesce,
    /// Rename vs field edit; both land.
    Merge,
    /// Additive candidates.
    Coexist,
    /// Rejected with a witness.
    Conflict(ConflictWitness),
}

/// Why a pair was rejected. Never last-writer-wins.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictWitness {
    /// Classification.
    pub reason: ConflictReason,
    /// Primary object.
    pub target: AnchorId,
    /// Dependent that read or wrote the target, if any.
    pub dependent: Option<AnchorId>,
    /// Overlapping cell, if any.
    pub field: Option<FieldId>,
    /// Left change.
    pub a: ChangeId,
    /// Right change.
    pub b: ChangeId,
}

/// Closed conflict reasons.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ConflictReason {
    /// Same cell, different values.
    OverlappingWrite,
    /// Remove versus a dependent edit.
    RemoveDependent,
    /// Two different new names, or the same name on two objects.
    RenameClash,
    /// Concurrent Canon writers.
    CanonConcurrent,
    /// Concurrent pattern version/argument writers.
    PatternConcurrent,
    /// Conflicting relative-order constraints.
    OrderConstraint,
}

impl fmt::Display for ConflictWitness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} target={} dependent={:?} field={:?}",
            self.reason, self.target, self.dependent, self.field
        )
    }
}

/// Total function over [`OpKind`] pairs.
#[must_use]
pub fn matrix_rule(a: OpKind, b: OpKind) -> MatrixRule {
    use MatrixRule::*;
    use OpKind::*;
    if a == Remove || b == Remove {
        return RemoveVsDependent;
    }
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    match (lo, hi) {
        (Rename, Rename) => RenameVsRename,
        (Rename, AddFact | BindAsset | AddReference | SetArgument | AddCanonDiff | AddLocus) => {
            RenameVsEdit
        }
        (AddLocus, AddLocus) => MergeByChildId,
        (BindAsset, BindAsset) => AssetCoexist,
        (AddCanonDiff, AddCanonDiff) => CanonNoBulk,
        (AddFact, AddFact) => CoalesceIfIdentical,
        (AddReference, AddReference) => AssetCoexist,
        (Instantiate | SetArgument | AddJourney, Instantiate | SetArgument | AddJourney) => {
            PatternUnlessIdentical
        }
        _ => CommuteIfDisjoint,
    }
}

/// Classify two ops against `base`. `a_id` / `b_id` label the witness.
#[must_use]
pub fn classify_pair(
    base: &AuthoringSnapshot,
    a_id: ChangeId,
    a: &AuthorOp,
    b_id: ChangeId,
    b: &AuthorOp,
) -> MergeClass {
    let rule = matrix_rule(a.kind(), b.kind());
    let da = declare(a, Some(base));
    let db = declare(b, Some(base));
    match rule {
        MatrixRule::RemoveVsDependent => classify_remove(base, a_id, a, b_id, b),
        MatrixRule::RenameVsRename => classify_renames(a_id, a, b_id, b),
        MatrixRule::RenameVsEdit => MergeClass::Merge,
        MatrixRule::AssetCoexist => MergeClass::Coexist,
        MatrixRule::CanonNoBulk => {
            if a == b {
                MergeClass::Coalesce
            } else {
                MergeClass::Conflict(witness(
                    ConflictReason::CanonConcurrent,
                    a.primary_anchor(),
                    None,
                    Some(FieldId::CanonDiff),
                    a_id,
                    b_id,
                ))
            }
        }
        MatrixRule::PatternUnlessIdentical => {
            if a == b {
                MergeClass::Coalesce
            } else {
                MergeClass::Conflict(witness(
                    ConflictReason::PatternConcurrent,
                    a.primary_anchor(),
                    None,
                    Some(FieldId::PatternArg),
                    a_id,
                    b_id,
                ))
            }
        }
        MatrixRule::MergeByChildId => classify_collection(a_id, a, b_id, b),
        MatrixRule::CoalesceIfIdentical => {
            classify_same_field(a_id, a, &da.writes, b_id, b, &db.writes)
        }
        MatrixRule::CommuteIfDisjoint => {
            if disjoint(&da.writes, &db.writes) {
                MergeClass::Commute
            } else {
                classify_same_field(a_id, a, &da.writes, b_id, b, &db.writes)
            }
        }
    }
}

fn classify_remove(
    base: &AuthoringSnapshot,
    a_id: ChangeId,
    a: &AuthorOp,
    b_id: ChangeId,
    b: &AuthorOp,
) -> MergeClass {
    let (rid, remove, oid, other) = match (a, b) {
        (AuthorOp::Remove { .. }, _) => (a_id, a, b_id, b),
        (_, AuthorOp::Remove { .. }) => (b_id, b, a_id, a),
        _ => return MergeClass::Commute,
    };
    let AuthorOp::Remove { target, .. } = remove else {
        return MergeClass::Commute;
    };
    if let AuthorOp::Remove { target: t2, .. } = other {
        if t2 == target {
            return MergeClass::Coalesce;
        }
        return MergeClass::Commute;
    }
    let other_decl = declare(other, Some(base));
    let mut involved: Vec<AnchorId> = other_decl
        .reads
        .iter()
        .chain(other_decl.writes.iter())
        .map(|c| c.anchor)
        .collect();
    involved.push(other.primary_anchor());
    if let AuthorOp::AddFact { fact, .. } = other {
        match fact {
            AnchoredSeedFact::Rel { a, b, .. } => {
                involved.push(*a);
                involved.push(*b);
            }
            AnchoredSeedFact::Qty { of, .. } | AnchoredSeedFact::Pose { of, .. } => {
                involved.push(*of);
            }
        }
    }
    let deps = base.dependents(*target);
    let hits_target = involved.contains(target);
    let dependent = involved
        .iter()
        .copied()
        .find(|id| *id != *target && (deps.contains(id) || hits_target))
        .or_else(|| hits_target.then_some(*target))
        .or_else(|| involved.iter().copied().find(|id| deps.contains(id)));
    if hits_target || deps.iter().any(|d| involved.contains(d)) {
        MergeClass::Conflict(ConflictWitness {
            reason: ConflictReason::RemoveDependent,
            target: *target,
            dependent,
            field: other_decl.writes.first().map(|c| c.field),
            a: rid,
            b: oid,
        })
    } else {
        MergeClass::Commute
    }
}

fn classify_renames(a_id: ChangeId, a: &AuthorOp, b_id: ChangeId, b: &AuthorOp) -> MergeClass {
    let (AuthorOp::Rename { target: t1, to: n1 }, AuthorOp::Rename { target: t2, to: n2 }) = (a, b)
    else {
        return MergeClass::Commute;
    };
    if t1 == t2 {
        if n1 == n2 {
            MergeClass::Coalesce
        } else {
            MergeClass::Conflict(witness(
                ConflictReason::RenameClash,
                *t1,
                None,
                Some(FieldId::Name),
                a_id,
                b_id,
            ))
        }
    } else if n1 == n2 {
        MergeClass::Conflict(witness(
            ConflictReason::RenameClash,
            *t1,
            Some(*t2),
            Some(FieldId::Name),
            a_id,
            b_id,
        ))
    } else {
        MergeClass::Commute
    }
}

fn classify_collection(a_id: ChangeId, a: &AuthorOp, b_id: ChangeId, b: &AuthorOp) -> MergeClass {
    match (a, b) {
        (AuthorOp::AddLocus { anchor: x, .. }, AuthorOp::AddLocus { anchor: y, .. }) => {
            if x == y {
                if a == b {
                    MergeClass::Coalesce
                } else {
                    MergeClass::Conflict(witness(
                        ConflictReason::OverlappingWrite,
                        *x,
                        None,
                        Some(FieldId::CollectionOrder),
                        a_id,
                        b_id,
                    ))
                }
            } else {
                MergeClass::Commute
            }
        }
        _ => MergeClass::Commute,
    }
}

fn classify_same_field(
    a_id: ChangeId,
    a: &AuthorOp,
    a_writes: &[Cell],
    b_id: ChangeId,
    b: &AuthorOp,
    b_writes: &[Cell],
) -> MergeClass {
    if disjoint(a_writes, b_writes) {
        return MergeClass::Commute;
    }
    if a == b {
        return MergeClass::Coalesce;
    }
    let field = a_writes
        .iter()
        .find(|c| b_writes.contains(c))
        .map(|c| c.field);
    MergeClass::Conflict(witness(
        ConflictReason::OverlappingWrite,
        a.primary_anchor(),
        Some(b.primary_anchor()),
        field,
        a_id,
        b_id,
    ))
}

fn disjoint(a: &[Cell], b: &[Cell]) -> bool {
    !a.iter().any(|c| b.contains(c))
}

fn witness(
    reason: ConflictReason,
    target: AnchorId,
    dependent: Option<AnchorId>,
    field: Option<FieldId>,
    a: ChangeId,
    b: ChangeId,
) -> ConflictWitness {
    ConflictWitness {
        reason,
        target,
        dependent,
        field,
        a,
        b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_total_and_symmetric() {
        for a in OpKind::ALL {
            for b in OpKind::ALL {
                let r = matrix_rule(a, b);
                let s = matrix_rule(b, a);
                assert_eq!(r, s, "{a} vs {b}");
            }
        }
    }
}
