//! Kernel reproduction of quantized contact evidence (K24 / K62).

use klotho_core::{Epoch, HullWitness, PoseMm, QuantizedContact, ShapeKind};

use crate::query::contact;
use crate::shape::{GeomError, Shape, cooked_shape};

/// Contact slop from K62, millimetres.
pub const CONTACT_SLOP_MM: i32 = 2;

/// Point Chebyshev tolerance when reproducing a claimed contact, millimetres.
const POINT_SLOP_MM: i32 = 4;
/// Packed-normal component tolerance (32767 scale).
const NORMAL_SLOP: i16 = 2_048;

/// True when `claimed` is a kernel-reproducible copy of `computed`.
#[must_use]
pub fn evidence_matches(computed: QuantizedContact, claimed: QuantizedContact) -> bool {
    if claimed.normal == (0, 0, 0) && claimed.depth_mm > 0 {
        return false;
    }
    if (claimed.depth_mm - computed.depth_mm).abs() > CONTACT_SLOP_MM {
        return false;
    }
    if chebyshev(claimed.point, computed.point) > POINT_SLOP_MM {
        return false;
    }
    if claimed.normal.0.abs_diff(computed.normal.0) > NORMAL_SLOP.unsigned_abs()
        || claimed.normal.1.abs_diff(computed.normal.1) > NORMAL_SLOP.unsigned_abs()
        || claimed.normal.2.abs_diff(computed.normal.2) > NORMAL_SLOP.unsigned_abs()
    {
        return false;
    }
    let dot = i32::from(claimed.normal.0) * i32::from(computed.normal.0)
        + i32::from(claimed.normal.1) * i32::from(computed.normal.1)
        + i32::from(claimed.normal.2) * i32::from(computed.normal.2);
    dot >= 0
}

/// Reproduce a gameplay contact named by `witness` between `a` and `b`.
pub fn verify_contact(
    witness: HullWitness,
    live_epoch: Epoch,
    a: Shape,
    pose_a: PoseMm,
    b: Shape,
    pose_b: PoseMm,
) -> Result<QuantizedContact, GeomError> {
    if witness.epoch != live_epoch {
        return Err(GeomError::Malformed);
    }
    if !witness.shape.is_dynamic() {
        return Err(GeomError::Unsupported);
    }
    let Some(claimed) = witness.evidence else {
        return Err(GeomError::Malformed);
    };
    let Some(computed) = contact(a, pose_a, b, pose_b)? else {
        return Err(GeomError::Malformed);
    };
    if !evidence_matches(computed, claimed) {
        return Err(GeomError::Malformed);
    }
    Ok(computed)
}

/// Cook both hulls and verify. Used by commit when only AABB extents are stored.
#[allow(clippy::too_many_arguments)]
pub fn verify_cooked(
    witness: HullWitness,
    live_epoch: Epoch,
    kind_a: ShapeKind,
    local_a: klotho_core::AabbMm,
    pose_a: PoseMm,
    kind_b: ShapeKind,
    local_b: klotho_core::AabbMm,
    pose_b: PoseMm,
) -> Result<QuantizedContact, GeomError> {
    let a = cooked_shape(kind_a, local_a)?;
    let b = cooked_shape(kind_b, local_b)?;
    verify_contact(witness, live_epoch, a, pose_a, b, pose_b)
}

fn chebyshev(a: klotho_core::IVec3, b: klotho_core::IVec3) -> i32 {
    (a.x.abs_diff(b.x))
        .max(a.y.abs_diff(b.y))
        .max(a.z.abs_diff(b.z)) as i32
}
