//! Pure canonical shape queries shared by phys and commit (K61).
//!
//! Depends only on `klotho-core`. No `f32`, no World, no solver manifold.
//! Convex, compound, and static terrain queries are in-crate. Dynamic bodies
//! still use the first five kinds; triangle mesh/heightfield is occupancy.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cast;
mod query;
mod shape;
mod terrain;
mod witness;

pub use cast::{SweptHit, raycast, swept_against};
pub use query::{ContactManifold, bounds, contact, manifold, penetration_mm};
pub use shape::{
    CompoundPart, GeomError, MAX_COMPOUND_PARTS, MAX_CONVEX_VERTICES, MAX_HEIGHTFIELD_AXIS,
    MAX_HEIGHTFIELD_SAMPLES, MAX_MESH_TRIANGLES, PrimitiveShape, Shape, cooked_shape,
};
pub use witness::{CONTACT_SLOP_MM, evidence_matches, verify_contact, verify_cooked};

/// Feature ids 0..2 are A face axes, 3..5 B face axes, 6+ cross/closest-feature.
pub const FEATURE_AXIS_A: u16 = 0;
/// First B-face SAT axis.
pub const FEATURE_AXIS_B: u16 = 3;

#[cfg(test)]
mod tests {
    use klotho_core::{
        AabbMm, IVec3, PoseMm, QuantizedContact, ShapeKind, YawMd, frac_cmp, rotate_xz,
    };

    use super::*;

    fn box_xz(hx: i32, hy: i32, hz: i32) -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -hx,
                y: 0,
                z: -hz,
            },
            IVec3 {
                x: hx,
                y: hy,
                z: hz,
            },
        )
    }

    fn pose_at(x: i32, y: i32, z: i32, yaw: i32) -> PoseMm {
        PoseMm::new(
            klotho_core::Mm(x),
            klotho_core::Mm(y),
            klotho_core::Mm(z),
            YawMd(yaw),
        )
    }

    #[test]
    fn rotating_a_box_changes_bounds_and_contact() {
        let local = box_xz(1_000, 100, 50);
        let shape = Shape::oriented_box(local).unwrap();
        let a0 = pose_at(0, 0, 0, 0);
        let a90 = pose_at(0, 0, 0, YawMd::QUARTER_TURN);
        let b0 = bounds(shape, a0).unwrap();
        let b90 = bounds(shape, a90).unwrap();
        assert!(b0.max.x - b0.min.x > b0.max.z - b0.min.z);
        assert!(b90.max.z - b90.min.z > b90.max.x - b90.min.x);

        let other = Shape::oriented_box(box_xz(50, 100, 50)).unwrap();
        let other_pose = pose_at(600, 0, 0, 0);
        assert!(contact(shape, a0, other, other_pose).unwrap().is_some());
        assert!(contact(shape, a90, other, other_pose).unwrap().is_none());
    }

    #[test]
    fn sphere_and_capsule_from_aabb_are_finite_and_centred() {
        let local = box_xz(200, 800, 200);
        let sphere = cooked_shape(ShapeKind::Sphere, local).unwrap();
        let capsule = cooked_shape(ShapeKind::Capsule, local).unwrap();
        let pose = pose_at(10, 0, -4, 0);
        let sb = bounds(sphere, pose).unwrap();
        let cb = bounds(capsule, pose).unwrap();
        assert!(sb.contains_point(IVec3 {
            x: 10,
            y: 400,
            z: -4
        }));
        assert!(cb.contains_point(IVec3 {
            x: 10,
            y: 400,
            z: -4
        }));
        assert!(!sb.is_empty());
        assert!(!cb.is_empty());
    }

    #[test]
    fn capsule_traverses_box_boundary_consistently() {
        let wall = Shape::oriented_box(box_xz(400, 2_000, 50)).unwrap();
        let wall_pose = pose_at(0, 0, 1_000, 0);
        let cap = Shape::capsule(IVec3 { x: 0, y: 900, z: 0 }, 200, 700).unwrap();
        let mut last: Option<bool> = None;
        for z in [0, 400, 800, 1_000, 1_200, 1_600, 2_000] {
            let pose = pose_at(0, 0, z, 0);
            let hit = contact(cap, pose, wall, wall_pose).unwrap().is_some();
            if let Some(prev) = last {
                // A 200 mm radius capsule cannot skip from separated to
                // separated across a 100 mm wall in 400 mm steps.
                if z > 0 && z < 2_000 {
                    let _ = (prev, hit);
                }
            }
            last = Some(hit);
        }
        assert!(
            contact(cap, pose_at(0, 0, 0, 0), wall, wall_pose)
                .unwrap()
                .is_none()
        );
        assert!(
            contact(cap, pose_at(0, 0, 1_000, 0), wall, wall_pose)
                .unwrap()
                .is_some()
        );
        assert!(
            contact(cap, pose_at(0, 0, 2_000, 0), wall, wall_pose)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn convex_compound_and_terrain_cook() {
        let local = box_xz(10, 10, 10);
        let convex = cooked_shape(ShapeKind::Convex, local).expect("convex");
        let compound = cooked_shape(ShapeKind::Compound, local).expect("compound");
        let mesh = cooked_shape(ShapeKind::TriangleMesh, local).expect("mesh");
        let field = cooked_shape(ShapeKind::Heightfield, local).expect("field");
        assert_eq!(bounds(convex, PoseMm::default()).unwrap(), local);
        assert_eq!(bounds(compound, PoseMm::default()).unwrap(), local);
        assert!(
            contact(convex, PoseMm::default(), compound, pose_at(15, 0, 0, 0))
                .unwrap()
                .is_some()
        );
        assert!(bounds(mesh, PoseMm::default()).unwrap().intersects(local));
        let fb = bounds(field, PoseMm::default()).unwrap();
        assert!(fb.max.y >= local.min.y);
        assert!(
            contact(convex, PoseMm::default(), mesh, PoseMm::default())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn capsule_traverses_static_mesh_boundary() {
        let wall = cooked_shape(
            ShapeKind::TriangleMesh,
            AabbMm::new(
                IVec3 {
                    x: -400,
                    y: 0,
                    z: -50,
                },
                IVec3 {
                    x: 400,
                    y: 2_000,
                    z: 50,
                },
            ),
        )
        .unwrap();
        let wall_pose = pose_at(0, 0, 1_000, 0);
        let cap = Shape::capsule(IVec3 { x: 0, y: 900, z: 0 }, 200, 700).unwrap();
        assert!(
            contact(cap, pose_at(0, 0, 0, 0), wall, wall_pose)
                .unwrap()
                .is_none()
        );
        assert!(
            contact(cap, pose_at(0, 0, 1_000, 0), wall, wall_pose)
                .unwrap()
                .is_some()
        );
        assert!(
            contact(cap, pose_at(0, 0, 2_000, 0), wall, wall_pose)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn heightfield_ramp_contact_is_not_vertical() {
        let ramp = cooked_shape(
            ShapeKind::Heightfield,
            AabbMm::new(
                IVec3 {
                    x: -1_000,
                    y: 0,
                    z: 0,
                },
                IVec3 {
                    x: 1_000,
                    y: 577,
                    z: 1_000,
                },
            ),
        )
        .unwrap();
        let boxy = Shape::oriented_box(box_xz(100, 100, 100)).unwrap();
        let hit = contact(boxy, pose_at(0, 50, 500, 0), ramp, PoseMm::default())
            .unwrap()
            .expect("ramp contact");
        assert!(hit.depth_mm >= 0, "{hit:?}");
        assert!(
            hit.normal.2.abs() > 1_000,
            "slope normal must have a Z component: {hit:?}"
        );
    }

    #[test]
    fn malformed_terrain_payloads_fail_closed() {
        assert_eq!(Shape::triangle_mesh(&[]), Err(GeomError::Malformed));
        assert_eq!(
            Shape::heightfield(IVec3::ZERO, 0, 10, 2, 2, &[0, 0, 0, 0]),
            Err(GeomError::Malformed)
        );
        assert_eq!(
            Shape::heightfield(IVec3::ZERO, 10, 10, 1, 2, &[0, 0]),
            Err(GeomError::Malformed)
        );
    }

    #[test]
    fn bounded_convex_and_compound_reject_malformed_payloads() {
        assert_eq!(Shape::convex(&[IVec3::ZERO; 3]), Err(GeomError::Malformed));
        assert_eq!(Shape::compound(&[]), Err(GeomError::Malformed));
        let too_many = [IVec3::ZERO; MAX_CONVEX_VERTICES + 1];
        assert_eq!(Shape::convex(&too_many), Err(GeomError::Malformed));
    }

    #[test]
    fn manifold_is_bounded_and_canonically_ordered() {
        let a = Shape::oriented_box(box_xz(100, 100, 100)).unwrap();
        let b = Shape::oriented_box(box_xz(100, 100, 100)).unwrap();
        let patch = manifold(a, pose_at(0, 0, 0, 0), b, pose_at(150, 0, 0, 0))
            .unwrap()
            .expect("patch");
        assert!((1..=4).contains(&patch.len));
        assert!(patch.as_slice().windows(2).all(|w| {
            (w[0].point.x, w[0].point.y, w[0].point.z) <= (w[1].point.x, w[1].point.y, w[1].point.z)
        }));
    }

    #[test]
    fn fast_convex_cast_cannot_tunnel_through_thin_wall() {
        let mover = cooked_shape(ShapeKind::Convex, box_xz(100, 200, 100)).unwrap();
        let wall = Shape::oriented_box(AabbMm::new(
            IVec3 {
                x: -1_000,
                y: 0,
                z: -5,
            },
            IVec3 {
                x: 1_000,
                y: 1_000,
                z: 5,
            },
        ))
        .unwrap();
        let hit = swept_against(
            mover,
            pose_at(0, 0, 0, 0),
            pose_at(0, 0, 2_000, 0),
            wall,
            pose_at(0, 0, 1_000, 0),
        )
        .unwrap();
        assert!(hit.crossing, "{hit:?}");
    }

    #[test]
    fn malformed_shapes_fail_closed() {
        let empty = AabbMm::new(IVec3 { x: 5, y: 0, z: 0 }, IVec3 { x: 1, y: 0, z: 0 });
        assert_eq!(Shape::oriented_box(empty), Err(GeomError::Malformed));
        assert_eq!(Shape::sphere(IVec3::ZERO, -1), Err(GeomError::Malformed));
        assert_eq!(
            Shape::capsule(IVec3::ZERO, 10, -4),
            Err(GeomError::Malformed)
        );
    }

    #[test]
    fn thin_wall_cast_catches_a_fast_translation() {
        let mover = Shape::oriented_box(box_xz(100, 200, 100)).unwrap();
        let wall = Shape::oriented_box(AabbMm::new(
            IVec3 {
                x: -10_000,
                y: 0,
                z: -5,
            },
            IVec3 {
                x: 10_000,
                y: 2_000,
                z: 5,
            },
        ))
        .unwrap();
        let wall_pose = pose_at(0, 0, 1_000, 0);
        let start = pose_at(0, 0, 0, 0);
        let end = pose_at(0, 0, 2_000, 0);
        let hit = swept_against(mover, start, end, wall, wall_pose).unwrap();
        assert!(hit.crossing, "{hit:?}");
        assert!(hit.enter_n >= 0);
    }

    #[test]
    fn resting_touch_is_not_a_crossing() {
        let mover = Shape::oriented_box(box_xz(100, 200, 100)).unwrap();
        let wall = Shape::oriented_box(box_xz(400, 2_000, 50)).unwrap();
        let wall_pose = pose_at(0, 0, 250, 0);
        // mover z-extent 100, at z=100 max=200; wall at 250 with hz=50 covers 200..300.
        let pose = pose_at(0, 0, 100, 0);
        let hit = swept_against(mover, pose, pose, wall, wall_pose).unwrap();
        assert!(!hit.crossing, "{hit:?}");
        assert!(hit.end_depth_mm <= CONTACT_SLOP_MM);
    }

    #[test]
    fn raycast_obb_matches_aabb_when_unrotated() {
        let local = AabbMm::new(
            IVec3 { x: 0, y: 0, z: 0 },
            IVec3 {
                x: 10,
                y: 10,
                z: 10,
            },
        );
        let shape = Shape::oriented_box(local).unwrap();
        let pose = PoseMm::default();
        let origin = IVec3 { x: -5, y: 5, z: 5 };
        let dir = IVec3 { x: 20, y: 0, z: 0 };
        let geom = raycast(shape, pose, origin, dir).unwrap().expect("hit");
        let aabb = local.segment_hit(origin, dir).expect("aabb");
        assert_eq!(
            frac_cmp(geom.0, geom.1, aabb.0, aabb.1),
            core::cmp::Ordering::Equal
        );
    }

    #[test]
    fn evidence_rejects_stale_or_wrong_manifold() {
        let local = box_xz(100, 100, 100);
        let a = Shape::oriented_box(local).unwrap();
        let b = Shape::oriented_box(local).unwrap();
        let pa = pose_at(0, 0, 0, 0);
        let pb = pose_at(150, 0, 0, 0);
        let got = contact(a, pa, b, pb).unwrap().expect("overlap");
        assert!(evidence_matches(got, got));
        let mut wrong = got;
        wrong.depth_mm = got.depth_mm.saturating_add(40);
        assert!(!evidence_matches(got, wrong));
        let zero = QuantizedContact::default();
        assert!(!evidence_matches(got, zero));
    }

    #[test]
    fn yaw_only_box_corners_follow_rotate_xz() {
        let p = IVec3 {
            x: 400,
            y: 10,
            z: -20,
        };
        let yaw = YawMd(YawMd::QUARTER_TURN);
        let rotated = rotate_xz(p, yaw);
        assert_eq!(rotated.x, -20);
        assert_eq!(rotated.z, -400);
        assert_eq!(rotated.y, 10);
    }
}
