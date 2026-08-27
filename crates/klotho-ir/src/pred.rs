//! Predicate AST (§10). Eval is `klotho-canon`; this crate only stores and checks shape.

use serde::{Deserialize, Serialize};

use klotho_core::{IVec3, Mm, SimLod};

use crate::agency::Channel;
use crate::error::IrError;
use crate::name::Name;
use crate::rel::Rel;
use crate::target::Slot;
use crate::verb::Verb;

/// Quantity comparison.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Cmp {
    /// `<`
    Lt,
    /// `≤`
    Le,
    /// `=`
    Eq,
    /// `≥`
    Ge,
    /// `>`
    Gt,
}

/// Who emitted the current proposal.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum SourceKind {
    /// `Proposal::Player`.
    Player,
    /// `Proposal::Mind`.
    Mind,
    /// Space proposer.
    Space,
    /// Motion proposer.
    Motion,
    /// `Proposal::Infer`.
    Infer,
    /// Phys proposer (`ProposalKind::Phys`).
    Phys,
    /// Place load/evict (`ProposalKind::Residency`).
    Residency,
}

/// Closed-world predicate. Combinators are binary `And` / `Or`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Pred {
    /// Projection bitset.
    Affordance(Slot, Name),
    /// Relation row.
    Rel(Slot, Rel, Slot),
    /// Quantity compare.
    Qty(Slot, Name, Cmp, i32),
    /// Current proposal verb.
    EqVerb(Verb),
    /// A `RiteMachine` row exists for this actor + id.
    RiteActive(Name),
    /// Conservative AABB distance ≤ Mm.
    AabbNear(Slot, Slot, Mm),
    /// Current tick is inside that WAIT.
    InWindow(Name, Channel),
    /// Knows table.
    Knows(Slot, Name),
    /// Proposer kind.
    SourceIs(SourceKind),
    /// On the *current* proposal.
    AgencyClaimed(Channel),
    /// Sugar in Canon: `Qty(s, heat) Ge 400`. Stored as an atom so authors can write it.
    Burning(Slot),
    /// Sugar: Opaque ∧ LockedBy-self. Stored as an atom.
    OpaqueClosed(Slot),
    /// Current Space/Motion swept AABB vs any OpaqueClosed hull.
    SweptHitsOpaqueClosed,
    /// `sleep_ticks == 0`.
    IslandAwake(Slot),
    /// Slot equality.
    SelfIs(Slot),
    /// Slot equality.
    TargetIs(Slot),
    /// Slot equality.
    OtherIs(Slot),
    /// Hitscan vs hulls. Eval is false until phys (AAA-08).
    RayHits {
        /// Ray origin.
        from: Slot,
        /// Direction, millimetres.
        dir: IVec3,
        /// Maximum length.
        max: Mm,
        /// Hull mask (engine bits).
        mask: u8,
    },
    /// Sim LOD. Missing column evaluates as `Full`.
    SimLodIs(Slot, SimLod),
    /// Place membership. Until the place column exists, this is `Rel(s, In, p)`.
    InPlace(Slot, Slot),
    /// Conjunction.
    And(Box<Pred>, Box<Pred>),
    /// Disjunction.
    Or(Box<Pred>, Box<Pred>),
    /// Negation.
    Not(Box<Pred>),
    /// Scan cap 64. `pred` is quantifier-free.
    ExistsRelated {
        /// Origin slot.
        of: Slot,
        /// Edge label.
        rel: Rel,
        /// Quantifier-free predicate over `Other`.
        pred: Box<Pred>,
    },
    /// Scan cap 64 plus compare.
    CountRelated {
        /// Origin slot.
        of: Slot,
        /// Edge label.
        rel: Rel,
        /// Quantifier-free predicate over `Other`.
        pred: Box<Pred>,
        /// Compare against `n`.
        cmp: Cmp,
        /// Threshold.
        n: i32,
    },
}

impl Pred {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        self.check_inner(false)
    }

    fn check_inner(&self, in_quant: bool) -> Result<(), IrError> {
        match self {
            Self::Affordance(s, n) | Self::Qty(s, n, _, _) | Self::Knows(s, n) => {
                s.check()?;
                n.check()
            }
            Self::Rel(a, _, b) | Self::AabbNear(a, b, _) | Self::InPlace(a, b) => {
                a.check()?;
                b.check()
            }
            Self::RayHits { from, .. } => from.check(),
            Self::EqVerb(_)
            | Self::SourceIs(_)
            | Self::AgencyClaimed(_)
            | Self::SweptHitsOpaqueClosed => Ok(()),
            Self::RiteActive(n) => n.check(),
            Self::InWindow(n, _) => n.check(),
            Self::Burning(s)
            | Self::OpaqueClosed(s)
            | Self::IslandAwake(s)
            | Self::SelfIs(s)
            | Self::TargetIs(s)
            | Self::OtherIs(s)
            | Self::SimLodIs(s, _) => s.check(),
            Self::And(a, b) | Self::Or(a, b) => {
                a.check_inner(in_quant)?;
                b.check_inner(in_quant)
            }
            Self::Not(a) => a.check_inner(in_quant),
            Self::ExistsRelated { of, pred, .. } | Self::CountRelated { of, pred, .. } => {
                if in_quant {
                    return Err(IrError::NestedQuantifier);
                }
                of.check()?;
                pred.check_inner(true)
            }
        }
    }
}
