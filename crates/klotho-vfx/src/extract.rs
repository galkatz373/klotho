//! Trace events → decals and one-shot meshes.

use std::collections::BTreeMap;

use klotho_core::{BlobId, Epoch, PoseMm, Sigil, Tick};
use klotho_manifest::{Decal, MaterialRef, MaterialTag, OneShotMesh, VisualManifest};
use klotho_trace::{PoseReason, RelTag, TraceBody, TraceEvent};

/// Recipe key for lock / wield / hinge / pick / drop / dead impact decals.
pub const RECIPE_IMPACT: &str = "vfx.decal.impact";
/// Recipe key for quantity-change scorch decals.
pub const RECIPE_SCORCH: &str = "vfx.decal.scorch";
/// Recipe key for `Emitted` one-shot meshes.
pub const RECIPE_BURST: &str = "vfx.oneshot.burst";

/// Presentation TTL. A cue is live while `now < born + ttl`. `now == born` is live.
pub const DEFAULT_TTL_TICKS: u16 = 4;
/// Hard cap on decals per extract. Extra cues drop.
pub const MAX_DECALS: usize = 128;
/// Hard cap on one-shot meshes per extract. Extra cues drop.
pub const MAX_ONESHOTS: usize = 128;

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
    for ev in events {
        if !live(ev.tick, DEFAULT_TTL_TICKS, now) {
            continue;
        }
        let Some(kind) = cue(&ev.body) else {
            continue;
        };
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
        }
    }
    VisualManifest::from_vfx(epoch, now, decals, one_shots)
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

fn cue(body: &TraceBody) -> Option<Cue> {
    match body {
        TraceBody::RelAdd { a, rel, .. } | TraceBody::RelDel { a, rel, .. }
            if *rel == RelTag::LOCKED_BY || *rel == RelTag::WIELDED_BY =>
        {
            Some(Cue::Decal {
                pose: CuePos::Locus(*a),
                key: RECIPE_IMPACT,
            })
        }
        TraceBody::RelAdd { a, rel, .. } if *rel == RelTag::DEAD => Some(Cue::Decal {
            pose: CuePos::Locus(*a),
            key: RECIPE_IMPACT,
        }),
        TraceBody::PoseCommitted {
            pose,
            reason: PoseReason::Hinge | PoseReason::Pick | PoseReason::Drop,
            ..
        } => Some(Cue::Decal {
            pose: CuePos::Pose(*pose),
            key: RECIPE_IMPACT,
        }),
        TraceBody::QtyChanged { id, .. } => Some(Cue::Decal {
            pose: CuePos::Locus(*id),
            key: RECIPE_SCORCH,
        }),
        TraceBody::Emitted { a, .. } => Some(Cue::OneShot {
            pose: CuePos::Locus(*a),
            key: RECIPE_BURST,
        }),
        _ => None,
    }
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
        } = vis;
    }
}
