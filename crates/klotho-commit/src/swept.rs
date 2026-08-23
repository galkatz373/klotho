//! Kernel-derived swept AABB (K24). Proposer-supplied swept is ignored.

use klotho_core::{AabbMm, PoseMm, RejectReason, Sigil};
use klotho_world::{WorldView, world_aabb};

/// Conservative swept of `hull(mover)` from current pose to `proposed`.
#[must_use]
pub fn derived_swept(view: &WorldView, mover: Sigil, proposed: PoseMm) -> Option<AabbMm> {
    let local = view.hull(mover)?;
    let prev = view.pose(mover).unwrap_or(proposed);
    let a = world_aabb(local, prev.translation());
    let b = world_aabb(local, proposed.translation());
    Some(a.swept_union(b))
}

/// Exact overlap vs OpaqueClosed hulls (sleepers included). Skips `mover`.
#[must_use]
pub fn hits_opaque_closed(view: &WorldView, mover: Sigil, swept: AabbMm) -> bool {
    for s in view.space_candidates(swept, true) {
        if s == mover {
            continue;
        }
        if let Some(h) = view.posed_hull(s) {
            if h.intersects(swept) {
                return true;
            }
        }
    }
    false
}

/// K24 witness check. `Ok(hits)` or `WrongHull`.
pub fn check_space(
    view: &WorldView,
    mover: Sigil,
    proposed: PoseMm,
    hull: klotho_core::BlobId,
    hint_overlap: bool,
) -> Result<bool, RejectReason> {
    if let Some(bound) = view.hull_id(mover) {
        if bound != klotho_core::BlobId::ZERO && hull != klotho_core::BlobId::ZERO && bound != hull
        {
            return Err(RejectReason::WrongHull);
        }
    }
    let Some(swept) = derived_swept(view, mover, proposed) else {
        return Ok(false);
    };
    let hits = hits_opaque_closed(view, mover, swept);
    if hits && !hint_overlap {
        return Err(RejectReason::WitnessMismatch);
    }
    Ok(hits)
}
