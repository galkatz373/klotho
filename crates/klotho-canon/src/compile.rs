//! Pred compiler: desugar, intern packed ids, emit [`PredProgram`].

use std::collections::BTreeMap;

use klotho_core::{AffordanceId, ResourceId, Sigil};
use klotho_ir::{Name, Pred, Rel, Slot};

use crate::ast::{
    Atom, CookedSlot, HEAT, IGNITE, OPAQUE, PRED_OPS_PER_EVAL, PredChunk, PredOp, PredProgram,
    RelatedScan, RiteId,
};
use crate::error::CookError;

/// Shared intern tables for one cook (or one standalone [`compile_pred`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct Interner {
    pub affordances: BTreeMap<Name, AffordanceId>,
    pub resources: BTreeMap<Name, ResourceId>,
    pub rites: BTreeMap<Name, RiteId>,
    pub facts: BTreeMap<Name, u16>,
    pub pins: BTreeMap<Name, u16>,
    pub pin_names: Vec<Name>,
    pub pin_sigils: Vec<Option<Sigil>>,
    /// When set, unseen `Slot::Name` is [`CookError::UnboundName`].
    pub strict_pins: bool,
}

impl Interner {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn intern_affordance(&mut self, n: &Name) -> Result<AffordanceId, CookError> {
        if let Some(&id) = self.affordances.get(n) {
            return Ok(id);
        }
        let id = AffordanceId(u16_id(self.affordances.len())?);
        self.affordances.insert(n.clone(), id);
        Ok(id)
    }

    pub(crate) fn intern_resource(&mut self, n: &Name) -> Result<ResourceId, CookError> {
        if let Some(&id) = self.resources.get(n) {
            return Ok(id);
        }
        let id = ResourceId(u8_id(self.resources.len())?);
        self.resources.insert(n.clone(), id);
        Ok(id)
    }

    pub(crate) fn intern_rite(&mut self, n: &Name) -> Result<RiteId, CookError> {
        if let Some(&id) = self.rites.get(n) {
            return Ok(id);
        }
        let id = RiteId(u16_id(self.rites.len())?);
        self.rites.insert(n.clone(), id);
        Ok(id)
    }

    pub(crate) fn intern_fact(&mut self, n: &Name) -> Result<u16, CookError> {
        if let Some(&id) = self.facts.get(n) {
            return Ok(id);
        }
        let id = u16_id(self.facts.len())?;
        self.facts.insert(n.clone(), id);
        Ok(id)
    }

    pub(crate) fn cook_slot(&mut self, slot: &Slot) -> Result<CookedSlot, CookError> {
        match slot {
            Slot::This => Ok(CookedSlot::This),
            Slot::Target => Ok(CookedSlot::Target),
            Slot::Other => Ok(CookedSlot::Other),
            Slot::Name(n) => {
                if let Some(&i) = self.pins.get(n) {
                    return Ok(CookedSlot::Pin(i));
                }
                if self.strict_pins {
                    return Err(CookError::UnboundName(n.0.clone()));
                }
                let i = u16_id(self.pin_names.len())?;
                self.pins.insert(n.clone(), i);
                self.pin_names.push(n.clone());
                self.pin_sigils.push(None);
                Ok(CookedSlot::Pin(i))
            }
        }
    }
}

fn u16_id(n: usize) -> Result<u16, CookError> {
    u16::try_from(n).map_err(|_| CookError::TableFull)
}

fn u8_id(n: usize) -> Result<u8, CookError> {
    u8::try_from(n).map_err(|_| CookError::TableFull)
}

/// Desugar authoring sugar. `Possessed` is not in the IR (authors write `Rel`).
pub(crate) fn desugar(pred: &Pred) -> Pred {
    match pred {
        Pred::Burning(s) => Pred::Qty(s.clone(), Name::from(HEAT), klotho_ir::Cmp::Ge, IGNITE),
        Pred::OpaqueClosed(s) => Pred::And(
            Box::new(Pred::Affordance(s.clone(), Name::from(OPAQUE))),
            Box::new(Pred::ExistsRelated {
                of: s.clone(),
                rel: Rel::LockedBy,
                pred: Box::new(Pred::OtherIs(Slot::Other)),
            }),
        ),
        Pred::And(a, b) => Pred::And(Box::new(desugar(a)), Box::new(desugar(b))),
        Pred::Or(a, b) => Pred::Or(Box::new(desugar(a)), Box::new(desugar(b))),
        Pred::Not(a) => Pred::Not(Box::new(desugar(a))),
        Pred::ExistsRelated { of, rel, pred } => Pred::ExistsRelated {
            of: of.clone(),
            rel: *rel,
            pred: Box::new(desugar(pred)),
        },
        Pred::CountRelated {
            of,
            rel,
            pred,
            cmp,
            n,
        } => Pred::CountRelated {
            of: of.clone(),
            rel: *rel,
            pred: Box::new(desugar(pred)),
            cmp: *cmp,
            n: *n,
        },
        other => other.clone(),
    }
}

/// Compile one predicate with a fresh intern (undeclared names are interned).
pub fn compile_pred(pred: &Pred) -> Result<PredProgram, CookError> {
    let mut intern = Interner::new();
    compile_pred_with(pred, &mut intern)
}

/// Compile using a shared intern (cook path).
pub(crate) fn compile_pred_with(
    pred: &Pred,
    intern: &mut Interner,
) -> Result<PredProgram, CookError> {
    let pred = desugar(pred);
    let mut atoms = Vec::new();
    let mut scans = Vec::new();
    let mut ops = Vec::new();
    emit(&pred, intern, &mut atoms, &mut scans, &mut ops)?;
    ops.push(PredOp::Halt);
    check_len(&ops)?;
    debug_assert_eq!(
        ops.iter()
            .filter(|o| matches!(o, PredOp::ExistsRelated | PredOp::CountRelated))
            .count(),
        scans.len()
    );
    Ok(PredProgram {
        chunk: PredChunk { ops },
        atoms,
        scans,
    })
}

fn check_len(ops: &[PredOp]) -> Result<(), CookError> {
    if ops.len() > PRED_OPS_PER_EVAL as usize {
        Err(CookError::PredTooLarge)
    } else {
        Ok(())
    }
}

fn emit(
    pred: &Pred,
    intern: &mut Interner,
    atoms: &mut Vec<Atom>,
    scans: &mut Vec<RelatedScan>,
    ops: &mut Vec<PredOp>,
) -> Result<(), CookError> {
    match pred {
        Pred::And(a, b) => {
            emit(a, intern, atoms, scans, ops)?;
            emit(b, intern, atoms, scans, ops)?;
            ops.push(PredOp::And);
        }
        Pred::Or(a, b) => {
            emit(a, intern, atoms, scans, ops)?;
            emit(b, intern, atoms, scans, ops)?;
            ops.push(PredOp::Or);
        }
        Pred::Not(a) => {
            emit(a, intern, atoms, scans, ops)?;
            ops.push(PredOp::Not);
        }
        Pred::ExistsRelated { of, rel, pred } => {
            let nested = emit_nested(pred, intern, atoms)?;
            scans.push(RelatedScan {
                of: intern.cook_slot(of)?,
                rel: *rel,
                pred: nested,
                count: None,
            });
            ops.push(PredOp::ExistsRelated);
        }
        Pred::CountRelated {
            of,
            rel,
            pred,
            cmp,
            n,
        } => {
            let nested = emit_nested(pred, intern, atoms)?;
            scans.push(RelatedScan {
                of: intern.cook_slot(of)?,
                rel: *rel,
                pred: nested,
                count: Some((*cmp, *n)),
            });
            ops.push(PredOp::CountRelated);
        }
        Pred::Burning(_) | Pred::OpaqueClosed(_) => {
            unreachable!("desugar removes sugar atoms");
        }
        atom => {
            let cooked = cook_atom(atom, intern)?;
            let i = intern_atom(cooked, atoms);
            ops.push(PredOp::PushAtom(i));
        }
    }
    Ok(())
}

fn emit_nested(
    pred: &Pred,
    intern: &mut Interner,
    atoms: &mut Vec<Atom>,
) -> Result<PredChunk, CookError> {
    let mut dummy_scans = Vec::new();
    let mut ops = Vec::new();
    emit(pred, intern, atoms, &mut dummy_scans, &mut ops)?;
    debug_assert!(
        dummy_scans.is_empty(),
        "nested quantifiers are rejected by klotho-ir"
    );
    ops.push(PredOp::Halt);
    check_len(&ops)?;
    Ok(PredChunk { ops })
}

fn intern_atom(atom: Atom, atoms: &mut Vec<Atom>) -> u16 {
    if let Some(i) = atoms.iter().position(|a| *a == atom) {
        return i as u16;
    }
    let i = atoms.len() as u16;
    atoms.push(atom);
    i
}

fn cook_atom(pred: &Pred, intern: &mut Interner) -> Result<Atom, CookError> {
    match pred {
        Pred::Affordance(s, n) => Ok(Atom::Affordance(
            intern.cook_slot(s)?,
            intern.intern_affordance(n)?,
        )),
        Pred::Rel(a, r, b) => Ok(Atom::Rel(intern.cook_slot(a)?, *r, intern.cook_slot(b)?)),
        Pred::Qty(s, n, cmp, v) => Ok(Atom::Qty(
            intern.cook_slot(s)?,
            intern.intern_resource(n)?,
            *cmp,
            *v,
        )),
        Pred::EqVerb(v) => Ok(Atom::EqVerb(*v)),
        Pred::RiteActive(n) => Ok(Atom::RiteActive(intern.intern_rite(n)?)),
        Pred::AabbNear(a, b, mm) => Ok(Atom::AabbNear(
            intern.cook_slot(a)?,
            intern.cook_slot(b)?,
            *mm,
        )),
        Pred::InWindow(n, ch) => Ok(Atom::InWindow(intern.intern_rite(n)?, *ch)),
        Pred::Knows(s, n) => Ok(Atom::Knows(intern.cook_slot(s)?, intern.intern_fact(n)?)),
        Pred::SourceIs(k) => Ok(Atom::SourceIs(*k)),
        Pred::AgencyClaimed(ch) => Ok(Atom::AgencyClaimed(*ch)),
        Pred::SweptHitsOpaqueClosed => Ok(Atom::SweptHitsOpaqueClosed),
        Pred::IslandAwake(s) => Ok(Atom::IslandAwake(intern.cook_slot(s)?)),
        Pred::SelfIs(s) => Ok(Atom::SelfIs(intern.cook_slot(s)?)),
        Pred::TargetIs(s) => Ok(Atom::TargetIs(intern.cook_slot(s)?)),
        Pred::OtherIs(s) => Ok(Atom::OtherIs(intern.cook_slot(s)?)),
        Pred::And(_, _)
        | Pred::Or(_, _)
        | Pred::Not(_)
        | Pred::ExistsRelated { .. }
        | Pred::CountRelated { .. }
        | Pred::Burning(_)
        | Pred::OpaqueClosed(_) => unreachable!("combinators / sugar are not atoms"),
    }
}

#[cfg(test)]
mod tests {
    use klotho_ir::{Cmp, Pred, Slot, Verb};

    use super::*;
    use crate::ast::PredOp;

    fn n(s: &str) -> Name {
        Name::from(s)
    }

    #[test]
    fn burning_desugars_to_heat_ge_ignite() {
        let d = desugar(&Pred::Burning(Slot::This));
        assert_eq!(d, Pred::Qty(Slot::This, n(HEAT), Cmp::Ge, IGNITE));
    }

    #[test]
    fn compile_and_emits_postfix() {
        let p = Pred::And(
            Box::new(Pred::EqVerb(Verb::Use)),
            Box::new(Pred::EqVerb(Verb::Open)),
        );
        let prog = compile_pred(&p).unwrap();
        assert_eq!(
            prog.chunk.ops,
            [
                PredOp::PushAtom(0),
                PredOp::PushAtom(1),
                PredOp::And,
                PredOp::Halt,
            ]
        );
        assert_eq!(prog.atoms.len(), 2);
    }

    #[test]
    fn pred_too_large_fails_cook() {
        let mut p = Pred::EqVerb(Verb::Use);
        for _ in 0..PRED_OPS_PER_EVAL {
            p = Pred::And(Box::new(p), Box::new(Pred::EqVerb(Verb::Use)));
        }
        assert_eq!(compile_pred(&p), Err(CookError::PredTooLarge));
    }
}
