//! Admit a validated constraint break: RelDel PartOf, spawn fragments, Trace.
//!
//! Phys proposes only the impulse witness. Semantic consequences live here so
//! a Law, cap, or later check still rolls the whole island back (K21).

use klotho_canon::Canon;
use klotho_core::{
    AabbMm, IVec3, MAX_COLLAPSE_FRAGMENTS, MAX_FRAGMENTS_GLOBAL, Mm, NO_ISLAND, RejectReason,
    Sigil, Tick,
};
use klotho_ir::Rel;
use klotho_trace::{RelTag, TraceBody, TraceEvent};
use klotho_world::SpecDelta;

use crate::proposal::ConstraintBreakClaim;
use crate::rite::alloc_spawn_sigil;

const FRAGMENT_HALF_MM: i32 = 50;

pub(crate) fn apply_constraint_breaks(
    spec: &mut SpecDelta,
    canon: &Canon,
    breaks: &[ConstraintBreakClaim],
    tick: Tick,
) -> Result<(), RejectReason> {
    for brk in breaks {
        let joint = canon
            .constraint(brk.constraint)
            .ok_or(RejectReason::WrongHull)?;
        detach_part_of(spec, tick, joint.a, joint.b);
        let spawned = spawn_fragments(spec, canon, tick, joint)?;
        spec.push(TraceEvent::new(
            tick,
            TraceBody::ConstraintBroken {
                constraint: brk.constraint,
                a: joint.a,
                b: joint.b,
                impulse: brk.impulse,
                fragments: spawned,
            },
        ));
    }
    Ok(())
}

fn detach_part_of(spec: &mut SpecDelta, tick: Tick, a: Sigil, b: Sigil) {
    for (src, dst) in [(a, b), (b, a)] {
        if spec.view().has_rel(src, Rel::PartOf, dst) {
            spec.push(TraceEvent::new(
                tick,
                TraceBody::RelDel {
                    a: src,
                    rel: RelTag::PART_OF,
                    b: dst,
                },
            ));
        }
    }
}

fn spawn_fragments(
    spec: &mut SpecDelta,
    canon: &Canon,
    tick: Tick,
    joint: klotho_core::ConstraintPhysics,
) -> Result<u8, RejectReason> {
    if joint.fragments == 0 {
        return Ok(0);
    }
    if joint.fragments > MAX_COLLAPSE_FRAGMENTS {
        return Err(RejectReason::WitnessMismatch);
    }
    let mark = canon
        .affordance_id("Fragment")
        .ok_or(RejectReason::WitnessMismatch)?;
    let existing = fragment_count(spec, mark);
    if existing.saturating_add(u16::from(joint.fragments)) > MAX_FRAGMENTS_GLOBAL {
        return Err(RejectReason::Budget);
    }
    let origin = spec
        .view()
        .pose(joint.a)
        .or_else(|| spec.view().pose(joint.b))
        .unwrap_or_default();
    let hull = spec
        .view()
        .hull_id(joint.a)
        .filter(|id| *id != klotho_core::BlobId::ZERO)
        .or_else(|| {
            spec.view()
                .hull_id(joint.b)
                .filter(|id| *id != klotho_core::BlobId::ZERO)
        })
        .unwrap_or(joint.binding);
    let template = canon
        .facts
        .iter()
        .position(|n| n.as_str() == "Fragment")
        .unwrap_or(0) as u16;
    for i in 0..joint.fragments {
        let sigil = alloc_spawn_sigil(spec)?;
        let mut at = origin;
        at.x = Mm(origin.x.0.saturating_add(i32::from(i).saturating_mul(80)));
        spec.push(TraceEvent::new(
            tick,
            TraceBody::Spawned {
                template,
                sigil,
                at,
            },
        ));
        spec.set_affordance(sigil, mark, true)
            .map_err(|_| RejectReason::Budget)?;
        spec.set_hull(sigil, fragment_hull(), hull)
            .map_err(|_| RejectReason::Budget)?;
        spec.set_island(sigil, NO_ISLAND, 0)
            .map_err(|_| RejectReason::Budget)?;
    }
    if fragment_count(spec, mark) > MAX_FRAGMENTS_GLOBAL {
        return Err(RejectReason::Budget);
    }
    Ok(joint.fragments)
}

fn fragment_count(spec: &SpecDelta, mark: klotho_core::AffordanceId) -> u16 {
    spec.view()
        .loci()
        .filter(|&s| spec.view().has_affordance(s, mark))
        .count() as u16
}

fn fragment_hull() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -FRAGMENT_HALF_MM,
            y: 0,
            z: -FRAGMENT_HALF_MM,
        },
        IVec3 {
            x: FRAGMENT_HALF_MM,
            y: FRAGMENT_HALF_MM * 2,
            z: FRAGMENT_HALF_MM,
        },
    )
}
