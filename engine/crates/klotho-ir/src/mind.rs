//! Mind packets and compiled authoring programs. No [`crate::Agency`].

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use klotho_core::{Mm, Sigil};

use crate::{IntentTarget, IrError, Name, Rel, Verb};

/// Maximum visible facts in one Full planning call (K90).
pub const MAX_MIND_FACTS: usize = 64;
/// Maximum compiled operators in one program (K90).
pub const MAX_MIND_OPERATORS: usize = 32;
/// Maximum simultaneous candidate goals (K90).
pub const MAX_MIND_GOALS: usize = 4;
/// Maximum Far table inputs (K90).
pub const MAX_FAR_INPUTS: usize = 16;

/// GOAP / Beat-emitted desire. Same admission path as the player, minus agency.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MindIntent {
    /// Acting NPC locus.
    pub locus: Sigil,
    /// Verb.
    pub verb: Verb,
    /// Target.
    pub target: IntentTarget,
    /// Debug ranking. Not replicated as authority.
    pub utility: u16,
}

impl MindIntent {
    /// Structural checks.
    pub fn validate(&self) -> Result<(), IrError> {
        self.target.check()
    }
}

/// A locus operand resolved from the actor and immutable seed pins.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum MindRef {
    /// The actor owning the program.
    This,
    /// An immutable seed pin.
    Pin(Name),
    /// First relation target in Projection order; used for squad coordination.
    Related(Rel),
}

impl MindRef {
    fn check(&self) -> Result<(), IrError> {
        if let Self::Pin(name) = self {
            name.check()?;
        }
        Ok(())
    }
}

/// A bounded, visible input fact. Planner scratch derives only from these rows.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum MindQuery {
    /// Constant false, useful as an operator's desired postcondition.
    Never,
    /// Constant true, useful for unconditional authored actions.
    Always,
    /// Projection relation membership.
    Related {
        /// Relation subject.
        a: MindRef,
        /// Relation tag.
        rel: Rel,
        /// Relation object.
        b: MindRef,
    },
    /// Integer Projection quantity threshold.
    QtyAtLeast {
        /// Quantity owner.
        of: MindRef,
        /// Resource name resolved against cooked Canon.
        res: Name,
        /// Inclusive threshold.
        min: i32,
    },
    /// True when any locus has at least `min` of a resource.
    AnyQtyAtLeast {
        /// Resource name resolved against cooked Canon.
        res: Name,
        /// Inclusive threshold.
        min: i32,
    },
    /// Integer XZ Chebyshev distance.
    Near {
        /// First locus.
        a: MindRef,
        /// Second locus.
        b: MindRef,
        /// Inclusive distance.
        within: Mm,
    },
    /// Deterministic Beat clock row.
    TickModulo {
        /// Non-zero period.
        period: u16,
        /// Phase less than `period`.
        phase: u16,
    },
}

impl MindQuery {
    fn check(&self) -> Result<(), IrError> {
        match self {
            Self::Never | Self::Always => Ok(()),
            Self::Related { a, b, .. } | Self::Near { a, b, .. } => {
                a.check()?;
                b.check()
            }
            Self::QtyAtLeast { of, res, .. } => {
                of.check()?;
                res.check()
            }
            Self::AnyQtyAtLeast { res, .. } => res.check(),
            Self::TickModulo { period, phase } if *period == 0 || *phase >= *period => {
                Err(IrError::InvalidMindProgram("invalid Beat clock".into()))
            }
            Self::TickModulo { .. } => Ok(()),
        }
    }
}

/// One named visible fact in a compiled program.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct MindFact {
    /// Program-local identity.
    pub id: Name,
    /// Projection-derived query.
    pub query: MindQuery,
    /// Whether Far may read or affect this unprotected fact.
    pub far_safe: bool,
}

/// Target for an authored Mind action.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum MindTarget {
    /// No target.
    None,
    /// Resolve a locus reference.
    Ref(MindRef),
}

/// One compiled GOAP operator.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct MindOperator {
    /// Program-local identity.
    pub id: Name,
    /// Facts that must be true.
    pub requires: Vec<Name>,
    /// Abstract facts made true after the action.
    pub sets: Vec<Name>,
    /// Abstract facts made false after the action.
    pub clears: Vec<Name>,
    /// Non-zero integer planning cost.
    pub cost: u16,
    /// Runtime verb emitted for the first plan step.
    pub verb: Verb,
    /// Runtime target.
    pub target: MindTarget,
}

/// One utility-ranked desired fact set.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct MindGoal {
    /// Author-facing goal id; no Rust dispatch is attached to it.
    pub id: Name,
    /// Facts which define completion.
    pub desired: Vec<Name>,
    /// Integer utility. Higher wins; declaration order breaks ties.
    pub utility: u16,
}

/// One direct table row for Far LOD; Far never runs GOAP search.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct FarRule {
    /// Facts read by the row.
    pub requires: Vec<Name>,
    /// Facts the action may affect; each must be `far_safe`.
    pub effects: Vec<Name>,
    /// Emitted verb.
    pub verb: Verb,
    /// Emitted target.
    pub target: MindTarget,
    /// Integer utility used for deterministic row selection.
    pub utility: u16,
}

/// Normalized, hash-interned behavior and encounter policy.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MindProgram {
    /// Optional Beat identity for encounter-direction audit/evidence.
    pub beat: Option<Name>,
    /// Visible facts.
    pub facts: Vec<MindFact>,
    /// Full-LOD GOAP operators.
    pub operators: Vec<MindOperator>,
    /// Full-LOD candidate goals.
    pub goals: Vec<MindGoal>,
    /// Far-LOD direct policy table.
    pub far: Vec<FarRule>,
}

impl MindProgram {
    fn check(&self, locus: &Name) -> Result<(), IrError> {
        if self.facts.len() > MAX_MIND_FACTS {
            return Err(cap(locus, "facts", self.facts.len(), MAX_MIND_FACTS));
        }
        if self.operators.len() > MAX_MIND_OPERATORS {
            return Err(cap(
                locus,
                "operators",
                self.operators.len(),
                MAX_MIND_OPERATORS,
            ));
        }
        if self.goals.len() > MAX_MIND_GOALS {
            return Err(cap(locus, "goals", self.goals.len(), MAX_MIND_GOALS));
        }
        if self.beat.as_ref().is_some_and(|n| n.check().is_err()) {
            return Err(IrError::EmptyName);
        }
        let mut facts = BTreeSet::new();
        for fact in &self.facts {
            fact.id.check()?;
            fact.query.check()?;
            if fact.far_safe
                && !matches!(
                    fact.query,
                    MindQuery::Never | MindQuery::Always | MindQuery::TickModulo { .. }
                )
            {
                return Err(IrError::UnsafeFarFact {
                    locus: locus.0.clone(),
                    fact: fact.id.0.clone(),
                });
            }
            if !facts.insert(fact.id.clone()) {
                return Err(invalid(locus, "duplicate fact", &fact.id));
            }
        }
        let mut operators = BTreeSet::new();
        for op in &self.operators {
            op.id.check()?;
            if op.cost == 0 || op.verb == Verb::Time || !operators.insert(op.id.clone()) {
                return Err(invalid(locus, "invalid operator", &op.id));
            }
            check_refs(
                locus,
                &facts,
                op.requires.iter().chain(&op.sets).chain(&op.clears),
            )?;
            check_target(&op.target)?;
        }
        let mut goals = BTreeSet::new();
        for goal in &self.goals {
            goal.id.check()?;
            if goal.desired.is_empty() || !goals.insert(goal.id.clone()) {
                return Err(invalid(locus, "invalid goal", &goal.id));
            }
            check_refs(locus, &facts, goal.desired.iter())?;
        }
        let mut far_inputs = BTreeSet::new();
        for row in &self.far {
            check_refs(locus, &facts, row.requires.iter().chain(&row.effects))?;
            check_target(&row.target)?;
            if !matches!(row.verb, Verb::Look | Verb::Move | Verb::Investigate)
                || !matches!(row.target, MindTarget::None)
            {
                return Err(IrError::InvalidMindProgram(format!(
                    "{}: Far action must be un-targeted Look, Move, or Investigate",
                    locus.as_str()
                )));
            }
            for name in row.requires.iter().chain(&row.effects) {
                far_inputs.insert(name.clone());
                if !self.facts.iter().any(|f| &f.id == name && f.far_safe) {
                    return Err(IrError::UnsafeFarFact {
                        locus: locus.0.clone(),
                        fact: name.0.clone(),
                    });
                }
            }
        }
        if far_inputs.len() > MAX_FAR_INPUTS {
            return Err(cap(locus, "far inputs", far_inputs.len(), MAX_FAR_INPUTS));
        }
        Ok(())
    }
}

fn check_target(target: &MindTarget) -> Result<(), IrError> {
    match target {
        MindTarget::None => Ok(()),
        MindTarget::Ref(r) => r.check(),
    }
}

fn check_refs<'a>(
    locus: &Name,
    facts: &BTreeSet<Name>,
    names: impl Iterator<Item = &'a Name>,
) -> Result<(), IrError> {
    for name in names {
        name.check()?;
        if !facts.contains(name) {
            return Err(invalid(locus, "unknown fact", name));
        }
    }
    Ok(())
}

fn invalid(locus: &Name, reason: &str, item: &Name) -> IrError {
    IrError::InvalidMindProgram(format!("{}: {reason} {}", locus.as_str(), item.as_str()))
}

fn cap(locus: &Name, resource: &str, actual: usize, cap: usize) -> IrError {
    IrError::MindProgramCap {
        locus: locus.0.clone(),
        resource: resource.into(),
        actual,
        cap,
    }
}

/// Authoring: which locus uses a compiled program and its dialogue templates.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MindSpec {
    /// Seed name of the actor.
    pub locus: Name,
    /// Authored GOAP/utility/Far tables. Labels carry no runtime dispatch.
    pub program: MindProgram,
    /// Slot templates (`"{name} won't sell that."`). Infer may fill slots, not facts.
    pub templates: Vec<String>,
}

impl MindSpec {
    /// Validate K90 caps, local references, and the conservative Far boundary.
    pub fn validate(&self) -> Result<(), IrError> {
        self.check()
    }

    pub(crate) fn check(&self) -> Result<(), IrError> {
        self.locus.check()?;
        self.program.check(&self.locus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(id: &str, far_safe: bool) -> MindFact {
        MindFact {
            id: Name::from(id),
            query: MindQuery::Never,
            far_safe,
        }
    }

    #[test]
    fn over_cap_program_has_actor_anchored_diagnostic() {
        let spec = MindSpec {
            locus: Name::from("chorus_guard"),
            program: MindProgram {
                facts: (0..=MAX_MIND_FACTS)
                    .map(|i| fact(&format!("fact_{i}"), false))
                    .collect(),
                ..MindProgram::default()
            },
            templates: Vec::new(),
        };
        let error = spec.check().unwrap_err();
        assert!(matches!(
            error,
            IrError::MindProgramCap { ref locus, ref resource, actual: 65, cap: 64 }
                if locus == "chorus_guard" && resource == "facts"
        ));
        assert_ne!(error.to_diagnostic().primary, crate::AnchorId::ZERO);
    }

    #[test]
    fn protected_far_fact_fails_closed() {
        let protected = Name::from("ownership");
        let spec = MindSpec {
            locus: Name::from("guard"),
            program: MindProgram {
                facts: vec![MindFact {
                    id: protected.clone(),
                    query: MindQuery::Related {
                        a: MindRef::This,
                        rel: Rel::OwnedBy,
                        b: MindRef::This,
                    },
                    far_safe: true,
                }],
                far: vec![FarRule {
                    requires: vec![protected.clone()],
                    effects: vec![protected],
                    verb: Verb::Investigate,
                    target: MindTarget::None,
                    utility: 1,
                }],
                ..MindProgram::default()
            },
            templates: Vec::new(),
        };
        assert!(matches!(spec.check(), Err(IrError::UnsafeFarFact { .. })));
    }
}
