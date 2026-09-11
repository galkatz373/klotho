//! K20 committed number types: millimetres, 16.16 velocity, millidegree yaw.

use core::fmt;
use core::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// Position along one axis, millimetres. Authoring metres convert at cook.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct Mm(pub i32);

impl Mm {
    /// Zero millimetres.
    pub const ZERO: Self = Self(0);

    /// Wrapping add. Overflow is a content bug; wrapping keeps hashes defined.
    #[must_use]
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self(self.0.wrapping_add(rhs.0))
    }

    /// Wrapping sub.
    #[must_use]
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        Self(self.0.wrapping_sub(rhs.0))
    }

    /// Apply a 16.16 velocity for one tick, truncating toward −∞ (arithmetic shift).
    #[must_use]
    pub const fn displace(self, vel: VelFx) -> Self {
        self.wrapping_add(vel.to_mm_trunc())
    }
}

impl Add for Mm {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        self.wrapping_add(rhs)
    }
}

impl AddAssign for Mm {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.wrapping_add(rhs);
    }
}

impl Sub for Mm {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.wrapping_sub(rhs)
    }
}

impl SubAssign for Mm {
    fn sub_assign(&mut self, rhs: Self) {
        *self = self.wrapping_sub(rhs);
    }
}

impl Neg for Mm {
    type Output = Self;
    fn neg(self) -> Self {
        Self(self.0.wrapping_neg())
    }
}

impl fmt::Display for Mm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}mm", self.0)
    }
}

/// Velocity in 16.16 fixed-point millimetres per tick (K20).
///
/// `ONE` is 1 mm / tick. Fractional millimetres live in the low 16 bits so
/// integration is deterministic across OS/arch without floats.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct VelFx(pub i32);

impl VelFx {
    /// Zero velocity.
    pub const ZERO: Self = Self(0);
    /// One millimetre per tick.
    pub const ONE: Self = Self(1 << Self::FRAC_BITS);
    /// Fractional bits in the 16.16 encoding.
    pub const FRAC_BITS: u32 = 16;
    /// `1 << FRAC_BITS`.
    pub const SCALE: i32 = 1 << Self::FRAC_BITS;

    /// Encode a whole millimetres-per-tick value.
    #[must_use]
    pub const fn from_mm_per_tick(mm: i32) -> Self {
        Self(mm.wrapping_mul(Self::SCALE))
    }

    /// Truncate toward −∞ to whole millimetres. Used when integrating pose.
    #[must_use]
    pub const fn to_mm_trunc(self) -> Mm {
        Mm(self.0 >> Self::FRAC_BITS)
    }

    /// Wrapping add of two velocities.
    #[must_use]
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self(self.0.wrapping_add(rhs.0))
    }

    /// Wrapping sub of two velocities.
    #[must_use]
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        Self(self.0.wrapping_sub(rhs.0))
    }

    /// Scale by a signed integer (e.g. tick count), wrapping.
    #[must_use]
    pub const fn wrapping_mul_i32(self, rhs: i32) -> Self {
        Self(self.0.wrapping_mul(rhs))
    }

    /// 16.16 × 16.16 → 16.16, arithmetic shift, wrapping into `i32`.
    #[must_use]
    pub const fn wrapping_mul(self, rhs: Self) -> Self {
        let prod = (self.0 as i64).wrapping_mul(rhs.0 as i64);
        Self((prod >> Self::FRAC_BITS) as i32)
    }
}

impl Add for VelFx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        self.wrapping_add(rhs)
    }
}

impl AddAssign for VelFx {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.wrapping_add(rhs);
    }
}

impl Sub for VelFx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.wrapping_sub(rhs)
    }
}

impl SubAssign for VelFx {
    fn sub_assign(&mut self, rhs: Self) {
        *self = self.wrapping_sub(rhs);
    }
}

impl Mul for VelFx {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        self.wrapping_mul(rhs)
    }
}

impl Mul<i32> for VelFx {
    type Output = Self;
    fn mul(self, rhs: i32) -> Self {
        self.wrapping_mul_i32(rhs)
    }
}

impl Neg for VelFx {
    type Output = Self;
    fn neg(self) -> Self {
        Self(self.0.wrapping_neg())
    }
}

impl fmt::Display for VelFx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mm = self.0 >> Self::FRAC_BITS;
        let frac = (self.0 as u32) & (Self::SCALE as u32 - 1);
        write!(f, "{mm}.{frac:04x} mm/t")
    }
}

/// Yaw in millidegrees. Canonical range is `[0, 360_000)`.
///
/// 1° = 1_000 millidegrees. Presenters convert to radians; the kernel does not.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct YawMd(pub i32);

impl YawMd {
    /// Zero yaw (facing +Z in Hearth's convention; cook documents the basis).
    pub const ZERO: Self = Self(0);
    /// One full turn, millidegrees.
    pub const FULL_TURN: i32 = 360_000;
    /// 90°, millidegrees.
    pub const QUARTER_TURN: i32 = 90_000;
    /// 180°, millidegrees.
    pub const HALF_TURN: i32 = 180_000;

    /// Wrap into `[0, FULL_TURN)`.
    #[must_use]
    pub const fn normalize(self) -> Self {
        let n = Self::FULL_TURN;
        let mut v = self.0 % n;
        if v < 0 {
            v += n;
        }
        Self(v)
    }

    /// Wrapping add, then normalize.
    #[must_use]
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self(self.0.wrapping_add(rhs.0)).normalize()
    }

    /// Wrapping sub, then normalize.
    #[must_use]
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        Self(self.0.wrapping_sub(rhs.0)).normalize()
    }

    /// Signed shortest delta in `(-180_000, 180_000]`.
    #[must_use]
    pub const fn delta(self, to: Self) -> i32 {
        let mut d = to.normalize().0.wrapping_sub(self.normalize().0);
        let half = Self::HALF_TURN;
        let full = Self::FULL_TURN;
        if d > half {
            d -= full;
        } else if d <= -half {
            d += full;
        }
        d
    }
}

impl Add for YawMd {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        self.wrapping_add(rhs)
    }
}

impl AddAssign for YawMd {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.wrapping_add(rhs);
    }
}

impl Sub for YawMd {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.wrapping_sub(rhs)
    }
}

impl SubAssign for YawMd {
    fn sub_assign(&mut self, rhs: Self) {
        *self = self.wrapping_sub(rhs);
    }
}

impl Neg for YawMd {
    type Output = Self;
    fn neg(self) -> Self {
        Self::ZERO.wrapping_sub(self)
    }
}

impl fmt::Display for YawMd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.normalize();
        write!(f, "{}.{:03}°", n.0 / 1000, n.0.abs() % 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vel_one_is_one_mm_per_tick() {
        assert_eq!(VelFx::from_mm_per_tick(1), VelFx::ONE);
        assert_eq!(VelFx::ONE.to_mm_trunc(), Mm(1));
        assert_eq!(VelFx::from_mm_per_tick(-3).to_mm_trunc(), Mm(-3));
    }

    #[test]
    fn vel_fractional_truncates_toward_neg_inf() {
        // 0.5 mm/tick → 0 mm this tick.
        let half = VelFx(VelFx::SCALE / 2);
        assert_eq!(half.to_mm_trunc(), Mm(0));
        // -0.5 mm/tick → -1 mm (arithmetic shift).
        let neg_half = VelFx(-VelFx::SCALE / 2);
        assert_eq!(neg_half.to_mm_trunc(), Mm(-1));
    }

    #[test]
    fn vel_mul_is_16_16() {
        let a = VelFx::from_mm_per_tick(2);
        let b = VelFx::from_mm_per_tick(3);
        assert_eq!(a * b, VelFx::from_mm_per_tick(6));
        let half = VelFx(VelFx::SCALE / 2);
        assert_eq!(
            VelFx::from_mm_per_tick(4) * half,
            VelFx::from_mm_per_tick(2)
        );
    }

    #[test]
    fn pose_integrates_with_trunc() {
        let p = Mm(1000);
        assert_eq!(p.displace(VelFx::from_mm_per_tick(7)), Mm(1007));
    }

    #[test]
    fn yaw_wraps_full_turn() {
        let a = YawMd(359_000);
        let b = YawMd(2_000);
        assert_eq!(a + b, YawMd(1_000));
        assert_eq!(YawMd(0) - YawMd(1_000), YawMd(359_000));
        assert_eq!(YawMd(-1).normalize(), YawMd(359_999));
        assert_eq!(YawMd(360_000).normalize(), YawMd(0));
    }

    #[test]
    fn yaw_shortest_delta() {
        assert_eq!(YawMd(0).delta(YawMd(10_000)), 10_000);
        assert_eq!(YawMd(0).delta(YawMd(350_000)), -10_000);
        assert_eq!(YawMd(10_000).delta(YawMd(0)), -10_000);
        assert_eq!(YawMd(0).delta(YawMd(180_000)), 180_000);
    }

    #[test]
    fn mm_wrapping_is_defined() {
        assert_eq!(Mm(i32::MAX) + Mm(1), Mm(i32::MIN));
    }
}
