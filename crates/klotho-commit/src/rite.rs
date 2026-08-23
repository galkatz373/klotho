//! Same-tick rite burst. `WAIT` yields and commits (K21).
#![allow(clippy::too_many_arguments)]

use klotho_canon::{Canon, CookedRite, EvalCtx, eval_pred};
use klotho_core::{RejectReason, ResourceId, Sigil, Tick};
use klotho_ir::{BindSrc, Channel, Rel, RiteOp, Slot, SourceKind, Status, Verb};
use klotho_trace::{RelTag, RiteEnd, TraceBody, TraceEvent};
use klotho_world::{RiteMachine, SpecDelta};

pub enum Burst {
    /// Halt/Complete ended the machine.
    Halt,
    /// WAIT: spec should commit; later tick resumes.
    Wait,
}

pub fn run_burst(
    spec: &mut SpecDelta,
    canon: &Canon,
    rite: &CookedRite,
    actor: Sigil,
    mut target: Option<Sigil>,
    verb: Verb,
    source: SourceKind,
    claimed: &[Channel],
    start_pc: u16,
    rite_steps: &mut u16,
    pred_ops: &mut u16,
    tick: Tick,
) -> Result<Burst, RejectReason> {
    let cap = rite.chunk.cap_steps.min(*rite_steps);
    let mut steps: u16 = 0;
    let mut pc = start_pc;
    let pins = canon.pin_sigils.as_slice();
    let view_pins = pins;
    loop {
        if steps >= cap || *rite_steps == 0 {
            spec.push(TraceEvent::new(
                tick,
                TraceBody::RiteEnded {
                    actor,
                    rite: rite.id.0,
                    status: RiteEnd::FailBudget,
                },
            ));
            return Ok(Burst::Halt);
        }
        *rite_steps -= 1;
        steps += 1;
        let instr = rite
            .chunk
            .instrs
            .iter()
            .find(|i| i.pc == pc)
            .ok_or(RejectReason::Budget)?;
        let next = next_pc(&rite.chunk.instrs, pc);
        match &instr.op {
            RiteOp::Halt(st) | RiteOp::Complete(st) => {
                spec.push(TraceEvent::new(
                    tick,
                    TraceBody::RiteEnded {
                        actor,
                        rite: rite.id.0,
                        status: status_end(*st),
                    },
                ));
                return Ok(Burst::Halt);
            }
            RiteOp::Guard(_, fail) => {
                let ok = eval_guard(
                    spec, canon, rite, pc, actor, target, verb, source, claimed, pred_ops,
                )?;
                pc = if ok {
                    next.ok_or(RejectReason::Budget)?
                } else {
                    *fail
                };
            }
            RiteOp::Spend(res, amount, fail) => {
                let rid = canon
                    .resource_id(res.as_str())
                    .ok_or(RejectReason::Resource(ResourceId(0)))?;
                let have = spec.view().qty(actor, rid);
                if have < *amount {
                    pc = *fail;
                } else {
                    spec.push(TraceEvent::new(
                        tick,
                        TraceBody::QtyChanged {
                            id: actor,
                            res: rid,
                            to: have - *amount,
                            quantum: 1,
                        },
                    ));
                    pc = next.ok_or(RejectReason::Budget)?;
                }
            }
            RiteOp::Wait(ticks, ch) => {
                let resume = next.unwrap_or(pc);
                spec.push(TraceEvent::new(
                    tick,
                    TraceBody::RiteAdvanced {
                        actor,
                        rite: rite.id.0,
                        pc: resume,
                        wait_left: *ticks,
                    },
                ));
                spec.put_rite(
                    actor,
                    rite.id,
                    RiteMachine {
                        pc: resume,
                        wait_left: *ticks,
                        target,
                        wait_ch: *ch,
                    },
                );
                return Ok(Burst::Wait);
            }
            RiteOp::Emit(_) => {
                spec.push(TraceEvent::new(
                    tick,
                    TraceBody::Emitted {
                        kind: 0,
                        a: actor,
                        b: target,
                    },
                ));
                pc = next.ok_or(RejectReason::Budget)?;
            }
            RiteOp::Branch(_, yes, no) => {
                let ok = eval_guard(
                    spec, canon, rite, pc, actor, target, verb, source, claimed, pred_ops,
                )?;
                pc = if ok { *yes } else { *no };
            }
            RiteOp::Bind(src) => {
                target = match src {
                    BindSrc::Target => target,
                    BindSrc::This => Some(actor),
                    BindSrc::Related(rel) => {
                        let mut n = Vec::new();
                        klotho_canon::PredStore::related(&spec.view(), actor, *rel, &mut n);
                        n.first().copied()
                    }
                };
                spec.put_rite(
                    actor,
                    rite.id,
                    RiteMachine {
                        pc: next.unwrap_or(pc),
                        wait_left: 0,
                        target,
                        wait_ch: None,
                    },
                );
                pc = next.ok_or(RejectReason::Budget)?;
            }
            RiteOp::Setq(slot, res, amount) => {
                let rid = canon
                    .resource_id(res.as_str())
                    .ok_or(RejectReason::Resource(ResourceId(0)))?;
                let s = resolve_slot(slot, actor, target, view_pins, canon)
                    .ok_or(RejectReason::Budget)?;
                spec.push(TraceEvent::new(
                    tick,
                    TraceBody::QtyChanged {
                        id: s,
                        res: rid,
                        to: *amount,
                        quantum: 1,
                    },
                ));
                pc = next.ok_or(RejectReason::Budget)?;
            }
            RiteOp::RelAdd(a, rel, b) => {
                let sa =
                    resolve_slot(a, actor, target, view_pins, canon).ok_or(RejectReason::Budget)?;
                let sb =
                    resolve_slot(b, actor, target, view_pins, canon).ok_or(RejectReason::Budget)?;
                spec.push(TraceEvent::new(
                    tick,
                    TraceBody::RelAdd {
                        a: sa,
                        rel: rel_tag(*rel),
                        b: sb,
                    },
                ));
                pc = next.ok_or(RejectReason::Budget)?;
            }
            RiteOp::RelDel(a, rel, b) => {
                let sa =
                    resolve_slot(a, actor, target, view_pins, canon).ok_or(RejectReason::Budget)?;
                let sb =
                    resolve_slot(b, actor, target, view_pins, canon).ok_or(RejectReason::Budget)?;
                spec.push(TraceEvent::new(
                    tick,
                    TraceBody::RelDel {
                        a: sa,
                        rel: rel_tag(*rel),
                        b: sb,
                    },
                ));
                pc = next.ok_or(RejectReason::Budget)?;
            }
            RiteOp::Awake(slot) => {
                if let Some(s) = resolve_slot(slot, actor, target, view_pins, canon) {
                    let island = spec.view().island(s).unwrap_or((0, 0)).0;
                    let _ = spec.set_island(s, island, 0);
                }
                pc = next.ok_or(RejectReason::Budget)?;
            }
        }
    }
}

fn eval_guard(
    spec: &SpecDelta,
    canon: &Canon,
    rite: &CookedRite,
    pc: u16,
    actor: Sigil,
    target: Option<Sigil>,
    verb: Verb,
    source: SourceKind,
    claimed: &[Channel],
    pred_ops: &mut u16,
) -> Result<bool, RejectReason> {
    let Some(&pid) = rite.guards.get(&pc) else {
        return Ok(true);
    };
    let prog = canon.pred(pid).ok_or(RejectReason::Budget)?;
    let view = spec.view();
    let ctx = EvalCtx {
        store: &view,
        this: actor,
        target,
        pins: canon.pin_sigils.as_slice(),
        verb,
        source,
        claimed,
        swept_hits_opaque_closed: false,
    };
    eval_pred(prog, &ctx, pred_ops)
}

fn next_pc(instrs: &[klotho_canon::RiteInstr], pc: u16) -> Option<u16> {
    let i = instrs.iter().position(|x| x.pc == pc)?;
    instrs.get(i + 1).map(|x| x.pc)
}

fn status_end(s: Status) -> RiteEnd {
    match s {
        Status::Success => RiteEnd::Success,
        Status::Fail => RiteEnd::Fail,
    }
}

fn resolve_slot(
    slot: &Slot,
    actor: Sigil,
    target: Option<Sigil>,
    pins: &[Option<Sigil>],
    canon: &Canon,
) -> Option<Sigil> {
    match slot {
        Slot::This => Some(actor),
        Slot::Target => target,
        Slot::Other => None,
        Slot::Name(n) => {
            let i = canon
                .pin_names
                .iter()
                .position(|p| p.as_str() == n.as_str())?;
            pins.get(i).copied().flatten()
        }
    }
}

fn rel_tag(r: Rel) -> RelTag {
    match r {
        Rel::In => RelTag::IN,
        Rel::OwnedBy => RelTag::OWNED_BY,
        Rel::WieldedBy => RelTag::WIELDED_BY,
        Rel::KeyedBy => RelTag::KEYED_BY,
        Rel::Knows => RelTag::KNOWS,
        Rel::Owes => RelTag::OWES,
        Rel::Fears => RelTag::FEARS,
        Rel::PartOf => RelTag::PART_OF,
        Rel::DerivedFrom => RelTag::DERIVED_FROM,
        Rel::LockedBy => RelTag::LOCKED_BY,
        Rel::Dead => RelTag::DEAD,
    }
}

/// Resume: WAIT.channel must be claimed by a Player. Others → `UnclaimedAgency`.
pub fn check_wait_agency(
    machine: &RiteMachine,
    source: SourceKind,
    claimed: &[Channel],
) -> Result<(), RejectReason> {
    let Some(ch) = machine.wait_ch else {
        return Ok(());
    };
    if source != SourceKind::Player || !claimed.contains(&ch) {
        return Err(RejectReason::UnclaimedAgency);
    }
    Ok(())
}

/// Start or resume a cooked rite into `spec`.
pub fn drive_rite(
    spec: &mut SpecDelta,
    canon: &Canon,
    actor: Sigil,
    target: Option<Sigil>,
    verb: Verb,
    source: SourceKind,
    claimed: &[Channel],
    rite_steps: &mut u16,
    pred_ops: &mut u16,
    tick: Tick,
) -> Result<(), RejectReason> {
    if let Some((rid, machine)) = spec.view().first_rite(actor) {
        check_wait_agency(&machine, source, claimed)?;
        let rite = canon
            .rites
            .iter()
            .find(|r| r.id == rid)
            .ok_or(RejectReason::Budget)?;
        let _ = run_burst(
            spec,
            canon,
            rite,
            actor,
            machine.target.or(target),
            verb,
            source,
            claimed,
            machine.pc,
            rite_steps,
            pred_ops,
            tick,
        )?;
        return Ok(());
    }
    // Use (and similar) starts the first cooked rite if any.
    if matches!(
        verb,
        Verb::Use | Verb::Carry | Verb::Pay | Verb::Fire | Verb::Open | Verb::Talk
    ) {
        let Some(rite) = canon.rites.first() else {
            return Ok(());
        };
        spec.push(TraceEvent::new(
            tick,
            TraceBody::RiteBegan {
                actor,
                rite: rite.id.0,
                target,
            },
        ));
        let _ = run_burst(
            spec,
            canon,
            rite,
            actor,
            target,
            verb,
            source,
            claimed,
            rite.chunk.entry,
            rite_steps,
            pred_ops,
            tick,
        )?;
    }
    Ok(())
}
