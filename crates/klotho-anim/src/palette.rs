//! Integer joint compose and CPU skin. GPU upload may promote to `f32`.

use klotho_core::{IVec3, Mm, PoseMm, Vel3, VelFx, rotate_xz};
use klotho_ir::Verb;

use crate::clip::ClipSet;

/// Palette / blob bone cap. `PaletteSlot::bones == 0` is identity.
pub const MAX_SKIN_BONES: usize = 256;

/// Quantized influence weights on a vertex must sum to this.
pub const WEIGHT_SUM: u32 = 65_535;

/// `(verb, grounded)` used by Motion: non-zero vel → Move, else Look.
#[must_use]
pub fn clip_verb(vel: Vel3) -> Verb {
    if vel.x != VelFx::ZERO || vel.y != VelFx::ZERO || vel.z != VelFx::ZERO {
        Verb::Move
    } else {
        Verb::Look
    }
}

/// Grounded selector: support contact or on/below y=0.
#[must_use]
pub fn clip_grounded(has_support: bool, y_mm: i32) -> bool {
    has_support || y_mm <= 0
}

/// Local joints at `tick` for the clip Motion would pick.
///
/// `None` if the table has no clip (even Look fallback). Empty slice is T-pose.
#[must_use]
pub fn sample_joints(
    clips: &ClipSet,
    vel: Vel3,
    grounded: bool,
    tick: klotho_core::Tick,
) -> Option<&[PoseMm]> {
    Some(clips.lookup(clip_verb(vel), grounded)?.sample_joints(tick))
}

/// `world = root * local` in millimetres / millidegrees (yaw about Y).
#[must_use]
pub fn compose(root: PoseMm, local: PoseMm) -> PoseMm {
    let r = rotate_xz(local.translation(), root.yaw);
    PoseMm {
        x: root.x.wrapping_add(Mm(r.x)),
        y: root.y.wrapping_add(Mm(r.y)),
        z: root.z.wrapping_add(Mm(r.z)),
        yaw: root.yaw.wrapping_add(local.yaw),
        pitch: root.pitch.wrapping_add(local.pitch),
        roll: root.roll.wrapping_add(local.roll),
    }
}

/// Apply a yaw+translate pose to a millimetre point.
#[must_use]
pub fn apply_pose(pose: PoseMm, p: IVec3) -> IVec3 {
    let r = rotate_xz(p, pose.yaw);
    IVec3 {
        x: r.x.wrapping_add(pose.x.0),
        y: r.y.wrapping_add(pose.y.0),
        z: r.z.wrapping_add(pose.z.0),
    }
}

/// 4-influence skin in millimetres. Missing / OOB joints are identity.
///
/// Weights are `u16` that should sum to [`WEIGHT_SUM`]. Zero total weight
/// returns `pos` (T-pose / rigid fallback).
#[must_use]
pub fn skin_vertex(pos: IVec3, joints: &[PoseMm], indices: [u8; 4], weights: [u16; 4]) -> IVec3 {
    let total = u32::from(weights[0])
        + u32::from(weights[1])
        + u32::from(weights[2])
        + u32::from(weights[3]);
    if total == 0 {
        return pos;
    }
    let mut x = 0i64;
    let mut y = 0i64;
    let mut z = 0i64;
    for k in 0..4 {
        let w = i64::from(weights[k]);
        if w == 0 {
            continue;
        }
        let ji = usize::from(indices[k]);
        let local = joints.get(ji).copied().unwrap_or_default();
        let p = apply_pose(local, pos);
        x += i64::from(p.x) * w;
        y += i64::from(p.y) * w;
        z += i64::from(p.z) * w;
    }
    let t = i64::from(total);
    IVec3 {
        x: (x / t) as i32,
        y: (y / t) as i32,
        z: (z / t) as i32,
    }
}

/// World-space vertex: `root * skin(local joints)`.
#[must_use]
pub fn skin_world(
    pos: IVec3,
    root: PoseMm,
    joints: &[PoseMm],
    indices: [u8; 4],
    weights: [u16; 4],
) -> IVec3 {
    apply_pose(root, skin_vertex(pos, joints, indices, weights))
}

/// Cap a joint list for a palette slot. Empty is T-pose (`bones == 0`).
#[must_use]
pub fn capped_joints(locals: &[PoseMm]) -> Vec<PoseMm> {
    locals.iter().copied().take(MAX_SKIN_BONES).collect()
}

#[cfg(test)]
mod tests {
    use klotho_core::{Tick, YawMd};

    use super::*;
    use crate::clip::{Clip, ClipSet, WALK_MM_PER_TICK};

    fn w1() -> [u16; 4] {
        [WEIGHT_SUM as u16, 0, 0, 0]
    }

    #[test]
    fn identity_palette_equals_rigid() {
        let pos = IVec3 {
            x: 100,
            y: 200,
            z: 300,
        };
        let root = PoseMm::new(Mm(10), Mm(20), Mm(30), YawMd::ZERO);
        let skinned = skin_world(pos, root, &[], [0; 4], w1());
        let rigid = apply_pose(root, pos);
        assert_eq!(skinned, rigid);
        assert_eq!(skin_vertex(pos, &[], [0; 4], w1()), pos);
        assert_eq!(skin_vertex(pos, &[], [0; 4], [0; 4]), pos);
    }

    #[test]
    fn non_identity_joint_moves_weighted_vertex() {
        let pos = IVec3 { x: 0, y: 0, z: 0 };
        let tpose = skin_vertex(pos, &[], [0; 4], w1());
        let joint = PoseMm::new(Mm(0), Mm(1000), Mm(0), YawMd::ZERO);
        let moved = skin_vertex(pos, &[joint], [0; 4], w1());
        assert_eq!(tpose, pos);
        assert_eq!(
            moved,
            IVec3 {
                x: 0,
                y: 1000,
                z: 0
            }
        );
        assert_ne!(moved, tpose);
    }

    #[test]
    fn compose_is_root_times_local() {
        let root = PoseMm::new(Mm(5), Mm(0), Mm(7), YawMd::ZERO);
        let local = PoseMm::new(Mm(1), Mm(2), Mm(3), YawMd::ZERO);
        let w = compose(root, local);
        assert_eq!(w.x, Mm(6));
        assert_eq!(w.y, Mm(2));
        assert_eq!(w.z, Mm(10));
    }

    #[test]
    fn empty_joints_sample_is_tpose() {
        let clips = ClipSet::hearth();
        let j = sample_joints(&clips, Vel3::ZERO, true, Tick(0)).unwrap();
        assert!(j.is_empty());
        let moving = Vel3::new(VelFx::ONE, VelFx::ZERO, VelFx::ZERO);
        let j = sample_joints(&clips, moving, true, Tick(0)).unwrap();
        assert!(j.is_empty());
        assert_eq!(
            clips.lookup(Verb::Move, true).unwrap().sample(Tick(0)).z,
            WALK_MM_PER_TICK
        );
    }

    #[test]
    fn missing_clip_is_none() {
        assert!(sample_joints(&ClipSet::default(), Vel3::ZERO, true, Tick(0)).is_none());
    }

    #[test]
    fn walk_clip_with_joints_samples_at_tick() {
        let joint0 = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let joint1 = PoseMm::new(Mm(0), Mm(400), Mm(0), YawMd::ZERO);
        let mut clips = ClipSet::hearth();
        clips.clips[1].joints = vec![vec![joint0], vec![joint1]];
        let moving = Vel3::new(VelFx::ONE, VelFx::ZERO, VelFx::ZERO);
        assert_eq!(
            sample_joints(&clips, moving, true, Tick(0)).unwrap(),
            &[joint0]
        );
        assert_eq!(
            sample_joints(&clips, moving, true, Tick(1)).unwrap(),
            &[joint1]
        );
        assert_eq!(
            sample_joints(&clips, moving, true, Tick(2)).unwrap(),
            &[joint0]
        );
        assert_eq!(
            clips.clips[1].sample(Tick(0)).z,
            WALK_MM_PER_TICK,
            "root channel stays hashed millimetres"
        );
    }

    #[test]
    fn oob_joint_index_is_identity() {
        let pos = IVec3 { x: 4, y: 5, z: 6 };
        let out = skin_vertex(pos, &[], [9, 0, 0, 0], w1());
        assert_eq!(out, pos);
    }

    #[test]
    fn clip_verb_matches_motion() {
        assert_eq!(clip_verb(Vel3::ZERO), Verb::Look);
        assert_eq!(
            clip_verb(Vel3::new(VelFx::ONE, VelFx::ZERO, VelFx::ZERO)),
            Verb::Move
        );
        assert!(clip_grounded(true, 100));
        assert!(clip_grounded(false, 0));
        assert!(!clip_grounded(false, 1));
    }

    #[test]
    fn unused_clip_constructs_with_joints() {
        let c = Clip {
            id: 9,
            verb: Verb::Look,
            grounded: true,
            looping: true,
            samples: vec![IVec3::ZERO],
            joints: vec![vec![PoseMm::default()]],
        };
        assert_eq!(c.sample_joints(Tick(0)).len(), 1);
    }
}
