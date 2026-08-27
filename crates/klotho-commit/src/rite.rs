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
    rite_steps: &mut u32,
    pred_ops: &mut u32,
    tick: Tick,
) -> Result<Burst, RejectReason> {
    let cap = u32::from(rite.chunk.cap_steps).min(*rite_steps);
    let mut steps: u32 = 0;
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
    pred_ops: &mut u32,
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

/// Resume: a `WAIT.channel` may only be advanced by a Player. Mind/Infer →
/// `UnclaimedAgency` (K10). A Player with an empty claim is allowed through so
/// the rite Guard can take the refuse / miss path (`RelDel` + Fail).
pub fn check_wait_agency(
    machine: &RiteMachine,
    source: SourceKind,
    _claimed: &[Channel],
) -> Result<(), RejectReason> {
    if machine.wait_ch.is_some() && source != SourceKind::Player {
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
    rite_steps: &mut u32,
    pred_ops: &mut u32,
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
    let Some(rite) = pick_start_rite(canon, spec, verb, target) else {
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
    if verb == Verb::Fire {
        drive_apply_hit(
            spec, canon, target, verb, source, claimed, rite_steps, pred_ops, tick,
        )?;
    }
    Ok(())
}

fn named_rite<'a>(canon: &'a Canon, id: &str) -> Option<&'a CookedRite> {
    canon.rites.iter().find(|r| r.name.as_str() == id)
}

fn pick_start_rite<'a>(
    canon: &'a Canon,
    spec: &SpecDelta,
    verb: Verb,
    target: Option<Sigil>,
) -> Option<&'a CookedRite> {
    let fallback = || canon.rites.first();
    match verb {
        Verb::Carry => named_rite(canon, "carry.pick").or_else(fallback),
        Verb::Talk => named_rite(canon, "trade.offer").or_else(fallback),
        Verb::Pay => named_rite(canon, "trade.pay"),
        Verb::Fire => named_rite(canon, "fire").or_else(fallback),
        Verb::Use | Verb::Open => pick_use_rite(canon, spec, target).or_else(fallback),
        _ => None,
    }
}

fn pick_use_rite<'a>(
    canon: &'a Canon,
    spec: &SpecDelta,
    target: Option<Sigil>,
) -> Option<&'a CookedRite> {
    let view = spec.view();
    if let Some(t) = target {
        if let Some(id) = canon.affordance_id("Lockable") {
            if view.has_affordance(t, id) {
                return named_rite(canon, "lockpick");
            }
        }
        if let Some(id) = canon.affordance_id("Flammable") {
            if view.has_affordance(t, id) {
                if burning(&view, canon, t) {
                    return named_rite(canon, "douse");
                }
                return named_rite(canon, "ignite");
            }
        }
    }
    named_rite(canon, "lockpick").or_else(|| named_rite(canon, "ignite"))
}

fn burning(view: &klotho_world::WorldView<'_>, canon: &Canon, s: Sigil) -> bool {
    let Some(heat) = canon.resource_id("heat") else {
        return false;
    };
    view.qty(s, heat) >= klotho_canon::IGNITE
}

fn drive_apply_hit(
    spec: &mut SpecDelta,
    canon: &Canon,
    target: Option<Sigil>,
    verb: Verb,
    source: SourceKind,
    claimed: &[Channel],
    rite_steps: &mut u32,
    pred_ops: &mut u32,
    tick: Tick,
) -> Result<(), RejectReason> {
    let Some(victim) = target else {
        return Ok(());
    };
    if !spec
        .events()
        .iter()
        .any(|e| matches!(e.body, TraceBody::Emitted { .. }))
    {
        return Ok(());
    }
    let Some(hit) = named_rite(canon, "apply_hit") else {
        return Ok(());
    };
    spec.push(TraceEvent::new(
        tick,
        TraceBody::RiteBegan {
            actor: victim,
            rite: hit.id.0,
            target: Some(victim),
        },
    ));
    let _ = run_burst(
        spec,
        canon,
        hit,
        victim,
        Some(victim),
        verb,
        source,
        claimed,
        hit.chunk.entry,
        rite_steps,
        pred_ops,
        tick,
    )?;
    Ok(())
}
