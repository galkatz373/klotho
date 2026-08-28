//! Truncate solver f32 toward −∞ at the Proposal boundary.

use klotho_core::{Mm, PoseMm, Vel3, VelFx, YawMd};

/// Residual metric name (`p99 ≤ 1 mm`, any sample `≤ 4 mm`).
pub const METRIC_QUANT_RESIDUAL_MM: &str = "klotho.phys.quant_residual_mm";

#[must_use]
pub(crate) fn trunc_mm(v: f32) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    v.floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

#[must_use]
pub(crate) fn trunc_vel(v: f32) -> VelFx {
    if !v.is_finite() {
        return VelFx::ZERO;
    }
    let scaled = v * (VelFx::SCALE as f32);
    VelFx(scaled.floor().clamp(i32::MIN as f32, i32::MAX as f32) as i32)
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
) -> (PoseMm, f32) {
    let qx = trunc_mm(x);
    let qy = trunc_mm(y);
    let qz = trunc_mm(z);
    let residual = (x - qx as f32)
        .abs()
        .max((y - qy as f32).abs())
        .max((z - qz as f32).abs());
    (
        PoseMm {
            x: Mm(qx),
            y: Mm(qy),
            z: Mm(qz),
            yaw,
            pitch,
            roll,
        },
        residual,
    )
}

#[must_use]
pub(crate) fn vel3(x: f32, y: f32, z: f32) -> Vel3 {
    Vel3::new(trunc_vel(x), trunc_vel(y), trunc_vel(z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trunc_toward_neg_inf() {
        assert_eq!(trunc_mm(1.9), 1);
        assert_eq!(trunc_mm(-0.1), -1);
        assert_eq!(trunc_mm(0.0), 0);
    }
}
