//! Authoring Canon diffs. Compiled tables live in `klotho-canon`.

use serde::{Deserialize, Serialize};

use klotho_core::{IVec3, Mm};

use crate::agency::Channel;
use crate::error::IrError;
use crate::name::Name;
use crate::pred::Pred;
use crate::rel::Rel;
use crate::target::Slot;

/// Soft cost on a Law `ought`. Hearth uses `None`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct Cost {
    /// Resource spent.
    pub res: Name,
    /// Amount.
    pub amount: i32,
}

impl Cost {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        self.res.check()
    }
}

/// Always-on invariant / conservation / admission rule.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct Law {
    /// Authoring id (`"carry.mass"`).
    pub id: Name,
    /// When this law is considered.
    pub when: Pred,
    /// Body evaluated on the speculative post-state (K21).
    pub body: LawBody,
}

impl Law {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        self.id.check()?;
        self.when.check()?;
        self.body.check()
    }
}

/// Law body. Continuous writers are only `Ramp` and `Spread`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum LawBody {
    /// Admission predicate.
    Pred {
        /// Must hold or the proposal rolls back.
        must: Pred,
        /// Optional soft cost. `None` in Hearth.
        ought: Option<Cost>,
    },
    /// Self-qty on dirty loci matching `when`.
    Ramp {
        /// Resource.
        res: Name,
        /// Added per tick.
        per_tick: i32,
        /// QtyChanged when `floor(qty/quantum)` changes.
        quantum: i32,
        /// Clamp.
        cap: i32,
    },
    /// Neighbor write + AWAKE. Rejected if it would exceed Cap.
    Spread {
        /// Resource.
        res: Name,
        /// Added per tick.
        per_tick: i32,
        /// AabbNear radius.
        near: Mm,
        /// Global cap on marked loci.
        cap_global: u16,
        /// Ignite threshold (Hearth heat = 400).
        ignite_at: i32,
    },
    /// Admission-time conservation over a relation.
    Conserve {
        /// Resource.
        res: Name,
        /// Relation summing the conserved qty (`WieldedBy`, `Owes`).
        over: Rel,
    },
    /// Kernel counter, not a 4k scan.
    Cap {
        /// Mark predicate (e.g. `Burning(Self)`).
        mark: Pred,
        /// Maximum marked loci.
        n: u16,
        /// Optional membership (`(In, Name("hearth"))`).
        require_rel: Option<(Rel, Slot)>,
    },
}

impl LawBody {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        match self {
            Self::Pred { must, ought } => {
                must.check()?;
                if let Some(c) = ought {
                    c.check()?;
                }
                Ok(())
            }
            Self::Ramp { res, .. } | Self::Spread { res, .. } | Self::Conserve { res, .. } => {
                res.check()
            }
            Self::Cap {
                mark, require_rel, ..
            } => {
                mark.check()?;
                if let Some((_, s)) = require_rel {
                    s.check()?;
                }
                Ok(())
            }
        }
    }
}

/// Stable capability. Requires are predicates; grants are verb/name tags.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct Affordance {
    /// `"Portable"`, `"Lockable"`, …
    pub id: Name,
    /// Must hold for the affordance to apply.
    pub requires: Vec<Pred>,
    /// Verbs this capability enables (`"Carry"`, `"Use"`).
    pub grants: Vec<Name>,
    /// Mutually exclusive affordance ids.
    pub conflicts: Vec<Name>,
}

impl Affordance {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        self.id.check()?;
        for p in &self.requires {
            p.check()?;
        }
        for n in self.grants.iter().chain(self.conflicts.iter()) {
            n.check()?;
        }
        Ok(())
    }
}

/// Complete / halt status.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Status {
    /// Rite succeeded.
    Success,
    /// Rite failed.
    Fail,
}

/// `BIND` source.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum BindSrc {
    /// Intent target.
    Target,
    /// Acting locus. Serialized as `Self`.
    #[serde(rename = "Self")]
    This,
    /// First related neighbor.
    Related(Rel),
}

impl BindSrc {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        Ok(())
    }
}

/// One ISA op. Fail targets are explicit `pc` values; 04a checks they exist.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum RiteOp {
    /// End.
    Halt(Status),
    /// If pred is false, jump to fail_pc. Appendix A `Guard(pred, fail: 10)` is
    /// rewritten to `Guard(pred, 10)` in [`crate::from_ron`].
    Guard(Pred, u16),
    /// Subtract qty; jump to fail_pc if insufficient.
    Spend(Name, i32, u16),
    /// Commit the burst and yield (K21). Channel gates who may resume.
    Wait(u16, Option<Channel>),
    /// Append a Trace event kind.
    Emit(Name),
    /// Conditional jump.
    Branch(Pred, u16, u16),
    /// Bind a slot.
    Bind(BindSrc),
    /// Set a quantity.
    Setq(Slot, Name, i32),
    /// Add a relation.
    RelAdd(Slot, Rel, Slot),
    /// Delete a relation.
    RelDel(Slot, Rel, Slot),
    /// Unsleep the island.
    Awake(Slot),
    /// Alias of Halt with Success/Fail.
    Complete(Status),
    /// Emit `TraceBody::Spawned`. Locus apply is later.
    Spawn(Name),
    /// Write [`klotho_core::PhysRequest`] on the acting locus. Not a `Qty`.
    PhysReq {
        /// Linear request, millimetres.
        lin: IVec3,
        /// Angular request, millidegrees.
        ang: IVec3,
    },
}

impl RiteOp {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        match self {
            Self::Halt(_) | Self::Wait(_, _) | Self::Complete(_) => Ok(()),
            Self::Guard(pred, _) | Self::Branch(pred, _, _) => pred.check(),
            Self::Spend(n, _, _) | Self::Emit(n) | Self::Spawn(n) => n.check(),
            Self::Bind(b) => b.check(),
            Self::PhysReq { .. } => Ok(()),
            Self::Setq(s, n, _) => {
                s.check()?;
                n.check()
            }
            Self::RelAdd(a, _, b) | Self::RelDel(a, _, b) => {
                a.check()?;
                b.check()
            }
            Self::Awake(s) => s.check(),
        }
    }
}

/// CFG node. Canonical RON is tagged (`Op(...)` / `Labeled(...)`).
/// [`crate::from_ron`] also accepts Appendix A bare ops and `{ pc, op }` maps
/// by normalizing them first.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum RiteNode {
    /// Op at the next implicit pc (`Op(Bind(Target))`).
    Op(RiteOp),
    /// Explicit pc (`Labeled(pc: 0, op: Bind(Target))`).
    Labeled {
        /// Program counter.
        pc: u16,
        /// Op at that pc.
        op: RiteOp,
    },
}

impl RiteNode {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        match self {
            Self::Op(op) | Self::Labeled { op, .. } => op.check(),
        }
    }
}

/// Authoring rite CFG. Cook compiles to a `RiteChunk`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct RiteGraph {
    /// `"lockpick"`, `"trade.offer"`.
    pub id: Name,
    /// Per-tick op cap for this rite (Hearth lockpick = 32).
    pub cap_steps: u16,
    /// Wall ticks (Hearth lockpick = 180).
    pub cap_ticks: u16,
    /// Entry pc.
    pub entry: u16,
    /// Nodes. May be unlabeled; trade.goldens use labeled pcs.
    pub nodes: Vec<RiteNode>,
}

impl RiteGraph {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        self.id.check()?;
        if self.cap_steps == 0 || self.cap_ticks == 0 {
            return Err(IrError::InvalidRiteCap);
        }
        for n in &self.nodes {
            n.check()?;
        }
        Ok(())
    }
}

/// Episode / encounter chart. v1 stores id + notes; 04a may grow this.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct Beat {
    /// `"evening_trade"`.
    pub id: Name,
    /// Author notes.
    pub notes: String,
}

impl Beat {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        self.id.check()
    }
}

/// A Canon patch in an [`crate::IntentDoc`]. `RetractLaw` is cook-time only (K16).
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum CanonDiff {
    /// Add a Law.
    AddLaw(Law),
    /// Retract a Law. Runtime Canon is frozen; this is cook-only.
    RetractLaw {
        /// Law id.
        id: Name,
        /// Why.
        reason: String,
    },
    /// Add an Affordance.
    AddAffordance(Affordance),
    /// Add a Rite.
    AddRite(RiteGraph),
    /// Add a Beat.
    AddBeat(Beat),
}

impl CanonDiff {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        match self {
            Self::AddLaw(l) => l.check(),
            Self::RetractLaw { id, reason } => {
                id.check()?;
                if reason.is_empty() {
                    Err(IrError::EmptyName)
                } else {
                    Ok(())
                }
            }
            Self::AddAffordance(a) => a.check(),
            Self::AddRite(r) => r.check(),
            Self::AddBeat(b) => b.check(),
        }
    }
}
