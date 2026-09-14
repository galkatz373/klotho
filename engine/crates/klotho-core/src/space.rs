//! Integer space types the kernel can check without linking `klotho-space`.

use serde::{Deserialize, Serialize};

use crate::{Mm, Sigil, VelFx, YawMd};

/// Integer 3-vector in millimetres. Y is height.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct IVec3 {
    /// X, millimetres.
    pub x: i32,
    /// Y (height), millimetres.
    pub y: i32,
    /// Z, millimetres.
    pub z: i32,
}

impl IVec3 {
    /// Origin.
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };

    /// Component-wise wrapping add.
    #[must_use]
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self {
            x: self.x.wrapping_add(rhs.x),
            y: self.y.wrapping_add(rhs.y),
            z: self.z.wrapping_add(rhs.z),
        }
    }

    /// Component-wise min.
    #[must_use]
    pub const fn min(self, rhs: Self) -> Self {
        Self {
            x: min_i32(self.x, rhs.x),
            y: min_i32(self.y, rhs.y),
            z: min_i32(self.z, rhs.z),
        }
    }

    /// Component-wise max.
    #[must_use]
    pub const fn max(self, rhs: Self) -> Self {
        Self {
            x: max_i32(self.x, rhs.x),
            y: max_i32(self.y, rhs.y),
            z: max_i32(self.z, rhs.z),
        }
    }
}

/// Contact support from an admitted physical island: `(nx, ny, nz, depth_mm)`.
pub type Support = (i16, i16, i16, i32);

/// Linear/angular request written by `PHYS_REQ`. Not a quantity row.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct PhysRequest {
    /// One-shot linear Δv, millimetres per tick. Cleared when a physical island admits.
    pub lin: IVec3,
    /// Angular request, millidegrees.
    pub ang: IVec3,
}

const fn min_i32(a: i32, b: i32) -> i32 {
    if a < b { a } else { b }
}

const fn max_i32(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

/// Axis-aligned box in millimetres. Closed on all faces (inclusive min and max).
///
/// Empty iff `min` exceeds `max` on any axis. Edge-touching boxes overlap
/// (a locked door's face still blocks).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct AabbMm {
    /// Inclusive minimum corner.
    pub min: IVec3,
    /// Inclusive maximum corner.
    pub max: IVec3,
}

impl AabbMm {
    /// Construct without normalizing. Prefer [`Self::sorted`] for authored data.
    #[must_use]
    pub const fn new(min: IVec3, max: IVec3) -> Self {
        Self { min, max }
    }

    /// Sort corners so `min` ≤ `max` per axis.
    #[must_use]
    pub const fn sorted(a: IVec3, b: IVec3) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// Degenerate point box.
    #[must_use]
    pub const fn from_point(p: IVec3) -> Self {
        Self { min: p, max: p }
    }

    /// `true` if `min` exceeds `max` on any axis.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    /// Closed overlap, including shared faces. Empty boxes never overlap.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
            && self.min.z <= other.max.z
            && other.min.z <= self.max.z
    }

    /// Closed containment of a point.
    #[must_use]
    pub const fn contains_point(self, p: IVec3) -> bool {
        if self.is_empty() {
            return false;
        }
        self.min.x <= p.x
            && p.x <= self.max.x
            && self.min.y <= p.y
            && p.y <= self.max.y
            && self.min.z <= p.z
            && p.z <= self.max.z
    }

    /// Inclusive union. Empty operands are dropped.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => self,
            (true, false) => other,
            (false, true) => self,
            (false, false) => Self {
                min: self.min.min(other.min),
                max: self.max.max(other.max),
            },
        }
    }

    /// Conservative AABB covering `self` and `other` (swept stand-in).
    #[must_use]
    pub const fn swept_union(self, other: Self) -> Self {
        self.union(other)
    }

    /// First hit of the closed segment `origin` → `origin+dir`, as `(t_num, t_den)`
    /// with `t` in `[0, 1]` and `t_den > 0`. `None` if the segment misses.
    #[must_use]
    pub fn segment_hit(self, origin: IVec3, dir: IVec3) -> Option<(i64, i64)> {
        if self.is_empty() {
            return None;
        }
        if dir == IVec3::ZERO {
            return self.contains_point(origin).then_some((0, 1));
        }
        let mut t = Slab {
            tmin_n: 0,
            tmin_d: 1,
            tmax_n: 1,
            tmax_d: 1,
        };
        if !clip_axis(origin.x, dir.x, self.min.x, self.max.x, &mut t)
            || !clip_axis(origin.y, dir.y, self.min.y, self.max.y, &mut t)
            || !clip_axis(origin.z, dir.z, self.min.z, self.max.z, &mut t)
        {
            return None;
        }
        if frac_cmp(t.tmin_n, t.tmin_d, t.tmax_n, t.tmax_d) == core::cmp::Ordering::Greater {
            return None;
        }
        if frac_cmp(t.tmin_n, t.tmin_d, 1, 1) == core::cmp::Ordering::Greater {
            return None;
        }
        if frac_cmp(t.tmax_n, t.tmax_d, 0, 1) == core::cmp::Ordering::Less {
            return None;
        }
        Some((t.tmin_n, t.tmin_d))
    }
}

struct Slab {
    tmin_n: i64,
    tmin_d: i64,
    tmax_n: i64,
    tmax_d: i64,
}

fn clip_axis(origin: i32, dir: i32, min: i32, max: i32, t: &mut Slab) -> bool {
    if dir == 0 {
        return origin >= min && origin <= max;
    }
    let (enter_b, exit_b) = if dir > 0 { (min, max) } else { (max, min) };
    let (en, ed) = pos_den((enter_b as i64) - i64::from(origin), i64::from(dir));
    let (xn, xd) = pos_den((exit_b as i64) - i64::from(origin), i64::from(dir));
    if frac_cmp(en, ed, t.tmin_n, t.tmin_d) == core::cmp::Ordering::Greater {
        t.tmin_n = en;
        t.tmin_d = ed;
    }
    if frac_cmp(xn, xd, t.tmax_n, t.tmax_d) == core::cmp::Ordering::Less {
        t.tmax_n = xn;
        t.tmax_d = xd;
    }
    true
}

fn pos_den(n: i64, d: i64) -> (i64, i64) {
    if d < 0 { (-n, -d) } else { (n, d) }
}

/// Compare `an/ad` and `bn/bd` with positive denominators.
#[must_use]
pub fn frac_cmp(an: i64, ad: i64, bn: i64, bd: i64) -> core::cmp::Ordering {
    (an as i128 * bd as i128).cmp(&(bn as i128 * ad as i128))
}

/// Integer 3-velocity in 16.16 millimetres per tick (K20).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct Vel3 {
    /// X, 16.16 mm / tick.
    pub x: VelFx,
    /// Y (height), 16.16 mm / tick.
    pub y: VelFx,
    /// Z, 16.16 mm / tick.
    pub z: VelFx,
}

impl Vel3 {
    /// Zero on every axis.
    pub const ZERO: Self = Self {
        x: VelFx::ZERO,
        y: VelFx::ZERO,
        z: VelFx::ZERO,
    };

    /// Construct from axis components.
    #[must_use]
    pub const fn new(x: VelFx, y: VelFx, z: VelFx) -> Self {
        Self { x, y, z }
    }
}

/// Committed pose. Ground plane is XZ; Y is height. Angles are millidegrees.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct PoseMm {
    /// X, millimetres.
    pub x: Mm,
    /// Y (height), millimetres.
    pub y: Mm,
    /// Z, millimetres (forward in Hearth's default basis).
    pub z: Mm,
    /// Yaw about Y, millidegrees, normalized by callers that care.
    pub yaw: YawMd,
    /// Pitch about X, millidegrees.
    #[serde(default)]
    pub pitch: YawMd,
    /// Roll about Z, millidegrees.
    #[serde(default)]
    pub roll: YawMd,
}

impl PoseMm {
    /// Construct from millimetre translation and yaw. Pitch and roll are zero.
    #[must_use]
    pub const fn new(x: Mm, y: Mm, z: Mm, yaw: YawMd) -> Self {
        Self {
            x,
            y,
            z,
            yaw,
            pitch: YawMd::ZERO,
            roll: YawMd::ZERO,
        }
    }

    /// Integer millimetre translation of the origin of this pose.
    #[must_use]
    pub const fn translation(self) -> IVec3 {
        IVec3 {
            x: self.x.0,
            y: self.y.0,
            z: self.z.0,
        }
    }
}

/// Collision witness a proposer attaches to a `SpaceDelta` / `MotionDelta`.
///
/// The kernel **ignores any proposer-supplied swept volume** (K24). It derives
/// `swept = conservative_aabb(prev_pose, proposed, hull(mover, epoch))` and
/// rechecks `OpaqueClosed`. `overlaps_closed_opaque` is a hint: if it says no
/// overlap and the kernel finds one, the reject is
/// [`crate::RejectReason::WitnessMismatch`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct HullWitness {
    /// Locus whose hull is being moved.
    pub mover: Sigil,
    /// Where the proposer wants to go. Kernel derives swept from this.
    pub proposed: PoseMm,
    /// Proposer hint only. Kernel recomputes overlap against `OpaqueClosed`.
    pub overlaps_closed_opaque: bool,
}

impl HullWitness {
    /// Construct a witness. `overlaps_closed_opaque` is the K21 hint.
    #[must_use]
    pub const fn new(mover: Sigil, proposed: PoseMm, overlaps_closed_opaque: bool) -> Self {
        Self {
            mover,
            proposed,
            overlaps_closed_opaque,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocusKind;

    #[test]
    fn closed_face_touch_counts_as_overlap() {
        let a = AabbMm::new(
            IVec3 { x: 0, y: 0, z: 0 },
            IVec3 {
                x: 10,
                y: 10,
                z: 10,
            },
        );
        let b = AabbMm::new(
            IVec3 { x: 10, y: 0, z: 0 },
            IVec3 {
                x: 20,
                y: 10,
                z: 10,
            },
        );
        assert!(a.intersects(b));
        assert!(a.contains_point(IVec3 { x: 10, y: 0, z: 0 }));
    }

    #[test]
    fn empty_aabb_never_intersects() {
        let empty = AabbMm::new(IVec3 { x: 5, y: 0, z: 0 }, IVec3 { x: 1, y: 0, z: 0 });
        let point = AabbMm::from_point(IVec3 { x: 5, y: 0, z: 0 });
        assert!(empty.is_empty());
        assert!(!empty.intersects(point));
        assert!(!empty.contains_point(IVec3 { x: 5, y: 0, z: 0 }));
        assert!(
            empty
                .segment_hit(IVec3 { x: 5, y: 0, z: 0 }, IVec3::ZERO)
                .is_none()
        );
        assert!(
            empty
                .segment_hit(IVec3 { x: 0, y: 0, z: 0 }, IVec3 { x: 10, y: 0, z: 0 })
                .is_none()
        );
    }

    #[test]
    fn swept_union_covers_both() {
        let a = AabbMm::from_point(IVec3 { x: 0, y: 0, z: 0 });
        let b = AabbMm::from_point(IVec3 { x: 4, y: 2, z: -1 });
        let u = a.swept_union(b);
        assert!(u.contains_point(IVec3 { x: 0, y: 0, z: 0 }));
        assert!(u.contains_point(IVec3 { x: 4, y: 2, z: -1 }));
        assert!(u.contains_point(IVec3 { x: 2, y: 1, z: -1 }));
    }

    #[test]
    fn pose_new_assigns_y_as_height() {
        let p = PoseMm::new(Mm(1), Mm(2), Mm(3), YawMd(4));
        assert_eq!(p.x, Mm(1));
        assert_eq!(p.y, Mm(2));
        assert_eq!(p.z, Mm(3));
        assert_eq!(p.yaw, YawMd(4));
        assert_eq!(p.pitch, YawMd::ZERO);
        assert_eq!(p.roll, YawMd::ZERO);
        assert_eq!(p.translation(), IVec3 { x: 1, y: 2, z: 3 });
    }

    #[test]
    fn hull_witness_stores_hint() {
        let mover = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        let w = HullWitness::new(mover, PoseMm::default(), true);
        assert!(w.overlaps_closed_opaque);
        assert_eq!(w.mover, mover);
    }

    fn unit_box() -> AabbMm {
        AabbMm::new(
            IVec3 { x: 0, y: 0, z: 0 },
            IVec3 {
                x: 10,
                y: 10,
                z: 10,
            },
        )
    }

    #[test]
    fn segment_hit_enters_front_face() {
        let hit = unit_box()
            .segment_hit(IVec3 { x: -5, y: 5, z: 5 }, IVec3 { x: 20, y: 0, z: 0 })
            .expect("hit");
        assert_eq!(frac_cmp(hit.0, hit.1, 0, 1), core::cmp::Ordering::Greater);
        assert_eq!(frac_cmp(hit.0, hit.1, 1, 1), core::cmp::Ordering::Less);
    }

    #[test]
    fn segment_hit_closed_face_counts() {
        assert!(
            unit_box()
                .segment_hit(IVec3 { x: 10, y: 5, z: 5 }, IVec3 { x: 0, y: 0, z: 0 })
                .is_some()
        );
        assert!(
            unit_box()
                .segment_hit(IVec3 { x: -10, y: 5, z: 5 }, IVec3 { x: 10, y: 0, z: 0 })
                .is_some()
        );
    }

    #[test]
    fn segment_misses_off_axis_and_behind() {
        assert!(
            unit_box()
                .segment_hit(IVec3 { x: -5, y: 50, z: 5 }, IVec3 { x: 20, y: 0, z: 0 })
                .is_none()
        );
        assert!(
            unit_box()
                .segment_hit(IVec3 { x: 5, y: 5, z: -5 }, IVec3 { x: 0, y: 0, z: -10 })
                .is_none()
        );
        assert!(
            unit_box()
                .segment_hit(IVec3 { x: -20, y: 5, z: 5 }, IVec3 { x: 5, y: 0, z: 0 })
                .is_none()
        );
    }

    #[test]
    fn segment_origin_inside_is_t_zero() {
        let hit = unit_box()
            .segment_hit(IVec3 { x: 5, y: 5, z: 5 }, IVec3 { x: 10, y: 0, z: 0 })
            .expect("inside");
        assert_eq!(frac_cmp(hit.0, hit.1, 0, 1), core::cmp::Ordering::Equal);
    }
}
