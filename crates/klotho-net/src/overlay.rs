//! Client-only pose overlay. Never hashed; replaced on each TraceDelta.

use std::collections::BTreeMap;

use klotho_core::{Mm, PoseMm, Sigil, YawMd};
use klotho_trace::{TraceBody, TraceEvent};

/// Interpolated poses for remote proxies. Discarded on each [`TraceEvent`] delta.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    current: BTreeMap<Sigil, PoseMm>,
    previous: BTreeMap<Sigil, PoseMm>,
}

impl Overlay {
    /// Empty overlay.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace current poses from `PoseCommitted` / `IslandSnap` in `events`.
    /// Previous current becomes the interpolation source and is not a Trace
    /// input. A delta with neither pose event leaves overlay empty.
    pub fn apply_delta(&mut self, events: &[TraceEvent]) {
        self.previous = core::mem::take(&mut self.current);
        for e in events {
            match &e.body {
                TraceBody::PoseCommitted { s, xz, yaw, .. } => {
                    self.current.insert(
                        *s,
                        PoseMm {
                            x: xz.0,
                            z: xz.1,
                            y: Mm::ZERO,
                            yaw: *yaw,
                        },
                    );
                }
                TraceBody::IslandSnap(snap) => {
                    for (i, s) in snap.members.iter().enumerate() {
                        if let Some(p) = snap.poses.get(i) {
                            self.current.insert(*s, *p);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Current (post-delta) pose, if any.
    #[must_use]
    pub fn pose(&self, s: Sigil) -> Option<PoseMm> {
        self.current.get(&s).copied()
    }

    /// Integer permille lerp from the previous overlay sample to current.
    #[must_use]
    pub fn interpolate(&self, s: Sigil, t_permille: u16) -> Option<PoseMm> {
        let cur = self.current.get(&s).copied()?;
        let Some(prev) = self.previous.get(&s).copied() else {
            return Some(cur);
        };
        let t = t_permille.min(1000);
        Some(PoseMm {
            x: Mm(lerp_i32(prev.x.0, cur.x.0, t)),
            y: Mm(lerp_i32(prev.y.0, cur.y.0, t)),
            z: Mm(lerp_i32(prev.z.0, cur.z.0, t)),
            yaw: YawMd(lerp_i32(prev.yaw.0, cur.yaw.0, t)),
        })
    }

    /// No current poses.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn nudge(&mut self, s: Sigil, dx: Mm) {
        if let Some(p) = self.current.get_mut(&s) {
            p.x = p.x.wrapping_add(dx);
        }
    }
}

fn lerp_i32(a: i32, b: i32, t_permille: u16) -> i32 {
    let t = i64::from(t_permille);
    let a = i64::from(a);
    let b = i64::from(b);
    (a + (b - a) * t / 1000) as i32
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, Tick, VelFx};
    use klotho_trace::{IslandSnap, PoseReason, fold_prefix, genesis_hash};

    use super::*;

    fn actor() -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, 9).unwrap()
    }

    fn pose_event(x: i32) -> TraceEvent {
        TraceEvent::new(
            Tick(1),
            TraceBody::PoseCommitted {
                s: actor(),
                xz: (Mm(x), Mm(0)),
                yaw: YawMd(0),
                reason: PoseReason::Land,
            },
        )
    }

    fn snap_event(x: i32) -> TraceEvent {
        let pose = PoseMm::new(Mm(x), Mm(0), Mm(0), YawMd(0));
        TraceEvent::new(
            Tick(30),
            TraceBody::IslandSnap(
                IslandSnap::new(
                    0,
                    vec![actor()],
                    vec![pose],
                    vec![(VelFx::ZERO, VelFx::ZERO)],
                    vec![0],
                    vec![0],
                )
                .unwrap(),
            ),
        )
    }

    #[test]
    fn overlay_discarded_on_delta_not_hashed() {
        let a = pose_event(10);
        let b = pose_event(40);
        let prefix_a = fold_prefix(genesis_hash(), std::slice::from_ref(&a));
        let mut overlay = Overlay::new();
        overlay.apply_delta(std::slice::from_ref(&a));
        assert_eq!(overlay.pose(actor()).unwrap().x, Mm(10));
        overlay.nudge(actor(), Mm(999));
        assert_eq!(
            fold_prefix(genesis_hash(), std::slice::from_ref(&a)),
            prefix_a,
            "overlay bytes are not a Trace input"
        );
        overlay.apply_delta(std::slice::from_ref(&b));
        assert_eq!(overlay.pose(actor()).unwrap().x, Mm(40));
        assert_ne!(overlay.pose(actor()).unwrap().x, Mm(10));
        overlay.apply_delta(&[]);
        assert!(overlay.is_empty());
        let interp = {
            let mut o = Overlay::new();
            o.apply_delta(std::slice::from_ref(&a));
            o.apply_delta(std::slice::from_ref(&b));
            o.interpolate(actor(), 500).unwrap()
        };
        assert_eq!(interp.x, Mm(25));
    }

    #[test]
    fn island_snap_fills_overlay() {
        let snap = snap_event(40);
        let mut overlay = Overlay::new();
        overlay.apply_delta(std::slice::from_ref(&snap));
        assert_eq!(
            overlay.pose(actor()).unwrap(),
            PoseMm::new(Mm(40), Mm(0), Mm(0), YawMd(0))
        );
        overlay.apply_delta(&[]);
        assert!(overlay.is_empty());
    }
}
