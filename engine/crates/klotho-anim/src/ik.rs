//! Presentation-only look-at / IK. Mutates a palette copy, never Projection.

use klotho_core::{PoseMm, YawMd};

/// Yaw joint `index` on a copy of `joints`. OOB index returns a copy unchanged.
#[must_use]
pub fn look_at_yaw(joints: &[PoseMm], index: usize, yaw: YawMd) -> Vec<PoseMm> {
    let mut out = joints.to_vec();
    if let Some(j) = out.get_mut(index) {
        j.yaw = j.yaw.wrapping_add(yaw);
    }
    out
}

#[cfg(test)]
mod tests {
    use klotho_core::{IVec3, Mm, Tick, Vel3, VelFx};

    use super::*;
    use crate::clip::ClipSet;
    use crate::palette::sample_joints;

    #[test]
    fn look_at_does_not_change_hashed_root() {
        let clips = ClipSet::hearth();
        let clip = clips.lookup(klotho_ir::Verb::Move, true).unwrap();
        let root = clip.sample(Tick(0));
        let joints = vec![
            PoseMm::new(Mm(0), Mm(1800), Mm(0), YawMd::ZERO),
            PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
        ];
        let looked = look_at_yaw(&joints, 0, YawMd(45_000));
        assert_eq!(clip.sample(Tick(0)), root);
        assert_eq!(root, IVec3 { x: 0, y: 0, z: 20 });
        assert_eq!(looked[0].yaw, YawMd(45_000));
        assert_eq!(looked[1], joints[1]);
        assert_eq!(joints[0].yaw, YawMd::ZERO);
        let moving = Vel3::new(VelFx::ONE, VelFx::ZERO, VelFx::ZERO);
        assert!(
            sample_joints(&clips, moving, true, Tick(0))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn look_at_oob_is_copy() {
        let joints = vec![PoseMm::new(Mm(1), Mm(0), Mm(0), YawMd::ZERO)];
        let out = look_at_yaw(&joints, 4, YawMd(90_000));
        assert_eq!(out, joints);
    }
}
