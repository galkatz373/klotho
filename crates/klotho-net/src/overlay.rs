//! Client-only pose overlay. Never hashed; fed only by [`crate::Packet::PoseDelta`].

use std::collections::BTreeMap;

use klotho_core::{Mm, PoseMm, Sigil, YawMd};

use crate::packet::PoseBlock;

/// Chebyshev millimetre threshold for local-pawn snap-hard vs blend.
pub const DEFAULT_OVERLAY_SNAP_MM: i32 = 250;

/// Default permille used when blending a local pawn under the snap threshold.
const LOCAL_BLEND_PERMILLE: u16 = 500;

/// Interpolated poses for presentation. Discarded independently of Trace.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    current: BTreeMap<Sigil, PoseMm>,
    previous: BTreeMap<Sigil, PoseMm>,
    applied: BTreeMap<Sigil, PoseMm>,
    local: Option<Sigil>,
    snap_mm: i32,
}

impl Overlay {
    /// Empty overlay.
    #[must_use]
    pub fn new() -> Self {
        Self {
            snap_mm: DEFAULT_OVERLAY_SNAP_MM,
            ..Self::default()
        }
    }

    /// Mark `s` as the local pawn (snap/blend). Remotes interpolate only.
    pub fn set_local(&mut self, s: Option<Sigil>) {
        self.local = s;
    }

    /// Override the snap-hard Chebyshev threshold (millimetres).
    pub fn set_snap_mm(&mut self, snap_mm: i32) {
        self.snap_mm = snap_mm;
    }

    /// Map `local_ix` through `dict` and update current poses.
    ///
    /// Idle movers omitted (`n = 0`) keep the last pose. Unknown `local_ix` is
    /// ignored. Deltas apply onto the last Full-or-Delta server pose, not the
    /// blended display pose.
    pub fn apply_pose_delta(&mut self, dict: &[Sigil], block: &PoseBlock) {
        let n = match block {
            PoseBlock::Full(v) => v.len(),
            PoseBlock::Delta(v) => v.len(),
        };
        if n == 0 {
            return;
        }
        self.previous.clone_from(&self.current);
        match block {
            PoseBlock::Full(entries) => {
                for e in entries {
                    let Some(&s) = dict.get(e.local_ix as usize) else {
                        continue;
                    };
                    self.commit_server_pose(s, e.pose);
                }
            }
            PoseBlock::Delta(entries) => {
                for e in entries {
                    let Some(&s) = dict.get(e.local_ix as usize) else {
                        continue;
                    };
                    let Some(base) = self.applied.get(&s).copied() else {
                        continue;
                    };
                    let server = PoseMm {
                        x: Mm(base.x.0.wrapping_add(i32::from(e.dpose[0]))),
                        y: Mm(base.y.0.wrapping_add(i32::from(e.dpose[1]))),
                        z: Mm(base.z.0.wrapping_add(i32::from(e.dpose[2]))),
                        yaw: YawMd(base.yaw.0.wrapping_add(i32::from(e.dpose[3]))),
                        pitch: YawMd(base.pitch.0.wrapping_add(i32::from(e.dpose[4]))),
                        roll: YawMd(base.roll.0.wrapping_add(i32::from(e.dpose[5]))),
                    };
                    self.commit_server_pose(s, server);
                }
            }
        }
    }

    /// Snap-hard the local pawn to `server` when Chebyshev xyz error exceeds
    /// `snap_mm`; otherwise permille-blend (500) toward `server`.
    pub fn correct_local(&mut self, s: Sigil, server: PoseMm, snap_mm: i32) {
        let Some(cur) = self.current.get(&s).copied() else {
            self.current.insert(s, server);
            return;
        };
        if chebyshev_mm(cur, server) > snap_mm {
            self.current.insert(s, server);
        } else {
            self.current
                .insert(s, lerp_pose(cur, server, LOCAL_BLEND_PERMILLE));
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
        Some(lerp_pose(prev, cur, t_permille))
    }

    /// No current poses.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    /// Drop poses whose sigils are not in the new Interest codebook.
    pub fn retain(&mut self, keep: &[Sigil]) {
        self.current.retain(|s, _| keep.contains(s));
        self.previous.retain(|s, _| keep.contains(s));
        self.applied.retain(|s, _| keep.contains(s));
    }

    fn commit_server_pose(&mut self, s: Sigil, server: PoseMm) {
        self.applied.insert(s, server);
        if self.local == Some(s) && self.current.contains_key(&s) {
            self.correct_local(s, server, self.snap_mm);
        } else {
            self.current.insert(s, server);
        }
    }

    #[cfg(test)]
    pub(crate) fn nudge(&mut self, s: Sigil, dx: Mm) {
        if let Some(p) = self.current.get_mut(&s) {
            p.x = p.x.wrapping_add(dx);
        }
    }
}

fn chebyshev_mm(a: PoseMm, b: PoseMm) -> i32 {
    let dx = a.x.0.abs_diff(b.x.0);
    let dy = a.y.0.abs_diff(b.y.0);
    let dz = a.z.0.abs_diff(b.z.0);
    dx.max(dy).max(dz).min(i32::MAX as u32) as i32
}

fn lerp_pose(a: PoseMm, b: PoseMm, t_permille: u16) -> PoseMm {
    let t = t_permille.min(1000);
    PoseMm {
        x: Mm(lerp_i32(a.x.0, b.x.0, t)),
        y: Mm(lerp_i32(a.y.0, b.y.0, t)),
        z: Mm(lerp_i32(a.z.0, b.z.0, t)),
        yaw: YawMd(lerp_i32(a.yaw.0, b.yaw.0, t)),
        pitch: YawMd(lerp_i32(a.pitch.0, b.pitch.0, t)),
        roll: YawMd(lerp_i32(a.roll.0, b.roll.0, t)),
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
    use klotho_core::{LocusKind, Tick};
    use klotho_trace::{TraceBody, TraceEvent, fold_prefix, genesis_hash};

    use super::*;
    use crate::packet::{PoseDeltaEntry, PoseFull};

    fn actor() -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, 9).unwrap()
    }

    fn remote() -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, 10).unwrap()
    }

    fn pose_xyz(x: i32, y: i32, z: i32) -> PoseMm {
        PoseMm {
            x: Mm(x),
            y: Mm(y),
            z: Mm(z),
            yaw: YawMd(0),
            pitch: YawMd(1_000),
            roll: YawMd(2_000),
        }
    }

    fn full_block(s_ix: u16, pose: PoseMm) -> PoseBlock {
        PoseBlock::Full(vec![PoseFull {
            local_ix: s_ix,
            pose,
            vel: klotho_core::Vel3::ZERO,
        }])
    }

    #[test]
    fn overlay_nudge_does_not_change_trace_prefix() {
        let dict = [actor()];
        let a = pose_xyz(10, 50, 20);
        let b = pose_xyz(40, 50, 20);
        let events = [TraceEvent::new(Tick(1), TraceBody::SaveRequested)];
        let prefix_a = fold_prefix(genesis_hash(), &events);
        let mut overlay = Overlay::new();
        overlay.apply_pose_delta(&dict, &full_block(0, a));
        assert_eq!(overlay.pose(actor()).unwrap().x, Mm(10));
        overlay.nudge(actor(), Mm(999));
        assert_eq!(
            fold_prefix(genesis_hash(), &events),
            prefix_a,
            "overlay bytes are not a Trace input"
        );
        overlay.apply_pose_delta(&dict, &full_block(0, b));
        assert_eq!(overlay.pose(actor()).unwrap().x, Mm(40));
        overlay.apply_pose_delta(&dict, &PoseBlock::Delta(vec![]));
        assert_eq!(
            overlay.pose(actor()).unwrap().x,
            Mm(40),
            "n=0 keeps last pose"
        );
        let interp = {
            let mut o = Overlay::new();
            o.apply_pose_delta(&dict, &full_block(0, a));
            o.apply_pose_delta(&dict, &full_block(0, b));
            o.interpolate(actor(), 500).unwrap()
        };
        assert_eq!(interp.x, Mm(25));
        assert_eq!(interp.y, Mm(50));
        assert_eq!(interp.pitch, YawMd(1_000));
    }

    #[test]
    fn apply_full_then_delta_six_dof() {
        let dict = [actor()];
        let mut overlay = Overlay::new();
        overlay.apply_pose_delta(&dict, &full_block(0, pose_xyz(10, 20, 30)));
        overlay.apply_pose_delta(
            &dict,
            &PoseBlock::Delta(vec![PoseDeltaEntry {
                local_ix: 0,
                dpose: [5, 6, 7, 8, 9, 10],
            }]),
        );
        let p = overlay.pose(actor()).unwrap();
        assert_eq!(p.x, Mm(15));
        assert_eq!(p.y, Mm(26));
        assert_eq!(p.z, Mm(37));
        assert_eq!(p.yaw, YawMd(8));
        assert_eq!(p.pitch, YawMd(1_009));
        assert_eq!(p.roll, YawMd(2_010));
        let mid = overlay.interpolate(actor(), 500).unwrap();
        assert_eq!(mid.x, Mm(12));
        assert_eq!(mid.y, Mm(23));
    }

    #[test]
    fn snap_hard_when_error_exceeds_snap_mm() {
        let s = actor();
        let mut overlay = Overlay::new();
        overlay.set_local(Some(s));
        overlay.apply_pose_delta(&[s], &full_block(0, pose_xyz(0, 0, 0)));
        overlay.nudge(s, Mm(1_000));
        overlay.correct_local(s, pose_xyz(0, 0, 0), DEFAULT_OVERLAY_SNAP_MM);
        assert_eq!(overlay.pose(s).unwrap().x, Mm(0));
    }

    #[test]
    fn blend_when_error_under_snap_mm() {
        let s = actor();
        let mut overlay = Overlay::new();
        overlay.set_local(Some(s));
        overlay.apply_pose_delta(&[s], &full_block(0, pose_xyz(0, 0, 0)));
        overlay.nudge(s, Mm(100));
        overlay.correct_local(s, pose_xyz(0, 0, 0), DEFAULT_OVERLAY_SNAP_MM);
        assert_eq!(overlay.pose(s).unwrap().x, Mm(50));
    }

    #[test]
    fn remotes_are_not_snap_corrected() {
        let local = actor();
        let other = remote();
        let mut overlay = Overlay::new();
        overlay.set_local(Some(local));
        overlay.apply_pose_delta(
            &[local, other],
            &PoseBlock::Full(vec![
                PoseFull {
                    local_ix: 0,
                    pose: pose_xyz(0, 0, 0),
                    vel: klotho_core::Vel3::ZERO,
                },
                PoseFull {
                    local_ix: 1,
                    pose: pose_xyz(0, 0, 0),
                    vel: klotho_core::Vel3::ZERO,
                },
            ]),
        );
        overlay.nudge(local, Mm(100));
        overlay.nudge(other, Mm(100));
        overlay.apply_pose_delta(
            &[local, other],
            &PoseBlock::Full(vec![
                PoseFull {
                    local_ix: 0,
                    pose: pose_xyz(0, 0, 0),
                    vel: klotho_core::Vel3::ZERO,
                },
                PoseFull {
                    local_ix: 1,
                    pose: pose_xyz(0, 0, 0),
                    vel: klotho_core::Vel3::ZERO,
                },
            ]),
        );
        assert_eq!(overlay.pose(local).unwrap().x, Mm(50));
        assert_eq!(overlay.pose(other).unwrap().x, Mm(0));
    }

    #[test]
    fn unknown_local_ix_is_ignored() {
        let mut overlay = Overlay::new();
        overlay.apply_pose_delta(
            &[actor()],
            &PoseBlock::Delta(vec![PoseDeltaEntry {
                local_ix: 9,
                dpose: [1, 0, 0, 0, 0, 0],
            }]),
        );
        assert!(overlay.is_empty());
    }
}
