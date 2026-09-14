//! Cooked primitive descriptions. No World.

use klotho_core::{AabbMm, IVec3, ShapeKind};

/// Why a query refused to run.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum GeomError {
    /// Empty AABB, negative radius, or overflow-prone extents.
    Malformed,
    /// Kind is not implemented in this crate revision.
    Unsupported,
}

/// Canonical primitive. Local geometry is millimetres about the locus origin.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Shape {
    /// Local AABB rotated by the pose attitude.
    OrientedBox {
        /// Inclusive local extents.
        local: AabbMm,
    },
    /// Sphere about a local centre.
    Sphere {
        /// Local centre, millimetres.
        local_center: IVec3,
        /// Radius, millimetres.
        radius_mm: i32,
    },
    /// Capsule whose segment is local Y through `local_center`.
    Capsule {
        /// Local segment midpoint.
        local_center: IVec3,
        /// Radius, millimetres.
        radius_mm: i32,
        /// Half-length of the segment, millimetres.
        half_height_mm: i32,
    },
}

impl Shape {
    /// Oriented box from a non-empty local AABB.
    pub fn oriented_box(local: AabbMm) -> Result<Self, GeomError> {
        if local.is_empty() {
            return Err(GeomError::Malformed);
        }
        Ok(Self::OrientedBox { local })
    }

    /// Sphere with a non-negative radius.
    pub fn sphere(local_center: IVec3, radius_mm: i32) -> Result<Self, GeomError> {
        if radius_mm < 0 {
            return Err(GeomError::Malformed);
        }
        Ok(Self::Sphere {
            local_center,
            radius_mm,
        })
    }

    /// Y-axis capsule with non-negative radius and half-height.
    pub fn capsule(
        local_center: IVec3,
        radius_mm: i32,
        half_height_mm: i32,
    ) -> Result<Self, GeomError> {
        if radius_mm < 0 || half_height_mm < 0 {
            return Err(GeomError::Malformed);
        }
        Ok(Self::Capsule {
            local_center,
            radius_mm,
            half_height_mm,
        })
    }
}

/// Interpret a cooked hull AABB as a PHYS-A03 primitive. Later kinds fail closed.
pub fn cooked_shape(kind: ShapeKind, local: AabbMm) -> Result<Shape, GeomError> {
    if local.is_empty() {
        return Err(GeomError::Malformed);
    }
    let centre = IVec3 {
        x: mid(local.min.x, local.max.x),
        y: mid(local.min.y, local.max.y),
        z: mid(local.min.z, local.max.z),
    };
    let hx = half(local.min.x, local.max.x);
    let hy = half(local.min.y, local.max.y);
    let hz = half(local.min.z, local.max.z);
    match kind {
        ShapeKind::OrientedBox => Shape::oriented_box(local),
        ShapeKind::Sphere => Shape::sphere(centre, hx.max(hy).max(hz)),
        ShapeKind::Capsule => {
            let radius = if hx < hz { hx } else { hz };
            let half_height = hy.saturating_sub(radius).max(0);
            Shape::capsule(centre, radius, half_height)
        }
        ShapeKind::Convex
        | ShapeKind::Compound
        | ShapeKind::TriangleMesh
        | ShapeKind::Heightfield => Err(GeomError::Unsupported),
    }
}

fn mid(a: i32, b: i32) -> i32 {
    (i64::from(a) + i64::from(b)).div_euclid(2) as i32
}

fn half(a: i32, b: i32) -> i32 {
    (i64::from(b) - i64::from(a)).div_euclid(2).unsigned_abs() as i32
}
