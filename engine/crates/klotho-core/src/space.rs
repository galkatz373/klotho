//! Integer space types the kernel can check without linking `klotho-space`.

use serde::{Deserialize, Serialize};

use crate::{BlobId, Epoch, Mm, Sigil, VelFx, YawMd};

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

    /// Component-wise wrapping sub.
    #[must_use]
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        Self {
            x: self.x.wrapping_sub(rhs.x),
            y: self.y.wrapping_sub(rhs.y),
            z: self.z.wrapping_sub(rhs.z),
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

/// Cooked collision primitive. Identifiers live here (K61); queries live in
/// `klotho-geom`. Terrain kinds are static occupancy only.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum ShapeKind {
    /// Local AABB transformed by the locus pose.
    #[default]
    OrientedBox = 0,
    /// Sphere about the local AABB centre.
    Sphere = 1,
    /// Y-axis capsule transformed by the locus pose.
    Capsule = 2,
    /// Convex hull. PHYS-A04.
    Convex = 3,
    /// Compound of the first five dynamic kinds. PHYS-A04.
    Compound = 4,
    /// Static triangle mesh occupancy. PHYS-A05.
    TriangleMesh = 5,
    /// Static heightfield occupancy. PHYS-A05.
    Heightfield = 6,
}

/// Canonical rigid-body mode.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum BodyMode {
    /// Integrated and contact-resolved by Phys.
    #[default]
    Dynamic = 0,
    /// Canon trajectory drives pose; contacts see infinite mass.
    Kinematic = 1,
    /// Immutable occupancy; never a dynamic-island member.
    Static = 2,
}

/// Bounded Canon locomotion trajectory and capsule traversal policy (K63).
/// Root samples are local millimetres at authoritative tick boundaries.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct CharacterPhysics {
    /// Looping semantic root samples; unused entries must be zero.
    pub roots: [IVec3; 8],
    /// Number of live samples, 1..=8.
    pub root_count: u8,
    /// Maximum traversable riser in millimetres.
    pub step_mm: i32,
    /// Minimum upward contact normal, signed unit scaled by 32767.
    pub slope_min_y: i16,
}

impl Default for CharacterPhysics {
    fn default() -> Self {
        let mut roots = [IVec3::ZERO; 8];
        roots[0] = IVec3 { x: 0, y: 0, z: 20 };
        Self {
            roots,
            root_count: 1,
            step_mm: 250,
            slope_min_y: 23170,
        }
    }
}

impl CharacterPhysics {
    /// Bounded policy accepted by the pure character query.
    #[must_use]
    pub fn is_valid(self) -> bool {
        (1..=8).contains(&self.root_count)
            && (0..=500).contains(&self.step_mm)
            && self.slope_min_y > 0
            && self
                .roots
                .iter()
                .take(usize::from(self.root_count))
                .all(|r| {
                    i128::from(r.x) * i128::from(r.x) + i128::from(r.z) * i128::from(r.z)
                        <= 1_000_000
                        && r.x.unsigned_abs() <= 1000
                        && r.y.unsigned_abs() <= 1000
                        && r.z.unsigned_abs() <= 1000
                })
            && self
                .roots
                .iter()
                .skip(usize::from(self.root_count))
                .all(|r| *r == IVec3::ZERO)
    }

    /// Sample a Canon trajectory; no presenter clip or mutable time state.
    #[must_use]
    pub fn sample(self, tick: crate::Tick, moving: bool, yaw: YawMd) -> IVec3 {
        if !moving || !self.is_valid() {
            return IVec3::ZERO;
        }
        crate::rotate_xz(
            self.roots[(tick.0 % u64::from(self.root_count)) as usize],
            yaw,
        )
    }
}

/// Pure desired displacement, consumed inside the character island solve.
/// This is an input description, never an independently admitted proposal.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct CharacterDrive {
    /// Character identity.
    pub actor: Sigil,
    /// Desired world-space root displacement in millimetres.
    pub root: IVec3,
}

/// Bounded Canon four-wheel ray-cast rig (PHYS-A09). Wheels are configuration
/// on the chassis, not a component hierarchy or independent spatial owners.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct VehiclePhysics {
    /// Local hub positions in millimetres. Unused entries must be zero.
    pub wheels: [IVec3; 4],
    /// Number of live wheels, 2..=4.
    pub wheel_count: u8,
    /// Unloaded ray length to the contact plane, millimetres.
    pub rest_mm: i32,
    /// Spring stiffness, permille of the mass-derived rest load.
    pub stiffness_permille: u16,
    /// Damper coefficient, permille of critical damping.
    pub damper_permille: u16,
    /// Wheel radius, millimetres. Presentation spin derives from this.
    pub radius_mm: i32,
    /// Maximum front-wheel steer, millidegrees.
    pub steer_md: i32,
    /// Throttle magnitude at full `PhysRequest.lin.z`, millimetres per tick.
    pub drive_mm: i32,
    /// Brake magnitude at full `PhysRequest.lin.y`, millimetres per tick.
    pub brake_mm: i32,
    /// Longitudinal tire friction scale, permille.
    pub long_friction_permille: u16,
    /// Lateral tire friction scale, permille.
    pub lat_friction_permille: u16,
}

impl Default for VehiclePhysics {
    fn default() -> Self {
        Self {
            wheels: [
                IVec3 {
                    x: -300,
                    y: 0,
                    z: 500,
                },
                IVec3 {
                    x: 300,
                    y: 0,
                    z: 500,
                },
                IVec3 {
                    x: -300,
                    y: 0,
                    z: -500,
                },
                IVec3 {
                    x: 300,
                    y: 0,
                    z: -500,
                },
            ],
            wheel_count: 4,
            rest_mm: 250,
            stiffness_permille: 1_000,
            damper_permille: 800,
            radius_mm: 200,
            steer_md: 25_000,
            drive_mm: 40,
            brake_mm: 40,
            long_friction_permille: 1_000,
            lat_friction_permille: 1_000,
        }
    }
}

impl VehiclePhysics {
    /// Bounded rig accepted by the scalar vehicle solve.
    #[must_use]
    pub fn is_valid(self) -> bool {
        (2..=4).contains(&self.wheel_count)
            && (50..=2_000).contains(&self.rest_mm)
            && (20..=1_000).contains(&self.radius_mm)
            && (1..=2_000).contains(&self.stiffness_permille)
            && self.damper_permille <= 2_000
            && (0..=45_000).contains(&self.steer_md)
            && (1..=500).contains(&self.drive_mm)
            && (0..=500).contains(&self.brake_mm)
            && self.long_friction_permille <= 2_000
            && self.lat_friction_permille <= 2_000
            && self
                .wheels
                .iter()
                .take(usize::from(self.wheel_count))
                .all(|w| {
                    w.x.unsigned_abs() <= 5_000
                        && w.y.unsigned_abs() <= 2_000
                        && w.z.unsigned_abs() <= 5_000
                })
            && self
                .wheels
                .iter()
                .skip(usize::from(self.wheel_count))
                .all(|w| *w == IVec3::ZERO)
    }

    /// Instantaneous wheel spin rate from admitted chassis velocity, millidegrees per tick.
    /// Presentation integrates this; it is not Projection.
    #[must_use]
    pub fn presented_wheel_rate_md(self, vel: Vel3, yaw: YawMd) -> i32 {
        if !self.is_valid() {
            return 0;
        }
        let forward = crate::rotate_xz(
            IVec3 {
                x: vel.x.to_mm_trunc().0,
                y: 0,
                z: vel.z.to_mm_trunc().0,
            },
            YawMd(yaw.0.wrapping_neg()),
        );
        let radius = i64::from(self.radius_mm.max(1));
        ((i64::from(forward.z) * 57_296) / radius).clamp(-180_000, 180_000) as i32
    }
}

/// Pure chassis controls reconstructed from Canon, Projection, and `PhysRequest`.
/// Never an independently admitted proposal.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct VehicleDrive {
    /// Chassis identity.
    pub chassis: Sigil,
    /// Forward command, millimetres per tick, signed.
    pub throttle: i32,
    /// Brake command, millimetres per tick, not negative.
    pub brake: i32,
    /// Front-wheel steer, millidegrees, signed.
    pub steer_md: i32,
}

impl Default for VehicleDrive {
    fn default() -> Self {
        Self {
            chassis: Sigil::from_raw(0),
            throttle: 0,
            brake: 0,
            steer_md: 0,
        }
    }
}

/// Canon-bound material and mass properties. Integers keep this configuration
/// portable; `klotho-phys` alone converts them to its pinned scalar lane.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct BodyPhysics {
    /// Body mode.
    pub mode: BodyMode,
    /// Canonical cooked collision kind for the bound hull blob.
    pub shape: ShapeKind,
    /// Explicit driven-character binding. Legacy actors remain Motion-owned.
    #[serde(default)]
    pub character: Option<CharacterPhysics>,
    /// Explicit four-wheel rig. Unbound Driveable relics keep PHYS_REQ Δv.
    #[serde(default)]
    pub vehicle: Option<VehiclePhysics>,
    /// Mass in grams. Zero requests deterministic volume-derived mass.
    pub mass_grams: u32,
    /// Local centre of mass, millimetres.
    pub center_of_mass: IVec3,
    /// Diagonal inertia in gram-square-millimetres. Zero entries are derived.
    pub inertia_diag: [u64; 3],
    /// Coulomb coefficient, permille.
    pub friction_permille: u16,
    /// Normal restitution, permille.
    pub restitution_permille: u16,
}

impl Default for BodyPhysics {
    fn default() -> Self {
        Self {
            mode: BodyMode::Dynamic,
            shape: ShapeKind::OrientedBox,
            character: None,
            vehicle: None,
            mass_grams: 0,
            center_of_mass: IVec3::ZERO,
            inertia_diag: [0; 3],
            friction_permille: 900,
            restitution_permille: 0,
        }
    }
}

impl BodyPhysics {
    /// Values accepted by the scalar solver. Invalid Canon fails closed.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.friction_permille <= 2_000
            && self.restitution_permille <= 1_000
            && !(self.character.is_some() && self.vehicle.is_some())
            && self.character.is_none_or(|c| {
                c.is_valid() && self.shape == ShapeKind::Capsule && self.mode == BodyMode::Dynamic
            })
            && self.vehicle.is_none_or(|v| {
                v.is_valid() && self.shape.is_dynamic() && self.mode == BodyMode::Dynamic
            })
    }
}

impl ShapeKind {
    /// True for the PHYS-A03 dynamic primitives the kernel can reproduce.
    #[must_use]
    pub const fn is_oriented_primitive(self) -> bool {
        matches!(self, Self::OrientedBox | Self::Sphere | Self::Capsule)
    }

    /// True for a shape kind legal on an authoritative dynamic body.
    #[must_use]
    pub const fn is_dynamic(self) -> bool {
        matches!(
            self,
            Self::OrientedBox | Self::Sphere | Self::Capsule | Self::Convex | Self::Compound
        )
    }

    /// True for static occupancy that must never join a dynamic island.
    #[must_use]
    pub const fn is_static_occupancy(self) -> bool {
        matches!(self, Self::TriangleMesh | Self::Heightfield)
    }
}

/// Canonical joint kind. Endpoints, rest, and break threshold are Canon.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum ConstraintKind {
    /// Coincident anchors. No relative translation or rotation.
    #[default]
    Fixed = 0,
    /// Coincident anchors; rotation only about `axis`.
    Hinge = 1,
    /// Translation only along `axis`.
    Slider = 2,
    /// Hooke spring along the separation of the anchors.
    Spring = 3,
}

/// Canon-bound physical constraint. Identity is a [`Sigil`] that need not be a locus.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct ConstraintPhysics {
    /// Joint kind.
    pub kind: ConstraintKind,
    /// First body.
    pub a: Sigil,
    /// Second body, or a static occupancy locus.
    pub b: Sigil,
    /// Local anchor on `a`, millimetres.
    pub anchor_a: IVec3,
    /// Local anchor on `b`, millimetres.
    pub anchor_b: IVec3,
    /// Hinge/slider axis in `a`'s local frame. Ignored by fixed/spring.
    pub axis: IVec3,
    /// Spring rest length, millimetres. Hinge angular rest is unused.
    pub rest_mm: i32,
    /// Spring stiffness, permille of a unit XPBD compliance.
    pub stiffness_permille: u16,
    /// Hinge angular envelope, millidegrees. Zero is unlimited.
    pub limit_md: i32,
    /// Impulse that produces a break. Zero is unbreakable.
    pub break_impulse: i32,
    /// Authoritative physical fragments to spawn on an admitted break, 0..=64.
    pub fragments: u8,
    /// Canonical binding observed by the proposer and kernel.
    pub binding: BlobId,
}

impl Default for ConstraintPhysics {
    fn default() -> Self {
        Self {
            kind: ConstraintKind::Fixed,
            a: Sigil::from_raw(0),
            b: Sigil::from_raw(0),
            anchor_a: IVec3::ZERO,
            anchor_b: IVec3::ZERO,
            axis: IVec3 { x: 0, y: 1, z: 0 },
            rest_mm: 0,
            stiffness_permille: 1_000,
            limit_md: 0,
            break_impulse: 0,
            fragments: 0,
            binding: BlobId::ZERO,
        }
    }
}

impl ConstraintPhysics {
    /// Values the kernel will admit. Invalid Canon fails closed.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.a.raw() != 0
            && self.b.raw() != 0
            && self.a.raw() != self.b.raw()
            && self.stiffness_permille <= 2_000
            && self.limit_md >= 0
            && self.break_impulse >= 0
            && self.fragments <= MAX_COLLAPSE_FRAGMENTS
    }
}

/// Per-collapse cap on authoritative physical fragments (K41 / Ember).
pub const MAX_COLLAPSE_FRAGMENTS: u8 = 64;
/// Global cap on live Fragment-marked loci. Extra chips are Manifest-only.
pub const MAX_FRAGMENTS_GLOBAL: u16 = 128;

/// Admitted constraint row. Solver caches are illegal; this is Projection.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct ConstraintState {
    /// Last admitted impulse magnitude, millimetre-weighted.
    pub impulse: i32,
    /// True after an admitted break. Intact constraints keep this false.
    pub broken: bool,
}

/// Bounded quantized contact evidence. Not a solver manifold.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct QuantizedContact {
    /// Contact point, millimetres.
    pub point: IVec3,
    /// Unit-ish normal, 32767 scale, pointing from `b` toward `a`.
    pub normal: (i16, i16, i16),
    /// Penetration along `normal`, millimetres. Zero is touching.
    pub depth_mm: i32,
    /// Deterministic feature id (SAT axis or closest-feature index).
    pub feature: u16,
}

/// Collision witness a proposer attaches to a `SpaceDelta` / `MotionDelta`.
///
/// The kernel **ignores any proposer-supplied swept volume** (K24). It derives
/// `swept = conservative_aabb(prev_pose, proposed, hull(mover, epoch))` and
/// rechecks `OpaqueClosed`. `overlaps_closed_opaque` is a hint: if it says no
/// overlap and the kernel finds one, the reject is
/// [`crate::RejectReason::WitnessMismatch`].
///
/// `evidence` is required for gameplay-visible [`QuantizedContact`] claims;
/// the kernel reproduces it and never trusts an opaque manifold.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct HullWitness {
    /// Locus whose hull is being moved.
    pub mover: Sigil,
    /// Where the proposer wants to go. Kernel derives swept from this.
    pub proposed: PoseMm,
    /// Proposer hint only. Kernel recomputes overlap against `OpaqueClosed`.
    pub overlaps_closed_opaque: bool,
    /// Canon epoch observed with this witness. Mismatch → `StaleEpoch`.
    #[serde(default)]
    pub epoch: Epoch,
    /// Claimed cooked primitive. Kernel reproduces from the canonical hull.
    #[serde(default)]
    pub shape: ShapeKind,
    /// Optional quantized contact. Required for gameplay contact claims.
    #[serde(default)]
    pub evidence: Option<QuantizedContact>,
}

impl HullWitness {
    /// Construct a witness. `overlaps_closed_opaque` is the K21 hint.
    /// Epoch is zero, shape is an oriented box, and evidence is absent.
    #[must_use]
    pub const fn new(mover: Sigil, proposed: PoseMm, overlaps_closed_opaque: bool) -> Self {
        Self {
            mover,
            proposed,
            overlaps_closed_opaque,
            epoch: Epoch::ZERO,
            shape: ShapeKind::OrientedBox,
            evidence: None,
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
        assert_eq!(w.epoch, Epoch::ZERO);
        assert_eq!(w.shape, ShapeKind::OrientedBox);
        assert!(w.evidence.is_none());
    }

    #[test]
    fn body_physics_material_bounds_fail_closed() {
        assert!(BodyPhysics::default().is_valid());
        assert!(
            !BodyPhysics {
                friction_permille: 2_001,
                ..BodyPhysics::default()
            }
            .is_valid()
        );
        assert!(
            !BodyPhysics {
                restitution_permille: 1_001,
                ..BodyPhysics::default()
            }
            .is_valid()
        );
    }

    #[test]
    fn vehicle_physics_bounds_fail_closed() {
        assert!(VehiclePhysics::default().is_valid());
        assert!(
            BodyPhysics {
                vehicle: Some(VehiclePhysics::default()),
                ..BodyPhysics::default()
            }
            .is_valid()
        );
        assert!(
            !VehiclePhysics {
                wheel_count: 1,
                ..VehiclePhysics::default()
            }
            .is_valid()
        );
        assert!(
            !BodyPhysics {
                character: Some(CharacterPhysics::default()),
                vehicle: Some(VehiclePhysics::default()),
                shape: ShapeKind::Capsule,
                ..BodyPhysics::default()
            }
            .is_valid()
        );
    }

    #[test]
    fn constraint_physics_rejects_degenerate_endpoints() {
        let a = Sigil::pack(LocusKind::Relic, 0, 1).unwrap();
        let b = Sigil::pack(LocusKind::Relic, 0, 2).unwrap();
        let mut ok = ConstraintPhysics {
            a,
            b,
            binding: BlobId::from_bytes([1; 32]),
            ..ConstraintPhysics::default()
        };
        assert!(ok.is_valid());
        ok.a = b;
        assert!(!ok.is_valid());
        ok.a = a;
        ok.stiffness_permille = 2_001;
        assert!(!ok.is_valid());
        ok.stiffness_permille = 1_000;
        ok.fragments = MAX_COLLAPSE_FRAGMENTS;
        assert!(ok.is_valid());
        ok.fragments = MAX_COLLAPSE_FRAGMENTS + 1;
        assert!(!ok.is_valid());
        assert!(ShapeKind::Heightfield.is_static_occupancy());
        assert!(!ShapeKind::Convex.is_static_occupancy());
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
