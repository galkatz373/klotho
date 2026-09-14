//! Kernel-derived swept AABB (K24). Proposer-supplied swept is ignored.

use klotho_core::{AabbMm, BlobId, HullWitness, PoseMm, RejectReason, Sigil};
use klotho_geom::{CONTACT_SLOP_MM, bounds, cooked_shape, swept_against};
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

/// K24 witness check for Space/Motion. Translation AABB; Hearth/Ash hashes stay put.
pub fn check_space(
    view: &WorldView,
    mover: Sigil,
    proposed: PoseMm,
    hull: BlobId,
    hint_overlap: bool,
) -> Result<bool, RejectReason> {
    if let Some(bound) = view.hull_id(mover) {
        if bound != BlobId::ZERO && hull != BlobId::ZERO && bound != hull {
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

/// Oriented-primitive check for a physical island body (K61 / K62).
///
/// Resting or resolving contact within [`CONTACT_SLOP_MM`] is legal. Crossing
/// a closed barrier or finishing more than slop deep rejects the island.
/// Space/Motion keep [`check_space`] so kinematic goldens do not move.
pub fn check_phys_body(
    view: &WorldView,
    mover: Sigil,
    proposed: PoseMm,
    hull: BlobId,
    witness: HullWitness,
) -> Result<bool, RejectReason> {
    if let Some(bound) = view.hull_id(mover) {
        if bound != BlobId::ZERO && hull != BlobId::ZERO && bound != hull {
            return Err(RejectReason::WrongHull);
        }
    }
    if witness.epoch != view.epoch() {
        return Err(RejectReason::StaleEpoch);
    }
    if !witness.shape.is_dynamic() {
        return Err(RejectReason::WitnessMismatch);
    }
    if witness.shape != view.body_physics(mover).shape {
        return Err(RejectReason::WrongHull);
    }
    let Some(local) = view.hull(mover) else {
        return Ok(false);
    };
    let shape = cooked_shape(witness.shape, local).map_err(|_| RejectReason::WitnessMismatch)?;
    let prev = view.pose(mover).unwrap_or(proposed);
    let start_b = bounds(shape, prev).map_err(|_| RejectReason::WitnessMismatch)?;
    let end_b = bounds(shape, proposed).map_err(|_| RejectReason::WitnessMismatch)?;
    let swept = start_b.swept_union(end_b);
    let mut candidates = view.space_candidates(swept, true);
    candidates.sort_unstable();
    for s in candidates {
        if s == mover || !view.opaque_closed(s) {
            continue;
        }
        let Some(ol) = view.hull(s) else {
            continue;
        };
        let Some(opose) = view.pose(s) else {
            continue;
        };
        let occ_kind = view.body_physics(s).shape;
        let occ = cooked_shape(occ_kind, ol).map_err(|_| RejectReason::WitnessMismatch)?;
        let hit = swept_against(shape, prev, proposed, occ, opose)
            .map_err(|_| RejectReason::WitnessMismatch)?;
        if hit.crossing || hit.end_depth_mm > CONTACT_SLOP_MM {
            return Err(RejectReason::WitnessMismatch);
        }
    }
    let _ = witness.overlaps_closed_opaque;
    Ok(false)
}
