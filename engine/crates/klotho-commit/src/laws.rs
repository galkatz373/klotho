//! Admission Laws on the speculative post-state.
#![allow(clippy::too_many_arguments)]

use klotho_canon::{
    Canon, CookedLaw, CookedLawBody, CookedSlot, EvalCtx, PredId, PredStore, eval_pred,
};
use klotho_core::{RejectReason, ResourceId, Sigil};
use klotho_ir::{Channel, Rel, SourceKind, Verb};
use klotho_world::WorldView;

/// `when` true and `must` false → `RejectReason::Law`.
pub fn admit_laws(
    canon: &Canon,
    view: &WorldView,
    actor: Sigil,
    target: Option<Sigil>,
    verb: Verb,
    source: SourceKind,
    claimed: &[Channel],
    swept_hits: bool,
    pred_ops: &mut u32,
) -> Result<(), RejectReason> {
    for law in &canon.laws {
        admit_law(
            canon, view, law, actor, target, verb, source, claimed, swept_hits, pred_ops,
        )?;
    }
    Ok(())
}

/// Evaluate physical Laws in canonical `(LawId, body Sigil)` order against
/// one complete speculative island state (K59).
pub fn admit_phys_laws(
    canon: &Canon,
    view: &WorldView,
    bodies: &[(Sigil, bool)],
    pred_ops: &mut u32,
) -> Result<(), RejectReason> {
    for law in &canon.laws {
        for &(actor, swept_hits) in bodies {
            admit_law(
                canon,
                view,
                law,
                actor,
                None,
                Verb::Move,
                SourceKind::Phys,
                &[],
                swept_hits,
                pred_ops,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn admit_law(
    canon: &Canon,
    view: &WorldView,
    law: &CookedLaw,
    actor: Sigil,
    target: Option<Sigil>,
    verb: Verb,
    source: SourceKind,
    claimed: &[Channel],
    swept_hits: bool,
    pred_ops: &mut u32,
) -> Result<(), RejectReason> {
    let pins = canon.pin_sigils.as_slice();
    let ctx = EvalCtx {
        store: view,
        this: actor,
        target,
        pins,
        verb,
        source,
        claimed,
        swept_hits_opaque_closed: swept_hits,
    };
    let when = canon.pred(law.when).ok_or(RejectReason::Budget)?;
    let when_true = eval_pred(when, &ctx, pred_ops)?;
    match &law.body {
        CookedLawBody::Pred { must, .. } if when_true => {
            let p = canon.pred(*must).ok_or(RejectReason::Budget)?;
            if !eval_pred(p, &ctx, pred_ops)? {
                return Err(RejectReason::Law(law.id));
            }
        }
        CookedLawBody::Conserve { res, over } if when_true => {
            if !conserve_holds(view, actor, *res, *over) {
                return Err(RejectReason::Law(law.id));
            }
        }
        CookedLawBody::Cap {
            mark,
            n,
            require_rel,
        } => {
            let count = cap_count(
                canon,
                view,
                *mark,
                require_rel.as_ref(),
                actor,
                target,
                pins,
                verb,
                source,
                claimed,
                pred_ops,
            )?;
            // Kernel counter: a write that would leave more than `n`
            // marked loci is rejected (9th fire, 101st projectile).
            if count > *n {
                return Err(RejectReason::Law(law.id));
            }
        }
        CookedLawBody::Pred { .. }
        | CookedLawBody::Conserve { .. }
        | CookedLawBody::Ramp { .. }
        | CookedLawBody::Spread { .. } => {}
    }
    Ok(())
}

fn conserve_holds(view: &WorldView, actor: Sigil, res: ResourceId, over: Rel) -> bool {
    // Post-state only: pick/drop that breaks the sum is caught if the kernel
    // also checks pre vs post in the caller. Here we treat negative qty as fail.
    if view.qty(actor, res) < 0 {
        return false;
    }
    let mut neigh = Vec::new();
    PredStore::related(view, actor, over, &mut neigh);
    for s in neigh {
        if view.qty(s, res) < 0 {
            return false;
        }
    }
    let _ = over;
    true
}

fn cap_count(
    canon: &Canon,
    view: &WorldView,
    mark: PredId,
    require_rel: Option<&(Rel, CookedSlot)>,
    actor: Sigil,
    target: Option<Sigil>,
    pins: &[Option<Sigil>],
    verb: Verb,
    source: SourceKind,
    claimed: &[Channel],
    pred_ops: &mut u32,
) -> Result<u16, RejectReason> {
    let prog = canon.pred(mark).ok_or(RejectReason::Budget)?;
    let mut n: u16 = 0;
    for s in view.loci() {
        if let Some((rel, slot)) = require_rel {
            let other = resolve_cap_slot(*slot, actor, target, pins);
            if other.is_none_or(|b| !view.has_rel(s, *rel, b)) {
                continue;
            }
        }
        let ctx = EvalCtx {
            store: view,
            this: s,
            target,
            pins,
            verb,
            source,
            claimed,
            swept_hits_opaque_closed: false,
        };
        if eval_pred(prog, &ctx, pred_ops)? {
            n = n.saturating_add(1);
        }
    }
    Ok(n)
}

fn resolve_cap_slot(
    slot: CookedSlot,
    actor: Sigil,
    target: Option<Sigil>,
    pins: &[Option<Sigil>],
) -> Option<Sigil> {
    match slot {
        CookedSlot::This => Some(actor),
        CookedSlot::Target => target,
        CookedSlot::Other => None,
        CookedSlot::Pin(i) => pins.get(i as usize).copied().flatten(),
    }
}
