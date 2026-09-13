//! Trace events → [`SonicManifest`]. Trace is the cue list.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use klotho_core::{AabbMm, BlobId, Epoch, IVec3, Sigil};
use klotho_manifest::{BedRef, GrainVoice, Observer, SonicManifest};

use crate::music::{cue_from_events, stems_for};
use klotho_trace::{PoseReason, RelTag, TraceBody, TraceEvent};

const KNOCK: &str = "grain.wood.knock";
const CRACKLE: &str = "grain.fire.crackle";
const GAIN_FULL: u16 = 1000;

enum CuePos {
    Locus(Sigil),
    Xz(IVec3),
}

/// Build a sonic buffer from committed Trace. Missing grain tags are skipped.
/// `bed` is caller-supplied; extract does not invent one.
#[must_use]
pub fn extract_sonic(
    events: &[TraceEvent],
    epoch: Epoch,
    tags: &BTreeMap<String, BlobId>,
    bed: Option<BedRef>,
    pose_of: impl Fn(Sigil) -> Option<IVec3>,
    opaque: &[AabbMm],
    observer: Observer,
) -> SonicManifest {
    let eye = observer.eye.translation();
    let mut grains = Vec::new();
    for ev in events {
        let Some((tag, pos)) = cue(&ev.body) else {
            continue;
        };
        let Some(&blob) = tags.get(tag) else {
            continue;
        };
        let pos = match pos {
            CuePos::Xz(p) => Some(p),
            CuePos::Locus(s) => pose_of(s),
        };
        grains.push(GrainVoice {
            blob,
            at: ev.tick,
            gain_milli: GAIN_FULL,
            pos,
            occluded: grain_occluded(pos, eye, opaque),
        });
    }
    let cue = cue_from_events(events);
    let music = match cue {
        klotho_manifest::MusicCue::Explore => None,
        _ => Some(stems_for(cue)),
    };
    SonicManifest::from_voices_music(epoch, grains, bed, music)
}

fn cue(body: &TraceBody) -> Option<(&'static str, CuePos)> {
    match body {
        TraceBody::RelAdd { a, rel, .. } | TraceBody::RelDel { a, rel, .. }
            if *rel == RelTag::LOCKED_BY || *rel == RelTag::WIELDED_BY =>
        {
            Some((KNOCK, CuePos::Locus(*a)))
        }
        TraceBody::PoseCommitted {
            pose,
            reason: PoseReason::Hinge | PoseReason::Pick | PoseReason::Drop,
            ..
        } => Some((KNOCK, CuePos::Xz(pose.translation()))),
        TraceBody::QtyChanged { id, .. } => Some((CRACKLE, CuePos::Locus(*id))),
        TraceBody::Emitted { a, .. } => Some((KNOCK, CuePos::Locus(*a))),
        _ => None,
    }
}

fn grain_occluded(pos: Option<IVec3>, eye: IVec3, hulls: &[AabbMm]) -> bool {
    let Some(pos) = pos else {
        return false;
    };
    if hulls.is_empty() {
        return false;
    }
    for h in hulls {
        if h.is_empty() || h.contains_point(pos) {
            continue;
        }
        if open_segment_hits_aabb(eye, pos, *h) {
            return true;
        }
    }
    false
}

fn open_segment_hits_aabb(a: IVec3, b: IVec3, hull: AabbMm) -> bool {
    if hull.is_empty() {
        return false;
    }
    if a.x == b.x && a.y == b.y && a.z == b.z {
        return false;
    }
    let mut t = TInterval {
        min_n: 0,
        min_d: 1,
        max_n: 1,
        max_d: 1,
    };
    if !slab_axis(a.x, b.x, hull.min.x, hull.max.x, &mut t) {
        return false;
    }
    if !slab_axis(a.y, b.y, hull.min.y, hull.max.y, &mut t) {
        return false;
    }
    if !slab_axis(a.z, b.z, hull.min.z, hull.max.z, &mut t) {
        return false;
    }
    cmp_frac(t.max_n, t.max_d, 0, 1) == Ordering::Greater
        && cmp_frac(t.min_n, t.min_d, 1, 1) == Ordering::Less
}

struct TInterval {
    min_n: i64,
    min_d: i64,
    max_n: i64,
    max_d: i64,
}

fn slab_axis(ao: i32, bo: i32, min: i32, max: i32, t: &mut TInterval) -> bool {
    let orig = i64::from(ao);
    let dest = i64::from(bo);
    let min = i64::from(min);
    let max = i64::from(max);
    let dir = dest - orig;
    if dir == 0 {
        return orig >= min && orig <= max;
    }
    let mut enter_n = min - orig;
    let mut enter_d = dir;
    let mut exit_n = max - orig;
    let mut exit_d = dir;
    if cmp_frac(enter_n, enter_d, exit_n, exit_d) == Ordering::Greater {
        core::mem::swap(&mut enter_n, &mut exit_n);
        core::mem::swap(&mut enter_d, &mut exit_d);
    }
    if cmp_frac(enter_n, enter_d, t.min_n, t.min_d) == Ordering::Greater {
        t.min_n = enter_n;
        t.min_d = enter_d;
    }
    if cmp_frac(exit_n, exit_d, t.max_n, t.max_d) == Ordering::Less {
        t.max_n = exit_n;
        t.max_d = exit_d;
    }
    cmp_frac(t.min_n, t.min_d, t.max_n, t.max_d) != Ordering::Greater
}

fn cmp_frac(mut an: i64, mut ad: i64, mut bn: i64, mut bd: i64) -> Ordering {
    if ad < 0 {
        an = -an;
        ad = -ad;
    }
    if bd < 0 {
        bn = -bn;
        bd = -bd;
    }
    (i128::from(an) * i128::from(bd)).cmp(&(i128::from(bn) * i128::from(ad)))
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, Mm, PoseMm, ResourceId, Tick, YawMd};
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

    fn tags() -> BTreeMap<String, BlobId> {
        let mut m = BTreeMap::new();
        m.insert(KNOCK.into(), blob(1));
        m.insert(CRACKLE.into(), blob(2));
        m
    }

    fn poses() -> BTreeMap<Sigil, IVec3> {
        let mut m = BTreeMap::new();
        m.insert(
            relic(1),
            IVec3 {
                x: 100,
                y: 0,
                z: 200,
            },
        );
        m
    }

    fn extract(events: &[TraceEvent], bed: Option<BedRef>) -> SonicManifest {
        let poses = poses();
        extract_sonic(
            events,
            Epoch::ZERO,
            &tags(),
            bed,
            |s| poses.get(&s).copied(),
            &[],
            Observer::origin(),
        )
    }

    fn observer_at(x: i32, y: i32, z: i32) -> Observer {
        Observer {
            eye: PoseMm::new(Mm(x), Mm(y), Mm(z), YawMd::ZERO),
            pitch_md: 0,
        }
    }

    fn hinge_at(x: i32, y: i32, z: i32) -> TraceEvent {
        TraceEvent::new(
            Tick(1),
            TraceBody::PoseCommitted {
                s: relic(1),
                pose: PoseMm::new(Mm(x), Mm(y), Mm(z), YawMd::ZERO),
                reason: PoseReason::Hinge,
            },
        )
    }

    fn extract_opaque(opaque: &[AabbMm], observer: Observer, pos: IVec3) -> GrainVoice {
        let s = extract_sonic(
            &[hinge_at(pos.x, pos.y, pos.z)],
            Epoch::ZERO,
            &tags(),
            None,
            |_| None,
            opaque,
            observer,
        );
        s.grains[0]
    }

    fn wall(z0: i32, z1: i32) -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -10,
                y: -10,
                z: z0,
            },
            IVec3 {
                x: 10,
                y: 10,
                z: z1,
            },
        )
    }

    #[test]
    fn empty_trace_has_no_one_shots() {
        let bed = Some(BedRef {
            blob: blob(9),
            gain_milli: 400,
        });
        let s = extract(&[], bed);
        assert!(s.grains.is_empty());
        assert_eq!(s.bed, bed);
    }

    #[test]
    fn lock_wield_hinge_qty_emitted_cue() {
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
                Tick(2),
                TraceBody::RelDel {
                    a,
                    rel: RelTag::WIELDED_BY,
                    b: relic(2),
                },
            ),
            TraceEvent::new(
                Tick(3),
                TraceBody::PoseCommitted {
                    s: a,
                    pose: PoseMm::new(Mm(50), Mm(0), Mm(70), YawMd::ZERO),
                    reason: PoseReason::Hinge,
                },
            ),
            TraceEvent::new(
                Tick(4),
                TraceBody::QtyChanged {
                    id: a,
                    res: ResourceId(0),
                    to: 10,
                    quantum: 10,
                },
            ),
            TraceEvent::new(
                Tick(5),
                TraceBody::Emitted {
                    kind: 1,
                    a,
                    b: None,
                },
            ),
        ];
        let s = extract(&events, None);
        assert_eq!(s.grains.len(), 5);
        assert_eq!(s.grains[0].blob, blob(1));
        assert_eq!(s.grains[0].pos.unwrap().x, 100);
        assert_eq!(s.grains[1].blob, blob(1));
        assert_eq!(s.grains[2].pos.unwrap(), IVec3 { x: 50, y: 0, z: 70 });
        assert_eq!(s.grains[3].blob, blob(2));
        assert_eq!(s.grains[4].blob, blob(1));
        assert_eq!(s.grains[4].gain_milli, 1000);
        assert!(s.grains.iter().all(|g| !g.occluded));
    }

    #[test]
    fn pick_and_drop_cue_knock() {
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
                Tick(2),
                TraceBody::PoseCommitted {
                    s: a,
                    pose: PoseMm::new(Mm(3), Mm(0), Mm(4), YawMd::ZERO),
                    reason: PoseReason::Drop,
                },
            ),
        ];
        let s = extract(&events, None);
        assert_eq!(s.grains.len(), 2);
        assert!(s.grains.iter().all(|g| g.blob == blob(1)));
    }

    #[test]
    fn never_committed_bodies_are_silent() {
        let a = relic(1);
        let events = [
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
        ];
        let s = extract(&events, None);
        assert!(s.grains.is_empty());
        assert!(s.is_silent());
    }

    #[test]
    fn missing_tag_skips_event() {
        let a = relic(1);
        let events = [TraceEvent::new(
            Tick(1),
            TraceBody::QtyChanged {
                id: a,
                res: ResourceId(0),
                to: 1,
                quantum: 1,
            },
        )];
        let s = extract_sonic(
            &events,
            Epoch::ZERO,
            &BTreeMap::new(),
            None,
            |_| None,
            &[],
            Observer::origin(),
        );
        assert!(s.grains.is_empty());
    }

    #[test]
    fn missing_pose_is_non_spatial() {
        let a = relic(9);
        let events = [TraceEvent::new(
            Tick(1),
            TraceBody::RelAdd {
                a,
                rel: RelTag::LOCKED_BY,
                b: a,
            },
        )];
        let s = extract(&events, None);
        assert_eq!(s.grains.len(), 1);
        assert_eq!(s.grains[0].pos, None);
        assert!(!s.grains[0].occluded);
    }

    #[test]
    fn empty_hulls_never_occlude() {
        let g = extract_opaque(
            &[],
            observer_at(0, 0, 0),
            IVec3 {
                x: 0,
                y: 0,
                z: 1000,
            },
        );
        assert!(!g.occluded);
    }

    #[test]
    fn missing_pos_never_occludes_even_with_hulls() {
        let a = relic(9);
        let events = [TraceEvent::new(
            Tick(1),
            TraceBody::RelAdd {
                a,
                rel: RelTag::LOCKED_BY,
                b: a,
            },
        )];
        let hull = wall(400, 600);
        let s = extract_sonic(
            &events,
            Epoch::ZERO,
            &tags(),
            None,
            |_| None,
            &[hull],
            observer_at(0, 0, 0),
        );
        assert_eq!(s.grains[0].pos, None);
        assert!(!s.grains[0].occluded);
    }

    #[test]
    fn hull_between_observer_and_grain_occludes() {
        let g = extract_opaque(
            &[wall(400, 600)],
            observer_at(0, 0, 0),
            IVec3 {
                x: 0,
                y: 0,
                z: 1000,
            },
        );
        assert!(g.occluded);
    }

    #[test]
    fn grain_inside_hull_is_door_knock_not_occluded() {
        let g = extract_opaque(
            &[wall(400, 600)],
            observer_at(0, 0, 0),
            IVec3 { x: 0, y: 0, z: 500 },
        );
        assert!(!g.occluded);
    }

    #[test]
    fn empty_aabb_never_occludes() {
        let empty = AabbMm::new(IVec3 { x: 1, y: 0, z: 400 }, IVec3 { x: 0, y: 0, z: 600 });
        assert!(empty.is_empty());
        let g = extract_opaque(
            &[empty],
            observer_at(0, 0, 0),
            IVec3 {
                x: 0,
                y: 0,
                z: 1000,
            },
        );
        assert!(!g.occluded);
    }

    #[test]
    fn face_touching_closed_aabb_still_occludes() {
        let face = AabbMm::new(
            IVec3 {
                x: -10,
                y: -10,
                z: 500,
            },
            IVec3 {
                x: 10,
                y: 10,
                z: 500,
            },
        );
        let g = extract_opaque(
            &[face],
            observer_at(0, 0, 0),
            IVec3 {
                x: 0,
                y: 0,
                z: 1000,
            },
        );
        assert!(g.occluded);
    }

    #[test]
    fn observer_inside_blocking_hull_still_occludes() {
        let hull = AabbMm::new(
            IVec3 {
                x: -100,
                y: -100,
                z: -100,
            },
            IVec3 {
                x: 100,
                y: 100,
                z: 100,
            },
        );
        let g = extract_opaque(
            &[hull],
            observer_at(0, 0, 0),
            IVec3 {
                x: 0,
                y: 0,
                z: 1000,
            },
        );
        assert!(g.occluded);
    }

    #[test]
    fn extreme_i32_coords_neither_panic_nor_spurious() {
        let eye = observer_at(i32::MIN, 0, 0);
        let pos = IVec3 {
            x: i32::MAX,
            y: 0,
            z: 0,
        };
        let empty = extract_opaque(&[], eye, pos);
        assert!(!empty.occluded);
        let off = AabbMm::new(
            IVec3 {
                x: -10,
                y: 1_000,
                z: -10,
            },
            IVec3 {
                x: 10,
                y: 2_000,
                z: 10,
            },
        );
        assert!(!extract_opaque(&[off], eye, pos).occluded);
        let between = AabbMm::new(
            IVec3 {
                x: -10,
                y: -10,
                z: -10,
            },
            IVec3 {
                x: 10,
                y: 10,
                z: 10,
            },
        );
        assert!(extract_opaque(&[between], eye, pos).occluded);
    }
}
