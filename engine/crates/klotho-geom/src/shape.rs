//! Cooked primitive descriptions. No World.

use klotho_core::{AabbMm, IVec3, ShapeKind};

/// Consensus cap for vertices in one cooked convex hull.
pub const MAX_CONVEX_VERTICES: usize = 16;
/// Consensus cap for children in one cooked compound hull.
pub const MAX_COMPOUND_PARTS: usize = 8;

/// Why a query refused to run.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum GeomError {
    /// Empty AABB, negative radius, or overflow-prone extents.
    Malformed,
    /// Kind is not implemented in this crate revision.
    Unsupported,
}

/// Canonical primitive. Local geometry is millimetres about the locus origin.
// Inline fixed-cap storage keeps commit-path geometry allocation-free and makes
// malformed payload bounds explicit. The largest variant is intentionally the
// consensus-bounded compound rather than an indirection with allocation failure.
#[allow(clippy::large_enum_variant)]
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
    /// Bounded convex point cloud. Cook guarantees convexity and canonical order.
    Convex {
        /// Fixed storage; only the first `len` entries participate.
        vertices: [IVec3; MAX_CONVEX_VERTICES],
        /// Number of live vertices.
        len: u8,
    },
    /// Bounded union of non-compound children in canonical order.
    Compound {
        /// Fixed child storage; only the first `len` entries participate.
        parts: [CompoundPart; MAX_COMPOUND_PARTS],
        /// Number of live children.
        len: u8,
    },
}

/// One locally posed child of a compound hull.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct CompoundPart {
    /// Child geometry. Recursive compounds are deliberately impossible.
    pub shape: PrimitiveShape,
    /// Child pose relative to the locus origin.
    pub local_pose: klotho_core::PoseMm,
}

impl Default for CompoundPart {
    fn default() -> Self {
        Self {
            shape: PrimitiveShape::Sphere {
                local_center: IVec3::ZERO,
                radius_mm: 0,
            },
            local_pose: klotho_core::PoseMm::default(),
        }
    }
}

/// A non-compound shape that may appear inside a compound.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum PrimitiveShape {
    /// Oriented local box.
    OrientedBox {
        /// Inclusive local extents.
        local: AabbMm,
    },
    /// Sphere.
    Sphere {
        /// Local centre.
        local_center: IVec3,
        /// Radius.
        radius_mm: i32,
    },
    /// Y-axis capsule.
    Capsule {
        /// Local segment midpoint.
        local_center: IVec3,
        /// Radius.
        radius_mm: i32,
        /// Segment half-length.
        half_height_mm: i32,
    },
    /// Convex point cloud.
    Convex {
        /// Fixed vertex storage.
        vertices: [IVec3; MAX_CONVEX_VERTICES],
        /// Number of live vertices.
        len: u8,
    },
}

impl PrimitiveShape {
    pub(crate) const fn as_shape(self) -> Shape {
        match self {
            Self::OrientedBox { local } => Shape::OrientedBox { local },
            Self::Sphere {
                local_center,
                radius_mm,
            } => Shape::Sphere {
                local_center,
                radius_mm,
            },
            Self::Capsule {
                local_center,
                radius_mm,
                half_height_mm,
            } => Shape::Capsule {
                local_center,
                radius_mm,
                half_height_mm,
            },
            Self::Convex { vertices, len } => Shape::Convex { vertices, len },
        }
    }
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

    /// Bounded convex hull from canonically ordered vertices.
    pub fn convex(vertices: &[IVec3]) -> Result<Self, GeomError> {
        if !(4..=MAX_CONVEX_VERTICES).contains(&vertices.len()) {
            return Err(GeomError::Malformed);
        }
        let mut fixed = [IVec3::ZERO; MAX_CONVEX_VERTICES];
        fixed[..vertices.len()].copy_from_slice(vertices);
        Ok(Self::Convex {
            vertices: fixed,
            len: vertices.len() as u8,
        })
    }

    /// Bounded compound from canonically ordered, locally posed children.
    pub fn compound(parts: &[CompoundPart]) -> Result<Self, GeomError> {
        if parts.is_empty() || parts.len() > MAX_COMPOUND_PARTS {
            return Err(GeomError::Malformed);
        }
        let mut fixed = [CompoundPart::default(); MAX_COMPOUND_PARTS];
        fixed[..parts.len()].copy_from_slice(parts);
        Ok(Self::Compound {
            parts: fixed,
            len: parts.len() as u8,
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
        ShapeKind::Convex => Shape::convex(&box_vertices(local)),
        // The legacy hull binding stores one AABB. Treating it as a one-child
        // compound preserves exact geometry until the richer hull blob decoder
        // supplies multiple children.
        ShapeKind::Compound => Shape::compound(&[CompoundPart {
            shape: PrimitiveShape::OrientedBox { local },
            local_pose: klotho_core::PoseMm::default(),
        }]),
        ShapeKind::TriangleMesh | ShapeKind::Heightfield => Err(GeomError::Unsupported),
    }
}

fn box_vertices(local: AabbMm) -> [IVec3; 8] {
    [
        IVec3 {
            x: local.min.x,
            y: local.min.y,
            z: local.min.z,
        },
        IVec3 {
            x: local.min.x,
            y: local.min.y,
            z: local.max.z,
        },
        IVec3 {
            x: local.min.x,
            y: local.max.y,
            z: local.min.z,
        },
        IVec3 {
            x: local.min.x,
            y: local.max.y,
            z: local.max.z,
        },
        IVec3 {
            x: local.max.x,
            y: local.min.y,
            z: local.min.z,
        },
        IVec3 {
            x: local.max.x,
            y: local.min.y,
            z: local.max.z,
        },
        IVec3 {
            x: local.max.x,
            y: local.max.y,
            z: local.min.z,
        },
        IVec3 {
            x: local.max.x,
            y: local.max.y,
            z: local.max.z,
        },
    ]
}

fn mid(a: i32, b: i32) -> i32 {
    (i64::from(a) + i64::from(b)).div_euclid(2) as i32
}

fn half(a: i32, b: i32) -> i32 {
    (i64::from(b) - i64::from(a)).div_euclid(2).unsigned_abs() as i32
}
