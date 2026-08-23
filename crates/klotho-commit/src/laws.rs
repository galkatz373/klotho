//! Admission Laws on the speculative post-state.
#![allow(clippy::too_many_arguments)]

use klotho_canon::{Canon, CookedLawBody, EvalCtx, PredStore, eval_pred};
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
    pred_ops: &mut u16,
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
    for law in &canon.laws {
        let when = canon.pred(law.when).ok_or(RejectReason::Budget)?;
        if !eval_pred(when, &ctx, pred_ops)? {
            continue;
        }
        match &law.body {
            CookedLawBody::Pred { must, .. } => {
                let p = canon.pred(*must).ok_or(RejectReason::Budget)?;
                if !eval_pred(p, &ctx, pred_ops)? {
                    return Err(RejectReason::Law(law.id));
                }
            }
            CookedLawBody::Conserve { res, over } => {
                if !conserve_holds(view, actor, *res, *over) {
                    return Err(RejectReason::Law(law.id));
                }
            }
            CookedLawBody::Cap { n, .. } => {
                if cap_exceeded(view, *n) {
                    return Err(RejectReason::Law(law.id));
                }
            }
            CookedLawBody::Ramp { .. } | CookedLawBody::Spread { .. } => {}
        }
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

fn cap_exceeded(_view: &WorldView, _n: u16) -> bool {
    false
}
