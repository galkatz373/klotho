//! Bounded integer capsule traversal. No World, caches, or gameplay authority.

use klotho_core::{CharacterPhysics, IVec3, PoseMm, Support};

use crate::{CONTACT_SLOP_MM, GeomError, Shape, bounds, contact, swept_against};

/// Hard cap on nearby occupancy per driven-character solve.
pub const MAX_CHARACTER_OBSTACLES: usize = 512;

/// One immutable obstacle for a character query, in canonical identity order.
#[derive(Copy, Clone, Debug)]
pub struct CharacterObstacle {
    /// Canonical geometry.
    pub shape: Shape,
    /// Authoritative obstacle pose.
    pub pose: PoseMm,
}

/// Resolved capsule pose and grounding evidence.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct CharacterResolution {
    /// The only root pose offered for admission.
    pub pose: PoseMm,
    /// Walkable support normal and depth, if grounded.
    pub support: Option<Support>,
}

/// Resolve at most 1000 mm on each axis in 64 bounded sweep intervals.
/// Walkable slopes use vertical correction; steps use an up/forward/down path.
/// A grounded capsule snaps down by at most the Canon riser bound. Airborne
/// capsules receive only the supplied vertical displacement.
pub fn resolve_character(
    capsule: Shape,
    start: PoseMm,
    desire: IVec3,
    policy: CharacterPhysics,
    obstacles: &[CharacterObstacle],
) -> Result<CharacterResolution, GeomError> {
    if obstacles.len() > MAX_CHARACTER_OBSTACLES
        || !matches!(capsule, Shape::Capsule { .. })
        || !policy.is_valid()
        || [desire.x, desire.y, desire.z]
            .iter()
            .any(|v| v.unsigned_abs() > 1000)
    {
        return Err(GeomError::Malformed);
    }
    let mut pose = depenetrate(capsule, start, obstacles)?;
    let grounded = support_at(capsule, pose, policy, obstacles)?.is_some();
    let span = desire
        .x
        .unsigned_abs()
        .max(desire.z.unsigned_abs())
        .max(desire.y.unsigned_abs());
    let count = span.div_ceil(16).clamp(1, 64);
    let mut previous = IVec3::ZERO;
    for i in 1..=count {
        let part = IVec3 {
            x: (i64::from(desire.x) * i64::from(i) / i64::from(count)) as i32,
            y: (i64::from(desire.y) * i64::from(i) / i64::from(count)) as i32,
            z: (i64::from(desire.z) * i64::from(i) / i64::from(count)) as i32,
        };
        let delta = part.wrapping_sub(previous);
        previous = part;
        let next = translate(pose, delta);
        if clear_path(capsule, pose, next, obstacles)? {
            pose = next;
            continue;
        }
        // A slope may lift the capsule, but only on a walkable normal.
        let mut lifted = next;
        let mut walkable = true;
        let mut lift = 0;
        for obstacle in obstacles {
            if let Some(hit) = contact(capsule, next, obstacle.shape, obstacle.pose)? {
                if hit.depth_mm <= CONTACT_SLOP_MM {
                    continue;
                }
                if hit.normal.1 < policy.slope_min_y {
                    walkable = false;
                    break;
                }
                lift = lift
                    .max((i64::from(hit.depth_mm) * 32767 / i64::from(hit.normal.1) + 1) as i32);
            }
        }
        if walkable && lift > 0 && lift <= 32 {
            lifted.y.0 = lifted.y.0.saturating_add(lift);
            if clear(capsule, lifted, obstacles)? {
                pose = lifted;
                continue;
            }
        }
        // Terrain cannot be bypassed with a step: steep inclines stay blocked.
        let terrain_blocked = obstacles.iter().any(|o| {
            matches!(
                o.shape,
                Shape::TriangleMesh { .. } | Shape::Heightfield { .. }
            ) && contact(capsule, next, o.shape, o.pose)
                .ok()
                .flatten()
                .is_some_and(|h| h.depth_mm > CONTACT_SLOP_MM)
        });
        let too_high = obstacles.iter().any(|o| {
            !matches!(
                o.shape,
                Shape::TriangleMesh { .. } | Shape::Heightfield { .. }
            ) && contact(capsule, next, o.shape, o.pose)
                .ok()
                .flatten()
                .is_some_and(|h| h.depth_mm > CONTACT_SLOP_MM)
                && bounds(o.shape, o.pose)
                    .ok()
                    .zip(bounds(capsule, pose).ok())
                    .is_some_and(|(a, b)| a.max.y - b.min.y > policy.step_mm)
        });
        if grounded
            && !terrain_blocked
            && !too_high
            && policy.step_mm > 0
            && (delta.x != 0 || delta.z != 0)
        {
            let up = translate(
                pose,
                IVec3 {
                    x: 0,
                    y: policy.step_mm,
                    z: 0,
                },
            );
            let over = translate(up, IVec3 { y: 0, ..delta });
            if clear_path(capsule, pose, up, obstacles)?
                && clear_path(capsule, up, over, obstacles)?
            {
                let landed = descend(capsule, over, policy.step_mm, obstacles)?;
                if support_at(capsule, landed, policy, obstacles)?.is_some()
                    && landed.y.0 - pose.y.0 <= policy.step_mm
                {
                    pose = landed;
                }
            }
        }
    }
    if grounded && desire.y <= 0 {
        pose = descend(capsule, pose, policy.step_mm + CONTACT_SLOP_MM, obstacles)?;
    }
    Ok(CharacterResolution {
        pose,
        support: support_at(capsule, pose, policy, obstacles)?,
    })
}

fn translate(mut pose: PoseMm, d: IVec3) -> PoseMm {
    pose.x.0 = pose.x.0.saturating_add(d.x);
    pose.y.0 = pose.y.0.saturating_add(d.y);
    pose.z.0 = pose.z.0.saturating_add(d.z);
    pose
}

fn clear(shape: Shape, pose: PoseMm, obstacles: &[CharacterObstacle]) -> Result<bool, GeomError> {
    for o in obstacles {
        if contact(shape, pose, o.shape, o.pose)?.is_some_and(|h| h.depth_mm > CONTACT_SLOP_MM) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn clear_path(
    shape: Shape,
    from: PoseMm,
    to: PoseMm,
    obstacles: &[CharacterObstacle],
) -> Result<bool, GeomError> {
    if !clear(shape, to, obstacles)? {
        return Ok(false);
    }
    for o in obstacles {
        // Terrain uses the bounded 16 mm narrow-phase intervals above. Its
        // enclosing AABB is not a solid wall across an entire ramp.
        if !matches!(
            o.shape,
            Shape::TriangleMesh { .. } | Shape::Heightfield { .. }
        ) && swept_against(shape, from, to, o.shape, o.pose)?.crossing
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn depenetrate(
    shape: Shape,
    mut pose: PoseMm,
    obstacles: &[CharacterObstacle],
) -> Result<PoseMm, GeomError> {
    for _ in 0..8 {
        for o in obstacles {
            if let Some(h) = contact(shape, pose, o.shape, o.pose)? {
                if h.depth_mm <= CONTACT_SLOP_MM {
                    continue;
                }
                let d = i64::from(h.depth_mm + 1);
                pose = translate(
                    pose,
                    IVec3 {
                        x: (d * i64::from(h.normal.0) / 32767) as i32,
                        y: (d * i64::from(h.normal.1) / 32767) as i32,
                        z: (d * i64::from(h.normal.2) / 32767) as i32,
                    },
                );
            }
        }
        if clear(shape, pose, obstacles)? {
            return Ok(pose);
        }
    }
    Err(GeomError::Malformed)
}

fn support_at(
    shape: Shape,
    pose: PoseMm,
    policy: CharacterPhysics,
    obstacles: &[CharacterObstacle],
) -> Result<Option<Support>, GeomError> {
    let probe = translate(
        pose,
        IVec3 {
            x: 0,
            y: -CONTACT_SLOP_MM - 1,
            z: 0,
        },
    );
    let mut best = None;
    for o in obstacles {
        if let Some(h) = contact(shape, probe, o.shape, o.pose)? {
            let low_riser = !matches!(
                o.shape,
                Shape::TriangleMesh { .. } | Shape::Heightfield { .. }
            ) && h.normal.1 > 0
                && bounds(o.shape, o.pose)?.max.y - bounds(shape, pose)?.min.y <= policy.step_mm;
            if (h.normal.1 >= policy.slope_min_y || low_riser)
                && best.is_none_or(|s: Support| s.1 < h.normal.1)
            {
                best = Some((h.normal.0, h.normal.1, h.normal.2, 0));
            }
        }
    }
    Ok(best)
}

fn descend(
    shape: Shape,
    start: PoseMm,
    distance: i32,
    obstacles: &[CharacterObstacle],
) -> Result<PoseMm, GeomError> {
    // Small ordered intervals prevent crossing a thin platform on descent.
    let mut pose = start;
    for _ in 0..distance.div_euclid(16) + 1 {
        let moved = start.y.0.saturating_sub(pose.y.0);
        let remaining = distance - moved;
        if remaining <= 0 {
            break;
        }
        let down = translate(
            pose,
            IVec3 {
                x: 0,
                y: -remaining.min(16),
                z: 0,
            },
        );
        if clear_path(shape, pose, down, obstacles)? {
            pose = down;
            continue;
        }
        let mut lo = 0;
        let mut hi = remaining.min(16);
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if clear(
                shape,
                translate(
                    pose,
                    IVec3 {
                        x: 0,
                        y: -mid,
                        z: 0,
                    },
                ),
                obstacles,
            )? {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        pose.y.0 = pose.y.0.saturating_sub(lo);
        break;
    }
    // Force malformed geometry through validation even for a zero descent.
    let _ = bounds(shape, pose)?;
    Ok(pose)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{AabbMm, Mm, YawMd};

    #[test]
    fn fast_capsule_and_oversize_workloads_fail_closed() {
        let capsule = Shape::capsule(IVec3 { x: 0, y: 900, z: 0 }, 300, 600).unwrap();
        let wall = CharacterObstacle {
            shape: Shape::oriented_box(AabbMm::new(
                IVec3 {
                    x: -2000,
                    y: 0,
                    z: -1,
                },
                IVec3 {
                    x: 2000,
                    y: 3000,
                    z: 1,
                },
            ))
            .unwrap(),
            pose: PoseMm::new(Mm(0), Mm(0), Mm(500), YawMd::ZERO),
        };
        let policy = CharacterPhysics::default();
        let resolved = resolve_character(
            capsule,
            PoseMm::default(),
            IVec3 {
                x: 0,
                y: 0,
                z: 1000,
            },
            policy,
            &[wall],
        )
        .unwrap();
        assert!(resolved.pose.z.0 <= 201);
        assert!(
            resolve_character(
                capsule,
                PoseMm::default(),
                IVec3::ZERO,
                policy,
                &vec![wall; MAX_CHARACTER_OBSTACLES + 1]
            )
            .is_err()
        );
        assert!(
            resolve_character(
                capsule,
                PoseMm::default(),
                IVec3 {
                    x: 0,
                    y: 0,
                    z: 1001
                },
                policy,
                &[]
            )
            .is_err()
        );
    }
}
