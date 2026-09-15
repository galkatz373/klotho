//! Semantic evidence is proposed from the same view as the solved root, then
//! reproduced inside the island transaction. No player Agency is minted here.
use crate::{BodyDelta, MAX_MOTION_CONTACTS, MotionContact};
use klotho_core::{IVec3, PoseMm, RejectReason, Sigil};
use klotho_geom::{GeomError, cooked_shape, semantic_sweep, verify_semantic_sweep};
use klotho_world::WorldView;

fn pose(view: &WorldView<'_>, bodies: &[BodyDelta], actor: Sigil) -> Option<PoseMm> {
    bodies
        .iter()
        .find(|b| b.mover == actor)
        .map(|b| b.pose)
        .or_else(|| view.pose(actor))
}
fn point(root: PoseMm, p: IVec3) -> Result<IVec3, GeomError> {
    let p = klotho_core::rotate(p, root.yaw, root.pitch, root.roll);
    Ok(IVec3 {
        x: root.x.0.checked_add(p.x).ok_or(GeomError::Malformed)?,
        y: root.y.0.checked_add(p.y).ok_or(GeomError::Malformed)?,
        z: root.z.0.checked_add(p.z).ok_or(GeomError::Malformed)?,
    })
}
fn geometry(
    view: &WorldView<'_>,
    bodies: &[BodyDelta],
    actor: Sigil,
    sweep: usize,
) -> Result<([[IVec3; 2]; 2], i32, Sigil), GeomError> {
    let track = view.contact_track(actor).ok_or(GeomError::Malformed)?;
    let (_, machine) = view.contact_window(actor).ok_or(GeomError::Malformed)?;
    let boundary = usize::from(view.contact_boundary(actor).ok_or(GeomError::Malformed)?);
    let channel = track.sweeps.get(sweep).ok_or(GeomError::Malformed)?;
    let start = view.pose(actor).ok_or(GeomError::Malformed)?;
    let end = bodies
        .iter()
        .find(|b| b.mover == actor)
        .ok_or(GeomError::Malformed)?
        .pose;
    let a = channel.samples.get(boundary).ok_or(GeomError::Malformed)?;
    let b = channel
        .samples
        .get(boundary + 1)
        .ok_or(GeomError::Malformed)?;
    Ok((
        [
            [point(start, a[0])?, point(start, a[1])?],
            [point(end, b[0])?, point(end, b[1])?],
        ],
        channel.radius_mm,
        machine.target.ok_or(GeomError::Malformed)?,
    ))
}
/// Pure sampling for a fully resolved island. All query failures fail the island
/// closed. One bound target and the first name-ordered intersecting channel are
/// the frozen initial hit policy; a successful contact consumes the action.
pub fn sample_motion_contacts(
    view: &WorldView<'_>,
    bodies: &[BodyDelta],
) -> Result<Vec<MotionContact>, GeomError> {
    let mut out = Vec::new();
    let mut charged = 0_u32;
    if bodies
        .iter()
        .filter(|b| view.contact_window(b.mover).is_some())
        .count()
        > MAX_MOTION_CONTACTS
    {
        return Err(GeomError::Malformed);
    }
    for body in bodies {
        let Some((_, machine)) = view.contact_window(body.mover) else {
            continue;
        };
        let Some(target) = machine.target else {
            continue;
        };
        if view.character_physics(body.mover).is_none() {
            return Err(GeomError::Malformed);
        }
        let track = view.contact_track(body.mover).ok_or(GeomError::Malformed)?;
        let shape = cooked_shape(
            view.body_physics(target).shape,
            view.hull(target).ok_or(GeomError::Malformed)?,
        )?;
        let target_pose = pose(view, bodies, target).ok_or(GeomError::Malformed)?;
        for sweep in 0..track.sweeps.len() {
            let (ends, radius, _) = geometry(view, bodies, body.mover, sweep)?;
            charged = charged
                .checked_add(klotho_geom::semantic_sample_count(ends)?)
                .ok_or(GeomError::Malformed)?;
            if charged > 262_144 {
                return Err(GeomError::Malformed);
            }
            if let Some(witness) = semantic_sweep(ends, radius, shape, target_pose)? {
                out.push(MotionContact {
                    actor: body.mover,
                    rite_instance: machine.started_at,
                    target,
                    track: track.clone(),
                    sweep: sweep as u8,
                    boundary: view
                        .contact_boundary(body.mover)
                        .ok_or(GeomError::Malformed)?,
                    witness,
                });
                break;
            }
        }
        if out.len() > MAX_MOTION_CONTACTS {
            return Err(GeomError::Malformed);
        }
    }
    Ok(out)
}
pub(crate) fn validate(
    view: &WorldView<'_>,
    bodies: &[BodyDelta],
    claims: &[MotionContact],
) -> Result<(), RejectReason> {
    if claims.len() > MAX_MOTION_CONTACTS {
        return Err(RejectReason::IslandTooLarge);
    }
    if !claims.windows(2).all(|w| w[0].actor < w[1].actor) {
        return Err(RejectReason::WitnessMismatch);
    }
    for claim in claims {
        let (_, machine) = view
            .contact_window(claim.actor)
            .ok_or(RejectReason::UnclaimedAgency)?;
        if view.character_physics(claim.actor).is_none()
            || view.contact_track(claim.actor) != Some(&claim.track)
            || Some(claim.boundary) != view.contact_boundary(claim.actor)
            || claim.rite_instance != machine.started_at
            || machine.target != Some(claim.target)
            || claim.actor == claim.target
        {
            return Err(RejectReason::WitnessMismatch);
        }
        let (ends, radius, target) = geometry(view, bodies, claim.actor, claim.sweep.into())
            .map_err(|_| RejectReason::WitnessMismatch)?;
        let shape = cooked_shape(
            view.body_physics(target).shape,
            view.hull(target).ok_or(RejectReason::WrongHull)?,
        )
        .map_err(|_| RejectReason::WrongHull)?;
        let target_pose = pose(view, bodies, target).ok_or(RejectReason::WitnessMismatch)?;
        if !verify_semantic_sweep(ends, radius, shape, target_pose, claim.witness)
            .map_err(|_| RejectReason::WitnessMismatch)?
        {
            return Err(RejectReason::WitnessMismatch);
        }
    }
    Ok(())
}
