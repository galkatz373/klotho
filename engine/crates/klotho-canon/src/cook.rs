//! Cook Canon diffs + seed into packed tables.

use std::collections::BTreeMap;

use klotho_core::{AffordanceId, LawId, LocusKind, Sigil};
use klotho_ir::{
    Affordance, Beat, CanonDiff, IntentDoc, Law, LawBody, MindProgram, MindQuery, MindRef,
    MindSpec, MindTarget, Name, RiteGraph, RiteNode, RiteOp, SeedFact,
};

use crate::ast::{PredId, PredProgram, RiteChunk, RiteId, RiteInstr};
use crate::cfg::check_rite_cfg;
use crate::compile::{Interner, compile_pred_with};
use crate::contradict::check_fragment;
use crate::error::CookError;
use crate::tables::{
    Canon, CookedAffordance, CookedBeat, CookedCost, CookedLaw, CookedLawBody, CookedRite,
};

/// Cook an authoring document. Seed loci become pins; unbound `Name` slots fail.
pub fn cook(doc: &IntentDoc) -> Result<Canon, CookError> {
    doc.validate()
        .map_err(|e| CookError::InvalidDoc(e.to_string()))?;
    cook_inner(&doc.canon_diffs, &doc.seed, &doc.minds)
}

/// Cook diffs without seed. `Slot::Name` pins intern without Sigils.
pub fn cook_diffs(diffs: &[CanonDiff]) -> Result<Canon, CookError> {
    cook_inner(diffs, &[], &[])
}

fn cook_inner(
    diffs: &[CanonDiff],
    seed: &[SeedFact],
    minds: &[MindSpec],
) -> Result<Canon, CookError> {
    let draft = Draft::apply(diffs)?;
    let has_lockable = draft
        .affordances
        .iter()
        .any(|(n, _)| n.as_str() == "Lockable");
    check_fragment(&draft.laws, has_lockable)?;

    let mut intern = Interner::new();
    for (i, (n, _)) in draft.affordances.iter().enumerate() {
        intern
            .affordances
            .insert(n.clone(), AffordanceId(u16_len(i)?));
    }
    for (i, g) in draft.rites.iter().enumerate() {
        intern.rites.insert(g.id.clone(), RiteId(u16_len(i)?));
    }

    let mut next_sigil: u128 = 0;
    let mut seen_locus = BTreeMap::new();
    for s in seed {
        if let SeedFact::Locus { name, kind } = s {
            if seen_locus.insert(name.clone(), *kind).is_some() {
                return Err(CookError::DuplicateId(name.0.clone()));
            }
            let i = u16_len(intern.pin_names.len())?;
            intern.pins.insert(name.clone(), i);
            intern.pin_names.push(name.clone());
            intern
                .pin_sigils
                .push(Some(alloc_sigil(*kind, &mut next_sigil)?));
        }
    }
    intern.strict_pins = !seed.is_empty();
    if intern.strict_pins {
        for s in seed {
            match s {
                SeedFact::Rel { a, b, .. } | SeedFact::Qty { of: a, res: b, .. } => {
                    require_pin(&intern, a)?;
                    if matches!(s, SeedFact::Rel { .. }) {
                        require_pin(&intern, b)?;
                    }
                    if let SeedFact::Qty { res, .. } = s {
                        intern.intern_resource(res)?;
                    }
                }
                SeedFact::Pose { of, .. }
                | SeedFact::Physics { of, .. }
                | SeedFact::ContactTrack { of, .. } => require_pin(&intern, of)?,
                SeedFact::Locus { .. } => {}
            }
        }
    }
    for mind in minds {
        require_pin(&intern, &mind.locus)?;
        cook_mind_names(&mind.program, &mut intern)?;
    }

    let mut preds: Vec<PredProgram> = Vec::new();
    let mut laws = Vec::new();
    let mut law_by_name = BTreeMap::new();
    for (i, (name, law)) in draft.laws.iter().enumerate() {
        let id = LawId(u16_len(i)?);
        law_by_name.insert(name.clone(), id);
        let when = push_pred(&mut preds, compile_pred_with(&law.when, &mut intern)?)?;
        let body = cook_body(&law.body, &mut intern, &mut preds)?;
        laws.push(CookedLaw {
            id,
            name: name.clone(),
            when,
            body,
        });
    }

    let mut affordances = Vec::new();
    for (i, (name, a)) in draft.affordances.iter().enumerate() {
        let id = AffordanceId(u16_len(i)?);
        let mut requires = Vec::new();
        for p in &a.requires {
            requires.push(push_pred(&mut preds, compile_pred_with(p, &mut intern)?)?);
        }
        let mut conflicts = Vec::new();
        for c in &a.conflicts {
            conflicts.push(intern.intern_affordance(c)?);
        }
        affordances.push(CookedAffordance {
            id,
            name: name.clone(),
            requires,
            grants: a.grants.clone(),
            conflicts,
        });
    }

    let mut rites = Vec::new();
    for (i, g) in draft.rites.iter().enumerate() {
        rites.push(cook_rite(RiteId(u16_len(i)?), g, &mut intern, &mut preds)?);
    }

    // Implicit affordance rows (e.g. Opaque used in OpaqueClosed desugar).
    let declared: BTreeMap<_, _> = affordances.iter().map(|a| (a.name.clone(), a.id)).collect();
    let extra: Vec<(Name, AffordanceId)> = intern
        .affordances
        .iter()
        .filter(|(n, _)| !declared.contains_key(*n))
        .map(|(n, id)| (n.clone(), *id))
        .collect();
    for (name, id) in extra {
        affordances.push(CookedAffordance {
            id,
            name,
            requires: Vec::new(),
            grants: Vec::new(),
            conflicts: Vec::new(),
        });
    }
    affordances.sort_by_key(|a| a.id);
    debug_assert!(
        affordances
            .iter()
            .enumerate()
            .all(|(i, a)| a.id.0 as usize == i)
    );

    let beats = draft
        .beats
        .into_iter()
        .map(|b| CookedBeat {
            id: b.id,
            notes: b.notes,
        })
        .collect();

    let mut resource_items: Vec<_> = intern
        .resources
        .iter()
        .map(|(n, id)| (id.0, n.clone()))
        .collect();
    resource_items.sort_by_key(|(id, _)| *id);
    let resources: Vec<Name> = resource_items.into_iter().map(|(_, n)| n).collect();
    let resource_by_name = intern.resources.clone();

    let mut fact_items: Vec<_> = intern
        .facts
        .iter()
        .map(|(n, id)| (*id, n.clone()))
        .collect();
    fact_items.sort_by_key(|(id, _)| *id);
    let facts: Vec<Name> = fact_items.into_iter().map(|(_, n)| n).collect();

    let affordance_by_name = intern.affordances.clone();
    let rite_by_name = intern.rites.clone();

    let mut canon = Canon::from_parts(
        laws,
        affordances,
        rites,
        beats,
        preds,
        resources,
        facts,
        intern.pin_names,
        intern.pin_sigils,
        law_by_name,
        affordance_by_name,
        rite_by_name,
        resource_by_name,
    );
    for fact in seed {
        if let SeedFact::Physics { of, body } = fact {
            let locus = canon
                .pin(of.as_str())
                .ok_or_else(|| CookError::UnboundName(of.0.clone()))?;
            if body.character.is_some() && locus.kind() != Some(LocusKind::Actor) {
                return Err(CookError::InvalidDoc(format!(
                    "character physics requires an Actor: {of}"
                )));
            }
            if !canon.bind_physics(locus, *body) {
                return Err(CookError::DuplicateId(of.0.clone()));
            }
        }
    }
    for fact in seed {
        if let SeedFact::ContactTrack { of, track } = fact {
            let actor = canon
                .pin(of.as_str())
                .ok_or_else(|| CookError::UnboundName(of.0.clone()))?;
            let rite = canon.rites.iter().find(|r| r.name.as_str() == track.rite);
            if actor.kind() != Some(LocusKind::Actor)
                || !track.is_valid()
                || !rite.is_some_and(|r| {
                    r.chunk.instrs.iter().any(|i| {
                        i.pc == track.wait_pc
                            && matches!(i.op, RiteOp::Wait(t, Some(c)) if t == track.wait_ticks && c.as_u8() == track.channel)
                    })
                })
            {
                return Err(CookError::InvalidDoc(format!(
                    "invalid contact binding for {of}"
                )));
            }
            if canon.contact_tracks.insert(actor, track.clone()).is_some() {
                return Err(CookError::DuplicateId(of.0.clone()));
            }
        }
    }
    Ok(canon)
}

fn cook_mind_names(program: &MindProgram, intern: &mut Interner) -> Result<(), CookError> {
    for fact in &program.facts {
        match &fact.query {
            MindQuery::Related { a, b, .. } | MindQuery::Near { a, b, .. } => {
                cook_mind_ref(a, intern)?;
                cook_mind_ref(b, intern)?;
            }
            MindQuery::QtyAtLeast { of, res, .. } => {
                cook_mind_ref(of, intern)?;
                intern.intern_resource(res)?;
            }
            MindQuery::AnyQtyAtLeast { res, .. } => {
                intern.intern_resource(res)?;
            }
            MindQuery::Never | MindQuery::Always | MindQuery::TickModulo { .. } => {}
        }
    }
    for target in program
        .operators
        .iter()
        .map(|op| &op.target)
        .chain(program.far.iter().map(|row| &row.target))
    {
        if let MindTarget::Ref(reference) = target {
            cook_mind_ref(reference, intern)?;
        }
    }
    Ok(())
}

fn cook_mind_ref(reference: &MindRef, intern: &Interner) -> Result<(), CookError> {
    if let MindRef::Pin(name) = reference {
        require_pin(intern, name)?;
    }
    Ok(())
}

fn require_pin(intern: &Interner, n: &Name) -> Result<(), CookError> {
    if intern.pins.contains_key(n) {
        Ok(())
    } else {
        Err(CookError::UnboundName(n.0.clone()))
    }
}

fn alloc_sigil(kind: LocusKind, next: &mut u128) -> Result<Sigil, CookError> {
    let s = Sigil::pack(kind, 0, *next).ok_or(CookError::TableFull)?;
    *next += 1;
    Ok(s)
}

fn u16_len(n: usize) -> Result<u16, CookError> {
    u16::try_from(n).map_err(|_| CookError::TableFull)
}

fn push_pred(preds: &mut Vec<PredProgram>, p: PredProgram) -> Result<PredId, CookError> {
    let id = PredId(u16_len(preds.len())?);
    preds.push(p);
    Ok(id)
}

fn cook_body(
    body: &LawBody,
    intern: &mut Interner,
    preds: &mut Vec<PredProgram>,
) -> Result<CookedLawBody, CookError> {
    match body {
        LawBody::Pred { must, ought } => Ok(CookedLawBody::Pred {
            must: push_pred(preds, compile_pred_with(must, intern)?)?,
            ought: match ought {
                Some(c) => Some(CookedCost {
                    res: intern.intern_resource(&c.res)?,
                    amount: c.amount,
                }),
                None => None,
            },
        }),
        LawBody::Ramp {
            res,
            per_tick,
            quantum,
            cap,
        } => Ok(CookedLawBody::Ramp {
            res: intern.intern_resource(res)?,
            per_tick: *per_tick,
            quantum: *quantum,
            cap: *cap,
        }),
        LawBody::Spread {
            res,
            per_tick,
            near,
            cap_global,
            ignite_at,
        } => Ok(CookedLawBody::Spread {
            res: intern.intern_resource(res)?,
            per_tick: *per_tick,
            near: *near,
            cap_global: *cap_global,
            ignite_at: *ignite_at,
        }),
        LawBody::Conserve { res, over } => Ok(CookedLawBody::Conserve {
            res: intern.intern_resource(res)?,
            over: *over,
        }),
        LawBody::Cap {
            mark,
            n,
            require_rel,
        } => Ok(CookedLawBody::Cap {
            mark: push_pred(preds, compile_pred_with(mark, intern)?)?,
            n: *n,
            require_rel: match require_rel {
                Some((r, s)) => Some((*r, intern.cook_slot(s)?)),
                None => None,
            },
        }),
    }
}

fn cook_rite(
    id: RiteId,
    graph: &RiteGraph,
    intern: &mut Interner,
    preds: &mut Vec<PredProgram>,
) -> Result<CookedRite, CookError> {
    let _ops = check_rite_cfg(graph)?;
    let mut instrs = Vec::new();
    let mut guards = BTreeMap::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        let (pc, op) = match node {
            RiteNode::Op(op) => (i as u16, op),
            RiteNode::Labeled { pc, op } => (*pc, op),
        };
        match op {
            RiteOp::Guard(pred, _) | RiteOp::Branch(pred, _, _) => {
                guards.insert(pc, push_pred(preds, compile_pred_with(pred, intern)?)?);
            }
            RiteOp::Spend(res, _, _) | RiteOp::Setq(_, res, _) => {
                intern.intern_resource(res)?;
            }
            RiteOp::Spawn(n) => {
                intern.intern_fact(n)?;
            }
            _ => {}
        }
        instrs.push(RiteInstr { pc, op: op.clone() });
    }
    Ok(CookedRite {
        id,
        name: graph.id.clone(),
        chunk: RiteChunk {
            entry: graph.entry,
            cap_steps: graph.cap_steps,
            cap_ticks: graph.cap_ticks,
            instrs,
        },
        guards,
    })
}

struct Draft {
    laws: Vec<(Name, Law)>,
    law_ix: BTreeMap<Name, usize>,
    affordances: Vec<(Name, Affordance)>,
    aff_ix: BTreeMap<Name, usize>,
    rites: Vec<RiteGraph>,
    rite_ix: BTreeMap<Name, usize>,
    beats: Vec<Beat>,
    beat_ix: BTreeMap<Name, usize>,
}

impl Draft {
    fn apply(diffs: &[CanonDiff]) -> Result<Self, CookError> {
        let mut d = Self {
            laws: Vec::new(),
            law_ix: BTreeMap::new(),
            affordances: Vec::new(),
            aff_ix: BTreeMap::new(),
            rites: Vec::new(),
            rite_ix: BTreeMap::new(),
            beats: Vec::new(),
            beat_ix: BTreeMap::new(),
        };
        for diff in diffs {
            match diff {
                CanonDiff::AddLaw(l) => {
                    if d.law_ix.contains_key(&l.id) {
                        return Err(CookError::DuplicateId(l.id.0.clone()));
                    }
                    d.law_ix.insert(l.id.clone(), d.laws.len());
                    d.laws.push((l.id.clone(), l.clone()));
                }
                CanonDiff::RetractLaw { id, .. } => d.retract(id)?,
                CanonDiff::AddAffordance(a) => {
                    if d.aff_ix.contains_key(&a.id) {
                        return Err(CookError::DuplicateId(a.id.0.clone()));
                    }
                    d.aff_ix.insert(a.id.clone(), d.affordances.len());
                    d.affordances.push((a.id.clone(), a.clone()));
                }
                CanonDiff::AddRite(g) => {
                    if d.rite_ix.contains_key(&g.id) {
                        return Err(CookError::DuplicateId(g.id.0.clone()));
                    }
                    d.rite_ix.insert(g.id.clone(), d.rites.len());
                    d.rites.push(g.clone());
                }
                CanonDiff::RetractRite { id, .. } => d.retract_rite(id)?,
                CanonDiff::AddBeat(b) => {
                    if d.beat_ix.contains_key(&b.id) {
                        return Err(CookError::DuplicateId(b.id.0.clone()));
                    }
                    d.beat_ix.insert(b.id.clone(), d.beats.len());
                    d.beats.push(b.clone());
                }
            }
        }
        Ok(d)
    }

    fn retract(&mut self, id: &Name) -> Result<(), CookError> {
        let Some(i) = self.law_ix.remove(id) else {
            return Err(CookError::UnknownRetract(id.0.clone()));
        };
        self.laws.remove(i);
        self.law_ix.clear();
        for (j, (n, _)) in self.laws.iter().enumerate() {
            self.law_ix.insert(n.clone(), j);
        }
        Ok(())
    }

    fn retract_rite(&mut self, id: &Name) -> Result<(), CookError> {
        let Some(i) = self.rite_ix.remove(id) else {
            return Err(CookError::UnknownRetract(id.0.clone()));
        };
        self.rites.remove(i);
        self.rite_ix.clear();
        for (j, rite) in self.rites.iter().enumerate() {
            self.rite_ix.insert(rite.id.clone(), j);
        }
        Ok(())
    }
}
