//! Trace events → decals and one-shot meshes.

use std::collections::BTreeMap;

use klotho_core::{AabbMm, BlobId, Epoch, Hash, PoseMm, Sigil, Tick};
use klotho_manifest::{
    Decal, MaterialRef, MaterialTag, OneShotMesh, ParticleEmitter, Ribbon, VisualManifest,
};
use klotho_prove::hash_bytes;
use klotho_trace::{PoseReason, RelTag, TraceBody, TraceEvent};

/// Recipe key for lock / wield / hinge / pick / drop / dead impact decals.
pub const RECIPE_IMPACT: &str = "vfx.decal.impact";
/// Recipe key for quantity-change scorch decals.
pub const RECIPE_SCORCH: &str = "vfx.decal.scorch";
/// Recipe key for `Emitted` one-shot meshes.
pub const RECIPE_BURST: &str = "vfx.oneshot.burst";
/// Recipe key for GPU particle emitters. Presentation only.
pub const RECIPE_PARTICLE: &str = "vfx.particle.burst";
/// Recipe key for GPU ribbons. Presentation only.
pub const RECIPE_RIBBON: &str = "vfx.ribbon.trail";

/// Presentation TTL. A cue is live while `now < born + ttl`. `now == born` is live.
pub const DEFAULT_TTL_TICKS: u16 = 4;
/// Hard cap on decals per extract. Extra cues drop.
pub const MAX_DECALS: usize = 128;
/// Hard cap on one-shot meshes per extract. Extra cues drop.
pub const MAX_ONESHOTS: usize = 128;
/// Hard cap on GPU particle emitters per extract. Extra cues drop.
pub const MAX_PARTICLES: usize = 256;
/// Hard cap on GPU ribbons per extract. Extra cues drop.
pub const MAX_RIBBONS: usize = 64;

const MAT: MaterialRef = MaterialRef {
    tag: MaterialTag::Organic,
    palette: 0,
};

enum CuePos {
    Locus(Sigil),
    Pose(PoseMm),
}

enum Cue {
    Decal { pose: CuePos, key: &'static str },
    OneShot { pose: CuePos, key: &'static str },
    Particle { pose: CuePos, key: &'static str },
    Ribbon { pose: CuePos, key: &'static str },
}

fn live(born: Tick, ttl: u16, now: Tick) -> bool {
    born.saturating_add(u64::from(ttl)) > now
}

/// Build a visual buffer of Trace-cued VFX. Missing recipes and poses are skipped.
#[must_use]
pub fn extract_vfx(
    events: &[TraceEvent],
    epoch: Epoch,
    now: Tick,
    recipes: &BTreeMap<String, BlobId>,
    pose_of: impl Fn(Sigil) -> Option<PoseMm>,
) -> VisualManifest {
    let mut decals = Vec::new();
    let mut one_shots = Vec::new();
    let mut particles = Vec::new();
    let mut ribbons = Vec::new();
    for ev in events {
        if !live(ev.tick, DEFAULT_TTL_TICKS, now) {
            continue;
        }
        for kind in cues(&ev.body) {
            match kind {
                Cue::Decal { pose, key } => {
                    if decals.len() >= MAX_DECALS {
                        continue;
                    }
                    let Some(item) = resolve_decal(ev.tick, key, pose, recipes, &pose_of) else {
                        continue;
                    };
                    decals.push(item);
                }
                Cue::OneShot { pose, key } => {
                    if one_shots.len() >= MAX_ONESHOTS {
                        continue;
                    }
                    let Some(item) = resolve_oneshot(ev.tick, key, pose, recipes, &pose_of) else {
                        continue;
                    };
                    one_shots.push(item);
                }
                Cue::Particle { pose, key } => {
                    if particles.len() >= MAX_PARTICLES {
                        continue;
                    }
                    let Some(item) = resolve_particle(ev.tick, key, pose, recipes, &pose_of) else {
                        continue;
                    };
                    particles.push(item);
                }
                Cue::Ribbon { pose, key } => {
                    if ribbons.len() >= MAX_RIBBONS {
                        continue;
                    }
                    let Some(item) = resolve_ribbon(ev.tick, key, pose, recipes, &pose_of) else {
                        continue;
                    };
                    ribbons.push(item);
                }
            }
        }
    }
    VisualManifest::from_vfx(epoch, now, decals, one_shots).with_gpu_vfx(particles, ribbons)
}

fn resolve_pose(pos: CuePos, pose_of: impl Fn(Sigil) -> Option<PoseMm>) -> Option<PoseMm> {
    match pos {
        CuePos::Pose(p) => Some(p),
        CuePos::Locus(s) => pose_of(s),
    }
}

fn resolve_decal(
    born: Tick,
    key: &str,
    pos: CuePos,
    recipes: &BTreeMap<String, BlobId>,
    pose_of: impl Fn(Sigil) -> Option<PoseMm>,
) -> Option<Decal> {
    let &blob = recipes.get(key)?;
    let pose = resolve_pose(pos, pose_of)?;
    Some(Decal {
        blob,
        pose,
        material: MAT,
        born,
        ttl_ticks: DEFAULT_TTL_TICKS,
    })
}

fn resolve_oneshot(
    born: Tick,
    key: &str,
    pos: CuePos,
    recipes: &BTreeMap<String, BlobId>,
    pose_of: impl Fn(Sigil) -> Option<PoseMm>,
) -> Option<OneShotMesh> {
    let &blob = recipes.get(key)?;
    let pose = resolve_pose(pos, pose_of)?;
    Some(OneShotMesh {
        blob,
        pose,
        material: MAT,
        born,
        ttl_ticks: DEFAULT_TTL_TICKS,
    })
}

fn resolve_particle(
    born: Tick,
    key: &str,
    pos: CuePos,
    recipes: &BTreeMap<String, BlobId>,
    pose_of: impl Fn(Sigil) -> Option<PoseMm>,
) -> Option<ParticleEmitter> {
    let &blob = recipes.get(key)?;
    let pose = resolve_pose(pos, pose_of)?;
    Some(ParticleEmitter {
        blob,
        pose,
        material: MAT,
        born,
        ttl_ticks: DEFAULT_TTL_TICKS,
        count: 32,
    })
}

fn resolve_ribbon(
    born: Tick,
    key: &str,
    pos: CuePos,
    recipes: &BTreeMap<String, BlobId>,
    pose_of: impl Fn(Sigil) -> Option<PoseMm>,
) -> Option<Ribbon> {
    let &blob = recipes.get(key)?;
    let pose = resolve_pose(pos, pose_of)?;
    Some(Ribbon {
        blob,
        pose,
        material: MAT,
        born,
        ttl_ticks: DEFAULT_TTL_TICKS,
        length_mm: 1_000,
    })
}

fn cues(body: &TraceBody) -> Vec<Cue> {
    match body {
        TraceBody::RelAdd { a, rel, .. } | TraceBody::RelDel { a, rel, .. }
            if *rel == RelTag::LOCKED_BY || *rel == RelTag::WIELDED_BY =>
        {
            vec![Cue::Decal {
                pose: CuePos::Locus(*a),
                key: RECIPE_IMPACT,
            }]
        }
        TraceBody::RelAdd { a, rel, .. } if *rel == RelTag::DEAD => vec![
            Cue::Decal {
                pose: CuePos::Locus(*a),
                key: RECIPE_IMPACT,
            },
            Cue::Ribbon {
                pose: CuePos::Locus(*a),
                key: RECIPE_RIBBON,
            },
        ],
        TraceBody::PoseCommitted {
            pose,
            reason: PoseReason::Hinge | PoseReason::Pick | PoseReason::Drop,
            ..
        } => vec![Cue::Decal {
            pose: CuePos::Pose(*pose),
            key: RECIPE_IMPACT,
        }],
        TraceBody::QtyChanged { id, .. } => vec![Cue::Decal {
            pose: CuePos::Locus(*id),
            key: RECIPE_SCORCH,
        }],
        TraceBody::Emitted { a, .. } => vec![
            Cue::OneShot {
                pose: CuePos::Locus(*a),
                key: RECIPE_BURST,
            },
            Cue::Particle {
                pose: CuePos::Locus(*a),
                key: RECIPE_PARTICLE,
            },
        ],
        _ => Vec::new(),
    }
}

/// Semantic area-effect identity: hull + Emitted events. Presentation recipes
/// are excluded so a GPU VFX swap cannot move the golden.
#[must_use]
pub fn area_effect_golden(hull: AabbMm, events: &[TraceEvent]) -> Hash {
    let mut bytes = Vec::new();
    for v in [
        hull.min.x, hull.min.y, hull.min.z, hull.max.x, hull.max.y, hull.max.z,
    ] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for ev in events {
        if let TraceBody::Emitted { kind, a, b } = &ev.body {
            bytes.extend_from_slice(&ev.tick.0.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&a.raw().to_le_bytes());
            match b {
                Some(s) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&s.raw().to_le_bytes());
                }
                None => bytes.push(0),
            }
        }
    }
    hash_bytes(&bytes)
}

/// Presenter-owned particle instances. Never a Proposal; never hashed.
#[must_use]
pub fn present_particles(emitters: &[ParticleEmitter], cap: u16) -> Vec<PoseMm> {
    let mut out = Vec::new();
    let cap = cap as usize;
    for e in emitters {
        if out.len() >= cap {
            break;
        }
        let n = (e.count as usize).min(cap - out.len());
        for i in 0..n {
            let mut pose = e.pose;
            pose.y = pose.y.wrapping_add(klotho_core::Mm(i as i32 * 20));
            out.push(pose);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, Mm, PoseMm, ResourceId, YawMd};
    use klotho_trace::{IslandSnap, PoseReason, RelTag, RiteEnd, TraceBody, TraceEvent};

    use super::*;

    fn blob(n: u8) -> BlobId {
        let mut b = [0u8; 32];
        b[0] = n;
        BlobId::from_bytes(b)
    }

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn pose_at(x: i32) -> PoseMm {
        PoseMm::new(Mm(x), Mm(0), Mm(0), YawMd::ZERO)
    }

    fn recipes() -> BTreeMap<String, BlobId> {
        let mut m = BTreeMap::new();
        m.insert(RECIPE_IMPACT.into(), blob(1));
        m.insert(RECIPE_SCORCH.into(), blob(2));
        m.insert(RECIPE_BURST.into(), blob(3));
        m
    }

    fn poses() -> BTreeMap<Sigil, PoseMm> {
        let mut m = BTreeMap::new();
        m.insert(relic(1), pose_at(100));
        m
    }

    fn extract_at(events: &[TraceEvent], now: Tick) -> VisualManifest {
        let poses = poses();
        extract_vfx(events, Epoch::ZERO, now, &recipes(), |s| {
            poses.get(&s).copied()
        })
    }

    fn extract(events: &[TraceEvent]) -> VisualManifest {
        extract_at(events, Tick(1))
    }

    #[test]
    fn empty_trace_has_no_vfx() {
        let vis = extract(&[]);
        assert!(vis.decals.is_empty());
        assert!(vis.one_shots.is_empty());
        assert!(vis.clusters.is_empty());
    }

    #[test]
    fn lock_wield_hinge_qty_emitted_dead_cues() {
        let a = relic(1);
        let hinge = PoseMm::new(Mm(50), Mm(0), Mm(70), YawMd::ZERO);
        let events = [
            TraceEvent::new(
                Tick(1),
                TraceBody::RelAdd {
                    a,
                    rel: RelTag::LOCKED_BY,
                    b: a,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::RelDel {
                    a,
                    rel: RelTag::WIELDED_BY,
                    b: relic(2),
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::PoseCommitted {
                    s: a,
                    pose: hinge,
                    reason: PoseReason::Hinge,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::QtyChanged {
                    id: a,
                    res: ResourceId(0),
                    to: 10,
                    quantum: 10,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::Emitted {
                    kind: 1,
                    a,
                    b: None,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::RelAdd {
                    a,
                    rel: RelTag::DEAD,
                    b: a,
                },
            ),
        ];
        let vis = extract(&events);
        assert_eq!(vis.decals.len(), 5);
        assert_eq!(vis.one_shots.len(), 1);
        assert_eq!(vis.decals[0].blob, blob(1));
        assert_eq!(vis.decals[0].pose, pose_at(100));
        assert_eq!(vis.decals[1].blob, blob(1));
        assert_eq!(vis.decals[2].pose, hinge);
        assert_eq!(vis.decals[3].blob, blob(2));
        assert_eq!(vis.decals[4].blob, blob(1));
        assert_eq!(vis.one_shots[0].blob, blob(3));
        assert_eq!(vis.one_shots[0].pose, pose_at(100));
        assert_eq!(vis.decals[0].born, Tick(1));
        assert_eq!(vis.decals[0].ttl_ticks, DEFAULT_TTL_TICKS);
    }

    #[test]
    fn pick_and_drop_cue_impact() {
        let a = relic(1);
        let events = [
            TraceEvent::new(
                Tick(1),
                TraceBody::PoseCommitted {
                    s: a,
                    pose: PoseMm::new(Mm(1), Mm(0), Mm(2), YawMd::ZERO),
                    reason: PoseReason::Pick,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::PoseCommitted {
                    s: a,
                    pose: PoseMm::new(Mm(3), Mm(0), Mm(4), YawMd::ZERO),
                    reason: PoseReason::Drop,
                },
            ),
        ];
        let vis = extract(&events);
        assert_eq!(vis.decals.len(), 2);
        assert!(vis.decals.iter().all(|d| d.blob == blob(1)));
        assert!(vis.one_shots.is_empty());
    }

    #[test]
    fn spawned_and_unmapped_bodies_are_skipped() {
        let a = relic(1);
        let events = [
            TraceEvent::new(
                Tick(1),
                TraceBody::Spawned {
                    template: 0,
                    sigil: a,
                    at: pose_at(9),
                },
            ),
            TraceEvent::new(Tick(1), TraceBody::SaveRequested),
            TraceEvent::new(Tick(1), TraceBody::Learned { mind: a, fact: 0 }),
            TraceEvent::new(
                Tick(1),
                TraceBody::RiteBegan {
                    actor: a,
                    rite: 0,
                    target: None,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::RiteAdvanced {
                    actor: a,
                    rite: 0,
                    pc: 1,
                    wait_left: 2,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::RiteEnded {
                    actor: a,
                    rite: 0,
                    status: RiteEnd::Success,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::Uttered {
                    speaker: a,
                    fact_ids: vec![1],
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::PoseCommitted {
                    s: a,
                    pose: PoseMm::default(),
                    reason: PoseReason::Land,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::RelAdd {
                    a,
                    rel: RelTag::IN,
                    b: a,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::PoseCommitted {
                    s: a,
                    pose: PoseMm::default(),
                    reason: PoseReason::Interact,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::IslandSnap(
                    IslandSnap::new(0, vec![], vec![], vec![], vec![], vec![]).unwrap(),
                ),
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::RelDel {
                    a,
                    rel: RelTag::DEAD,
                    b: a,
                },
            ),
        ];
        let vis = extract(&events);
        assert!(vis.decals.is_empty());
        assert!(vis.one_shots.is_empty());
    }

    #[test]
    fn missing_recipe_key_is_skipped() {
        let a = relic(1);
        let events = [
            TraceEvent::new(
                Tick(1),
                TraceBody::RelAdd {
                    a,
                    rel: RelTag::LOCKED_BY,
                    b: a,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::QtyChanged {
                    id: a,
                    res: ResourceId(0),
                    to: 1,
                    quantum: 1,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::Emitted {
                    kind: 1,
                    a,
                    b: None,
                },
            ),
        ];
        let vis = extract_vfx(&events, Epoch::ZERO, Tick(1), &BTreeMap::new(), |_| {
            Some(pose_at(1))
        });
        assert!(vis.decals.is_empty());
        assert!(vis.one_shots.is_empty());

        let mut partial = BTreeMap::new();
        partial.insert(RECIPE_IMPACT.into(), blob(1));
        let vis = extract_vfx(&events, Epoch::ZERO, Tick(1), &partial, |_| {
            Some(pose_at(1))
        });
        assert_eq!(vis.decals.len(), 1);
        assert_eq!(vis.decals[0].blob, blob(1));
        assert_ne!(vis.decals[0].blob, BlobId::ZERO);
        assert!(vis.one_shots.is_empty());
    }

    #[test]
    fn missing_pose_is_skipped() {
        let a = relic(9);
        let events = [TraceEvent::new(
            Tick(1),
            TraceBody::RelAdd {
                a,
                rel: RelTag::LOCKED_BY,
                b: a,
            },
        )];
        let vis = extract(&events);
        assert!(vis.decals.is_empty());
        assert!(vis.one_shots.is_empty());
    }

    #[test]
    fn ttl_hides_expired_cues() {
        let a = relic(1);
        let events = [TraceEvent::new(
            Tick(1),
            TraceBody::RelAdd {
                a,
                rel: RelTag::LOCKED_BY,
                b: a,
            },
        )];
        let at_born = extract_at(&events, Tick(1));
        assert_eq!(at_born.decals.len(), 1);
        let last_live = extract_at(&events, Tick(4));
        assert_eq!(last_live.decals.len(), 1);
        let expired = extract_at(&events, Tick(5));
        assert!(expired.decals.is_empty());
        assert!(expired.one_shots.is_empty());
    }

    #[test]
    fn cap_drops_extra_cues() {
        let a = relic(1);
        let n = MAX_DECALS + 1;
        let events: Vec<_> = (0..n)
            .map(|i| {
                TraceEvent::new(
                    Tick(1),
                    TraceBody::PoseCommitted {
                        s: a,
                        pose: pose_at(i as i32),
                        reason: PoseReason::Hinge,
                    },
                )
            })
            .collect();
        let vis = extract(&events);
        assert_eq!(vis.decals.len(), MAX_DECALS);
        assert_eq!(vis.decals.len(), 128);
        assert_eq!(vis.decals[0].pose, pose_at(0));
        assert_eq!(
            vis.decals[MAX_DECALS - 1].pose,
            pose_at((MAX_DECALS - 1) as i32)
        );
        assert!(
            vis.decals
                .iter()
                .all(|d| d.pose != pose_at(MAX_DECALS as i32))
        );
        assert!(vis.one_shots.is_empty());
    }

    #[test]
    fn oneshot_cap_drops_extra_cues() {
        let a = relic(1);
        let n = MAX_ONESHOTS + 1;
        let events: Vec<_> = (0..n)
            .map(|_| {
                TraceEvent::new(
                    Tick(1),
                    TraceBody::Emitted {
                        kind: 1,
                        a,
                        b: None,
                    },
                )
            })
            .collect();
        let vis = extract(&events);
        assert_eq!(vis.one_shots.len(), MAX_ONESHOTS);
        assert_eq!(vis.one_shots.len(), 128);
        assert_eq!(vis.decals.len(), 0);
    }

    #[test]
    fn no_sigil_on_decal_or_oneshot() {
        let a = relic(1);
        let events = [
            TraceEvent::new(
                Tick(1),
                TraceBody::RelAdd {
                    a,
                    rel: RelTag::LOCKED_BY,
                    b: a,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::Emitted {
                    kind: 1,
                    a,
                    b: None,
                },
            ),
        ];
        let vis = extract(&events);
        let Decal {
            blob: _,
            pose: _,
            material: _,
            born: _,
            ttl_ticks: _,
        } = vis.decals[0];
        let OneShotMesh {
            blob: _,
            pose: _,
            material: _,
            born: _,
            ttl_ticks: _,
        } = vis.one_shots[0];
        let VisualManifest {
            epoch: _,
            tick: _,
            clusters: _,
            materials: _,
            masked: _,
            masked_materials: _,
            skinned: _,
            palettes: _,
            lights: _,
            probes: _,
            post: _,
            debug_sigils: _,
            decals: _,
            one_shots: _,
            particles: _,
            ribbons: _,
        } = vis;
    }

    #[test]
    fn gpu_particles_need_their_own_recipe() {
        let a = relic(1);
        let events = [TraceEvent::new(
            Tick(1),
            TraceBody::Emitted {
                kind: 1,
                a,
                b: None,
            },
        )];
        let vis = extract(&events);
        assert_eq!(vis.one_shots.len(), 1);
        assert!(vis.particles.is_empty());

        let mut recipes = recipes();
        recipes.insert(RECIPE_PARTICLE.into(), blob(9));
        let vis = extract_vfx(&events, Epoch::ZERO, Tick(1), &recipes, |s| {
            poses().get(&s).copied()
        });
        assert_eq!(vis.particles.len(), 1);
        assert_eq!(vis.particles[0].blob, blob(9));
        assert_eq!(vis.one_shots.len(), 1);
    }

    #[test]
    fn presentation_swap_does_not_move_area_effect_golden() {
        let a = relic(1);
        let hull = AabbMm::from_point(klotho_core::IVec3 { x: 0, y: 0, z: 0 });
        let events = [TraceEvent::new(
            Tick(1),
            TraceBody::Emitted {
                kind: 7,
                a,
                b: None,
            },
        )];
        let semantic = area_effect_golden(hull, &events);
        let mut fire = recipes();
        fire.insert(RECIPE_PARTICLE.into(), blob(4));
        let mut smoke = recipes();
        smoke.insert(RECIPE_PARTICLE.into(), blob(5));
        let vis_fire = extract_vfx(&events, Epoch::ZERO, Tick(1), &fire, |s| {
            poses().get(&s).copied()
        });
        let vis_smoke = extract_vfx(&events, Epoch::ZERO, Tick(1), &smoke, |s| {
            poses().get(&s).copied()
        });
        assert_eq!(semantic, area_effect_golden(hull, &events));
        assert_ne!(vis_fire.particles[0].blob, vis_smoke.particles[0].blob);
        assert_eq!(vis_fire.one_shots[0].pose, vis_smoke.one_shots[0].pose);
    }

    #[test]
    fn present_particles_are_not_a_proposal() {
        let e = ParticleEmitter {
            blob: blob(1),
            pose: pose_at(0),
            material: MAT,
            born: Tick(1),
            ttl_ticks: DEFAULT_TTL_TICKS,
            count: 4,
        };
        let poses = present_particles(&[e], 8);
        assert_eq!(poses.len(), 4);
        assert_ne!(poses[0], poses[3]);
        let ParticleEmitter {
            blob: _,
            pose: _,
            material: _,
            born: _,
            ttl_ticks: _,
            count: _,
        } = e;
    }
}
