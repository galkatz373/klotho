//! Cooked primitive descriptions. No World.

use klotho_core::{AabbMm, IVec3, ShapeKind};

/// Consensus cap for vertices in one cooked convex hull.
pub const MAX_CONVEX_VERTICES: usize = 16;
/// Consensus cap for children in one cooked compound hull.
pub const MAX_COMPOUND_PARTS: usize = 8;
/// Consensus cap for triangles in one static mesh.
pub const MAX_MESH_TRIANGLES: usize = 16;
/// Consensus cap for samples along one heightfield axis (n×n grid).
pub const MAX_HEIGHTFIELD_AXIS: usize = 8;
/// Consensus cap for heightfield samples.
pub const MAX_HEIGHTFIELD_SAMPLES: usize = MAX_HEIGHTFIELD_AXIS * MAX_HEIGHTFIELD_AXIS;

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
    /// Static triangle mesh occupancy. Never a dynamic body.
    TriangleMesh {
        /// Triangle vertices in canonical order; only the first `len` participate.
        tris: [[IVec3; 3]; MAX_MESH_TRIANGLES],
        /// Number of live triangles.
        len: u8,
    },
    /// Static heightfield occupancy. Samples are local Y over a regular XZ grid.
    Heightfield {
        /// Local origin of sample (0, 0).
        origin: IVec3,
        /// Cell size along local X, millimetres.
        cell_x_mm: i32,
        /// Cell size along local Z, millimetres.
        cell_z_mm: i32,
        /// Sample count along local X, 2..=MAX_HEIGHTFIELD_AXIS.
        nx: u8,
        /// Sample count along local Z, 2..=MAX_HEIGHTFIELD_AXIS.
        nz: u8,
        /// Row-major heights; live count is `nx * nz`.
        samples: [i32; MAX_HEIGHTFIELD_SAMPLES],
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

    /// Bounded static triangle mesh.
    pub fn triangle_mesh(tris: &[[IVec3; 3]]) -> Result<Self, GeomError> {
        if tris.is_empty() || tris.len() > MAX_MESH_TRIANGLES {
            return Err(GeomError::Malformed);
        }
        let mut fixed = [[IVec3::ZERO; 3]; MAX_MESH_TRIANGLES];
        fixed[..tris.len()].copy_from_slice(tris);
        Ok(Self::TriangleMesh {
            tris: fixed,
            len: tris.len() as u8,
        })
    }

    /// Bounded static heightfield. `samples` is row-major `nz` rows of `nx`.
    pub fn heightfield(
        origin: IVec3,
        cell_x_mm: i32,
        cell_z_mm: i32,
        nx: u8,
        nz: u8,
        samples: &[i32],
    ) -> Result<Self, GeomError> {
        let nx_n = usize::from(nx);
        let nz_n = usize::from(nz);
        if cell_x_mm <= 0
            || cell_z_mm <= 0
            || !(2..=MAX_HEIGHTFIELD_AXIS).contains(&nx_n)
            || !(2..=MAX_HEIGHTFIELD_AXIS).contains(&nz_n)
            || samples.len() != nx_n.saturating_mul(nz_n)
        {
            return Err(GeomError::Malformed);
        }
        let mut fixed = [0i32; MAX_HEIGHTFIELD_SAMPLES];
        fixed[..samples.len()].copy_from_slice(samples);
        Ok(Self::Heightfield {
            origin,
            cell_x_mm,
            cell_z_mm,
            nx,
            nz,
            samples: fixed,
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
        ShapeKind::TriangleMesh => Shape::triangle_mesh(&box_triangles(local)),
        ShapeKind::Heightfield => ramp_heightfield(local),
    }
}

fn box_triangles(local: AabbMm) -> [[IVec3; 3]; 12] {
    let v = box_vertices(local);
    // 0: min,min,min  1: min,min,max  2: min,max,min  3: min,max,max
    // 4: max,min,min  5: max,min,max  6: max,max,min  7: max,max,max
    [
        [v[0], v[2], v[6]],
        [v[0], v[6], v[4]],
        [v[1], v[5], v[7]],
        [v[1], v[7], v[3]],
        [v[0], v[1], v[3]],
        [v[0], v[3], v[2]],
        [v[4], v[6], v[7]],
        [v[4], v[7], v[5]],
        [v[0], v[4], v[5]],
        [v[0], v[5], v[1]],
        [v[2], v[3], v[7]],
        [v[2], v[7], v[6]],
    ]
}

fn ramp_heightfield(local: AabbMm) -> Result<Shape, GeomError> {
    let cell_x = (i64::from(local.max.x) - i64::from(local.min.x))
        .unsigned_abs()
        .max(1) as i32;
    let cell_z = (i64::from(local.max.z) - i64::from(local.min.z))
        .unsigned_abs()
        .max(1) as i32;
    Shape::heightfield(
        IVec3 {
            x: local.min.x,
            y: 0,
            z: local.min.z,
        },
        cell_x,
        cell_z,
        2,
        2,
        &[local.min.y, local.min.y, local.max.y, local.max.y],
    )
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
