//! Same-tick rite burst. `WAIT` yields and commits (K21).
#![allow(clippy::too_many_arguments)]

use klotho_canon::{Canon, CookedRite, EvalCtx, PredStore, eval_pred};
use klotho_core::{LocusKind, PhysRequest, RejectReason, ResourceId, Sigil, Tick};
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
        // K21 budget split, signed off in docs/hld.md (§Rite ISA): pred-op
        // exhaustion rejects the whole proposal, but rite-step exhaustion
        // ends the burst — the delta admits together with RiteEnded
        // {FailBudget} as the atomic outcome (progress with its marker,
        // never an unmarked half-burst).
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
                if let Some(track) = canon
                    .contact_tracks
                    .get(&actor)
                    .filter(|t| t.rite == rite.name.as_str())
                {
                    if source != SourceKind::Player
                        || !claimed.iter().any(|c| c.as_u8() == track.channel)
                    {
                        return Err(RejectReason::UnclaimedAgency);
                    }
                    let machine = spec
                        .view()
                        .rite(actor, rite.id)
                        .ok_or(RejectReason::UnclaimedAgency)?;
                    if machine.contact_agency == 0 {
                        spec.push(TraceEvent::new(
                            tick,
                            TraceBody::MotionActionAuthorized {
                                actor,
                                rite: rite.id.0,
                                instance: machine.started_at,
                                channel: track.channel,
                            },
                        ));
                    }
                }
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
                if spec
                    .view()
                    .rite(actor, rite.id)
                    .is_some_and(|m| m.contact_hit && m.target != target)
                {
                    return Err(RejectReason::WitnessMismatch);
                }
                spec.put_rite(
                    actor,
                    rite.id,
                    RiteMachine {
                        contact_agency: spec
                            .view()
                            .rite(actor, rite.id)
                            .map_or(0, |m| m.contact_agency),
                        contact_hit: spec
                            .view()
                            .rite(actor, rite.id)
                            .is_some_and(|m| m.contact_hit),
                        started_at: spec
                            .view()
                            .rite(actor, rite.id)
                            .map_or(tick, |m| m.started_at),
                        wait_at: tick,
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
                if spec
                    .view()
                    .rite(actor, rite.id)
                    .is_some_and(|m| m.contact_hit && m.target != target)
                {
                    return Err(RejectReason::WitnessMismatch);
                }
                spec.put_rite(
                    actor,
                    rite.id,
                    RiteMachine {
                        contact_agency: spec
                            .view()
                            .rite(actor, rite.id)
                            .map_or(0, |m| m.contact_agency),
                        contact_hit: spec
                            .view()
                            .rite(actor, rite.id)
                            .is_some_and(|m| m.contact_hit),
                        started_at: spec
                            .view()
                            .rite(actor, rite.id)
                            .map_or(tick, |m| m.started_at),
                        wait_at: tick,
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
            RiteOp::Spawn(name) => {
                let template = canon
                    .facts
                    .iter()
                    .position(|n| n.as_str() == name.as_str())
                    .ok_or(RejectReason::Budget)? as u16;
                let sigil = alloc_spawn_sigil(spec)?;
                let at = spec.view().pose(actor).unwrap_or_default();
                spec.push(TraceEvent::new(
                    tick,
                    TraceBody::Spawned {
                        template,
                        sigil,
                        at,
                    },
                ));
                if !spec.view().contains(sigil) {
                    return Err(RejectReason::Budget);
                }
                if let Some(n) = canon.facts.get(template as usize) {
                    if let Some(aff) = canon.affordance_id(n.as_str()) {
                        spec.set_affordance(sigil, aff, true)
                            .map_err(|_| RejectReason::Budget)?;
                    }
                }
                pc = next.ok_or(RejectReason::Budget)?;
            }
            RiteOp::PhysReq { lin, ang } => {
                spec.set_phys_req(
                    actor,
                    PhysRequest {
                        lin: *lin,
                        ang: *ang,
                    },
                )
                .map_err(|_| RejectReason::Budget)?;
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
    RelTag(r.as_u8())
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
        if canon.contact_tracks.get(&actor).is_some_and(|track| {
            canon
                .rites
                .iter()
                .any(|r| r.id == rid && r.name.as_str() == track.rite)
        }) {
            // Only the coupled island may advance a contact-bound action.
            // Player may start it; player, Mind and Infer may not shortcut it.
            return Err(RejectReason::UnclaimedAgency);
        }
        check_wait_agency(&machine, source, claimed)?;
        let rite = canon
            .rites
            .iter()
            .find(|r| r.id == rid)
            .ok_or(RejectReason::Budget)?;
        let tgt = machine.target.or(target);
        let hit = rite.name.as_str() == "melee";
        let _ = run_burst(
            spec, canon, rite, actor, tgt, verb, source, claimed, machine.pc, rite_steps, pred_ops,
            tick,
        )?;
        if hit {
            drive_apply_hit(
                spec, canon, tgt, verb, source, claimed, rite_steps, pred_ops, tick,
            )?;
        }
        return Ok(());
    }
    let Some(rite) = pick_start_rite(canon, spec, verb, target) else {
        return Ok(());
    };
    if canon
        .contact_tracks
        .get(&actor)
        .is_some_and(|track| track.rite == rite.name.as_str())
        && canon
            .body_physics(actor)
            .and_then(|b| b.character)
            .is_none()
    {
        return Err(RejectReason::WrongHull);
    }
    spec.push(TraceEvent::new(
        tick,
        TraceBody::RiteBegan {
            actor,
            rite: rite.id.0,
            target,
        },
    ));
    let hit = verb == Verb::Fire
        || (rite.name.as_str() == "melee" && !canon.contact_tracks.contains_key(&actor));
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
    if hit {
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
        Verb::Steer => named_rite(canon, "steer"),
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
        if hittable(&view, canon, t) {
            if let Some(r) = named_rite(canon, "melee") {
                return Some(r);
            }
        }
        if let Some(id) = canon.affordance_id("Driveable") {
            if view.has_affordance(t, id) {
                return named_rite(canon, "possess");
            }
        }
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

fn hittable(view: &klotho_world::WorldView<'_>, canon: &Canon, t: Sigil) -> bool {
    let Some(id) = canon.affordance_id("Hittable") else {
        return false;
    };
    if view.has_affordance(t, id) {
        return true;
    }
    let mut n = Vec::new();
    PredStore::related(view, t, Rel::PartOf, &mut n);
    n.iter().any(|p| view.has_affordance(*p, id))
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
    let Some(raw) = target else {
        return Ok(());
    };
    if !spec
        .events()
        .iter()
        .any(|e| matches!(e.body, TraceBody::Emitted { .. }))
    {
        return Ok(());
    }
    let victim = part_of_parent(spec, raw).unwrap_or(raw);
    // Collapse once: PartOf is torn down in apply_hit, so sample it first.
    let collapse = may_collapse(spec, canon, victim);
    if let Some(hit) = named_rite(canon, "apply_hit") {
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
    }
    if collapse {
        drive_collapse(
            spec, canon, victim, verb, source, claimed, rite_steps, pred_ops, tick,
        )?;
    }
    Ok(())
}

fn drive_collapse(
    spec: &mut SpecDelta,
    canon: &Canon,
    victim: Sigil,
    verb: Verb,
    source: SourceKind,
    claimed: &[Channel],
    rite_steps: &mut u32,
    pred_ops: &mut u32,
    tick: Tick,
) -> Result<(), RejectReason> {
    let Some(mark) = canon.affordance_id("Destructible") else {
        return Ok(());
    };
    if !spec.view().has_affordance(victim, mark) {
        return Ok(());
    }
    let Some(res) = canon.resource_id("integrity") else {
        return Ok(());
    };
    if spec.view().qty(victim, res) > 0 {
        return Ok(());
    }
    let Some(collapse) = named_rite(canon, "collapse") else {
        return Ok(());
    };
    spec.push(TraceEvent::new(
        tick,
        TraceBody::RiteBegan {
            actor: victim,
            rite: collapse.id.0,
            target: Some(victim),
        },
    ));
    let _ = run_burst(
        spec,
        canon,
        collapse,
        victim,
        Some(victim),
        verb,
        source,
        claimed,
        collapse.chunk.entry,
        rite_steps,
        pred_ops,
        tick,
    )?;
    Ok(())
}

fn may_collapse(spec: &SpecDelta, canon: &Canon, victim: Sigil) -> bool {
    let Some(mark) = canon.affordance_id("Destructible") else {
        return false;
    };
    if !spec.view().has_affordance(victim, mark) {
        return false;
    }
    let mut n = Vec::new();
    PredStore::related(&spec.view(), victim, Rel::PartOf, &mut n);
    !n.is_empty()
}

fn part_of_parent(spec: &SpecDelta, s: Sigil) -> Option<Sigil> {
    let mut n = Vec::new();
    PredStore::related(&spec.view(), s, Rel::PartOf, &mut n);
    n.into_iter().find(|&p| p != s)
}

fn alloc_spawn_sigil(spec: &SpecDelta) -> Result<Sigil, RejectReason> {
    let mut next: u128 = 0;
    for s in spec.view().loci() {
        next = next.max(s.id().saturating_add(1));
    }
    Sigil::pack(LocusKind::Relic, 0, next).ok_or(RejectReason::Budget)
}

/// Advance contact-bound WAITs only from their actor's admitted island. The
/// initial profile consumes a one-target action at its first validated hit;
/// expiration ends it without running its hit tail.
pub(crate) fn admit_motion_windows(
    spec: &mut SpecDelta,
    canon: &Canon,
    bodies: &[crate::BodyDelta],
    claims: &[crate::MotionContact],
    rite_steps: &mut u32,
    pred_ops: &mut u32,
    tick: Tick,
) -> Result<(), RejectReason> {
    for body in bodies {
        let Some(track) = canon.contact_tracks.get(&body.mover) else {
            continue;
        };
        let Some(rite) = canon.rites.iter().find(|r| r.name.as_str() == track.rite) else {
            return Err(RejectReason::WitnessMismatch);
        };
        let Some(machine) = spec.view().rite(body.mover, rite.id) else {
            continue;
        };
        if machine.contact_agency != track.channel {
            return Err(RejectReason::UnclaimedAgency);
        }
        let index = rite
            .chunk
            .instrs
            .iter()
            .position(|i| i.pc == track.wait_pc)
            .ok_or(RejectReason::WitnessMismatch)?;
        let resume = rite
            .chunk
            .instrs
            .get(index + 1)
            .ok_or(RejectReason::WitnessMismatch)?
            .pc;
        if tick.0.saturating_sub(machine.wait_at.0) > u64::from(track.wait_ticks)
            && machine.pc == resume
        {
            spec.push(TraceEvent::new(
                tick,
                TraceBody::RiteEnded {
                    actor: body.mover,
                    rite: rite.id.0,
                    status: RiteEnd::Fail,
                },
            ));
            continue;
        }
        let elapsed = tick.0.saturating_sub(machine.wait_at.0);
        if machine.pc != resume {
            // Startup/recovery WAITs progress from their absolute clock. A
            // recovery tail may consume only previously admitted evidence.
            let pc_index = rite
                .chunk
                .instrs
                .iter()
                .position(|i| i.pc == machine.pc)
                .ok_or(RejectReason::WitnessMismatch)?;
            let previous = pc_index
                .checked_sub(1)
                .and_then(|i| rite.chunk.instrs.get(i))
                .ok_or(RejectReason::WitnessMismatch)?;
            let RiteOp::Wait(duration, _) = previous.op else {
                return Err(RejectReason::WitnessMismatch);
            };
            if elapsed < u64::from(duration) {
                spec.push(TraceEvent::new(
                    tick,
                    TraceBody::RiteAdvanced {
                        actor: body.mover,
                        rite: rite.id.0,
                        pc: machine.pc,
                        wait_left: duration - elapsed as u16,
                    },
                ));
            } else {
                let channel =
                    Channel::from_u8(track.channel).ok_or(RejectReason::UnclaimedAgency)?;
                let start_events = spec.events().len();
                let _ = run_burst(
                    spec,
                    canon,
                    rite,
                    body.mover,
                    machine.target,
                    Verb::Use,
                    SourceKind::Player,
                    &[channel],
                    machine.pc,
                    rite_steps,
                    pred_ops,
                    tick,
                )?;
                if machine.contact_hit
                    && spec.events()[start_events..]
                        .iter()
                        .any(|e| matches!(e.body, TraceBody::Emitted { .. }))
                {
                    drive_apply_hit(
                        spec,
                        canon,
                        machine.target,
                        Verb::Use,
                        SourceKind::Player,
                        &[channel],
                        rite_steps,
                        pred_ops,
                        tick,
                    )?;
                    if spec.view().rite(body.mover, rite.id).is_some() {
                        spec.push(TraceEvent::new(
                            tick,
                            TraceBody::RiteEnded {
                                actor: body.mover,
                                rite: rite.id.0,
                                status: RiteEnd::Success,
                            },
                        ));
                    }
                }
            }
            continue;
        }
        if let Some(claim) = claims.iter().find(|c| c.actor == body.mover) {
            if !hittable(&spec.view(), canon, claim.target) {
                return Err(RejectReason::WitnessMismatch);
            }
            let channel = Channel::from_u8(track.channel).ok_or(RejectReason::UnclaimedAgency)?;
            spec.push(TraceEvent::new(
                tick,
                TraceBody::MotionContactAdmitted {
                    actor: body.mover,
                    instrument: track.instrument,
                    target: claim.target,
                    rite: rite.id.0,
                    instance: machine.started_at,
                    channel: track.channel,
                    boundary: claim.boundary,
                },
            ));
            let start_events = spec.events().len();
            let _ = run_burst(
                spec,
                canon,
                rite,
                body.mover,
                Some(claim.target),
                Verb::Use,
                SourceKind::Player,
                &[channel],
                machine.pc,
                rite_steps,
                pred_ops,
                tick,
            )?;
            if spec.events()[start_events..]
                .iter()
                .any(|e| matches!(e.body, TraceBody::Emitted { .. }))
            {
                drive_apply_hit(
                    spec,
                    canon,
                    Some(claim.target),
                    Verb::Use,
                    SourceKind::Player,
                    &[channel],
                    rite_steps,
                    pred_ops,
                    tick,
                )?;
                if spec.view().rite(body.mover, rite.id).is_some() {
                    spec.push(TraceEvent::new(
                        tick,
                        TraceBody::RiteEnded {
                            actor: body.mover,
                            rite: rite.id.0,
                            status: RiteEnd::Success,
                        },
                    ));
                }
            }
        } else if elapsed < u64::from(track.wait_ticks) {
            spec.push(TraceEvent::new(
                tick,
                TraceBody::RiteAdvanced {
                    actor: body.mover,
                    rite: rite.id.0,
                    pc: machine.pc,
                    wait_left: track.wait_ticks - elapsed as u16,
                },
            ));
        } else {
            spec.push(TraceEvent::new(
                tick,
                TraceBody::RiteEnded {
                    actor: body.mover,
                    rite: rite.id.0,
                    status: RiteEnd::Fail,
                },
            ));
        }
    }
    Ok(())
}
