//! Truncate solver f32 toward −∞ at the Proposal boundary.

use klotho_core::{Mm, PoseMm, Vel3, VelFx, YawMd};

/// Residual metric name (`p99 ≤ 1 mm`, any sample `≤ 4 mm`).
pub const METRIC_QUANT_RESIDUAL_MM: &str = "klotho.phys.quant_residual_mm";

/// Count of solver bodies whose non-finite output was discarded this step.
pub const METRIC_REJECTED_NON_FINITE: &str = "klotho.phys.rejected_non_finite";

#[must_use]
pub(crate) fn trunc_mm(v: f32) -> Option<i32> {
    if !v.is_finite() {
        return None;
    }
    Some(v.floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32)
}

#[must_use]
pub(crate) fn trunc_vel(v: f32) -> Option<VelFx> {
    if !v.is_finite() {
        return None;
    }
    let scaled = v * (VelFx::SCALE as f32);
    if !scaled.is_finite() {
        return None;
    }
    Some(VelFx(
        scaled.floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32,
    ))
}

/// Quantize translation; keep integer attitude. Residual is max-axis |f32 − mm|.
#[must_use]
pub(crate) fn pose_and_residual(
    x: f32,
    y: f32,
    z: f32,
    yaw: YawMd,
    pitch: YawMd,
    roll: YawMd,
) -> Option<(PoseMm, f32)> {
    let qx = trunc_mm(x)?;
    let qy = trunc_mm(y)?;
    let qz = trunc_mm(z)?;
    let residual = (x - qx as f32)
        .abs()
        .max((y - qy as f32).abs())
        .max((z - qz as f32).abs());
    Some((
        PoseMm {
            x: Mm(qx),
            y: Mm(qy),
            z: Mm(qz),
            yaw,
            pitch,
            roll,
        },
        residual,
    ))
}

#[must_use]
pub(crate) fn vel3(x: f32, y: f32, z: f32) -> Option<Vel3> {
    Some(Vel3::new(trunc_vel(x)?, trunc_vel(y)?, trunc_vel(z)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trunc_toward_neg_inf() {
        assert_eq!(trunc_mm(1.9), Some(1));
        assert_eq!(trunc_mm(-0.1), Some(-1));
        assert_eq!(trunc_mm(0.0), Some(0));
        let neg_vel = -0.1 * (VelFx::SCALE as f32);
        assert_eq!(trunc_vel(-0.1).unwrap().0, neg_vel.floor() as i32);
        assert_eq!(trunc_mm(f32::NAN), None);
        assert_eq!(trunc_mm(f32::INFINITY), None);
        assert_eq!(trunc_mm(f32::NEG_INFINITY), None);
        assert_eq!(trunc_vel(f32::NAN), None);
        assert_eq!(trunc_vel(f32::INFINITY), None);
        assert_eq!(trunc_vel(f32::NEG_INFINITY), None);
    }
}
