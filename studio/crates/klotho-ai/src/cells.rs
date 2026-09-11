//! Read/write cells and value preconditions declared by each op.

use serde::{Deserialize, Serialize};

use klotho_author::AnchoredSeedFact;
use klotho_core::Hash;
use klotho_ir::{AnchorId, Name};

use crate::ids::{Cell, FieldId};
use crate::ops::AuthorOp;
use crate::workspace::AuthoringSnapshot;

/// Value precondition recorded at first apply and re-checked on rebase.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Precondition {
    /// Object must exist.
    AnchorExists(AnchorId),
    /// Object must not exist.
    AnchorAbsent(AnchorId),
    /// Name is not live, aliased, or tombstoned in `module`.
    NameFree {
        /// Module to check.
        module: AnchorId,
        /// Candidate name.
        name: Name,
    },
    /// Cell content hash at apply time.
    FieldHash {
        /// Cell.
        cell: Cell,
        /// Expected hash, including the absent sentinel.
        expected: Hash,
    },
    /// Object is not tombstoned.
    NotTombstoned(AnchorId),
}

/// Cells and preconditions for one op.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct OpDecl {
    /// Cells read.
    pub reads: Vec<Cell>,
    /// Cells written.
    pub writes: Vec<Cell>,
    /// Value preconditions.
    pub preconditions: Vec<Precondition>,
}

/// Declare cells. When `snap` is present, value hashes are captured.
#[must_use]
pub fn declare(op: &AuthorOp, snap: Option<&AuthoringSnapshot>) -> OpDecl {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut preconditions = Vec::new();
    match op {
        AuthorOp::AddModule { module } => {
            writes.push(Cell {
                anchor: module.anchor,
                field: FieldId::ModuleBody,
            });
            preconditions.push(Precondition::AnchorAbsent(module.anchor));
        }
        AuthorOp::Instantiate { instance } => {
            writes.push(Cell {
                anchor: instance.anchor,
                field: FieldId::PatternVersion,
            });
        }
        AuthorOp::SetArgument { instance, .. } => {
            writes.push(Cell {
                anchor: *instance,
                field: FieldId::PatternArg,
            });
            preconditions.push(Precondition::AnchorExists(*instance));
        }
        AuthorOp::AddLocus {
            module,
            anchor,
            name,
            ..
        } => {
            reads.push(Cell {
                anchor: *module,
                field: FieldId::ModuleBody,
            });
            writes.push(Cell {
                anchor: *anchor,
                field: FieldId::Name,
            });
            writes.push(Cell {
                anchor: *anchor,
                field: FieldId::Kind,
            });
            writes.push(Cell {
                anchor: *module,
                field: FieldId::CollectionOrder,
            });
            preconditions.push(Precondition::AnchorExists(*module));
            preconditions.push(Precondition::AnchorAbsent(*anchor));
            preconditions.push(Precondition::NameFree {
                module: *module,
                name: name.clone(),
            });
        }
        AuthorOp::AddFact { fact, .. } => match fact {
            AnchoredSeedFact::Qty { of, .. } => {
                reads.push(Cell {
                    anchor: *of,
                    field: FieldId::Name,
                });
                writes.push(Cell {
                    anchor: *of,
                    field: FieldId::Qty,
                });
                preconditions.push(Precondition::AnchorExists(*of));
                push_field_hash(
                    &mut preconditions,
                    snap,
                    Cell {
                        anchor: *of,
                        field: FieldId::Qty,
                    },
                );
            }
            AnchoredSeedFact::Pose { of, .. } => {
                reads.push(Cell {
                    anchor: *of,
                    field: FieldId::Name,
                });
                writes.push(Cell {
                    anchor: *of,
                    field: FieldId::Pose,
                });
                preconditions.push(Precondition::AnchorExists(*of));
                push_field_hash(
                    &mut preconditions,
                    snap,
                    Cell {
                        anchor: *of,
                        field: FieldId::Pose,
                    },
                );
            }
            AnchoredSeedFact::Rel { a, b, .. } => {
                reads.push(Cell {
                    anchor: *a,
                    field: FieldId::Name,
                });
                reads.push(Cell {
                    anchor: *b,
                    field: FieldId::Name,
                });
                writes.push(Cell {
                    anchor: *a,
                    field: FieldId::Rel,
                });
                writes.push(Cell {
                    anchor: *b,
                    field: FieldId::Rel,
                });
                preconditions.push(Precondition::AnchorExists(*a));
                preconditions.push(Precondition::AnchorExists(*b));
            }
        },
        AuthorOp::AddCanonDiff { module, .. } => {
            writes.push(Cell {
                anchor: *module,
                field: FieldId::CanonDiff,
            });
            preconditions.push(Precondition::AnchorExists(*module));
            push_field_hash(
                &mut preconditions,
                snap,
                Cell {
                    anchor: *module,
                    field: FieldId::CanonDiff,
                },
            );
        }
        AuthorOp::BindAsset { locus, .. } => {
            writes.push(Cell {
                anchor: *locus,
                field: FieldId::AssetBinding,
            });
            preconditions.push(Precondition::AnchorExists(*locus));
        }
        AuthorOp::AddJourney { .. } => {
            writes.push(Cell {
                anchor: AnchorId::ZERO,
                field: FieldId::Journey,
            });
        }
        AuthorOp::AddReference { target, .. } => {
            writes.push(Cell {
                anchor: *target,
                field: FieldId::Reference,
            });
            preconditions.push(Precondition::AnchorExists(*target));
        }
        AuthorOp::Remove { target, .. } => {
            writes.push(Cell {
                anchor: *target,
                field: FieldId::Tombstone,
            });
            writes.push(Cell {
                anchor: *target,
                field: FieldId::Name,
            });
            preconditions.push(Precondition::AnchorExists(*target));
            if let Some(snap) = snap {
                for dep in snap.dependents(*target) {
                    reads.push(Cell {
                        anchor: dep,
                        field: FieldId::SeedFact,
                    });
                }
            }
        }
        AuthorOp::Rename { target, to, .. } => {
            reads.push(Cell {
                anchor: *target,
                field: FieldId::Name,
            });
            writes.push(Cell {
                anchor: *target,
                field: FieldId::Name,
            });
            writes.push(Cell {
                anchor: *target,
                field: FieldId::Alias,
            });
            preconditions.push(Precondition::AnchorExists(*target));
            preconditions.push(Precondition::NotTombstoned(*target));
            if let Some(module) = snap.and_then(|s| s.owning_module(*target)) {
                preconditions.push(Precondition::NameFree {
                    module,
                    name: to.clone(),
                });
            }
        }
    }
    reads.sort();
    reads.dedup();
    writes.sort();
    writes.dedup();
    OpDecl {
        reads,
        writes,
        preconditions,
    }
}

fn push_field_hash(pre: &mut Vec<Precondition>, snap: Option<&AuthoringSnapshot>, cell: Cell) {
    if let Some(snap) = snap {
        pre.push(Precondition::FieldHash {
            cell,
            expected: snap.cell_hash(cell),
        });
    }
}
