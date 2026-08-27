//! Trace events → [`SonicManifest`]. Trace is the cue list.

use std::collections::BTreeMap;

use klotho_core::{BlobId, Epoch, IVec3, Sigil};
use klotho_manifest::{BedRef, GrainVoice, SonicManifest};
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
) -> SonicManifest {
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
            occluded: false,
        });
    }
    SonicManifest::from_voices(epoch, grains, bed)
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
        extract_sonic(events, Epoch::ZERO, &tags(), bed, |s| {
            poses.get(&s).copied()
        })
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
        let s = extract_sonic(&events, Epoch::ZERO, &BTreeMap::new(), None, |_| None);
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
    }
}
