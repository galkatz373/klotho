//! Global sim clock (K17) and cook/hull epoch.

use core::fmt;
use core::ops::{Add, AddAssign, Sub};

/// One global tick. Pause stops `step`; it does not mint a Place.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct Tick(pub u64);

impl Tick {
    /// The first tick of a session.
    pub const ZERO: Self = Self(0);

    /// Saturating increment. Kernel time does not wrap in v1.
    #[must_use]
    pub const fn saturating_add(self, dt: u64) -> Self {
        Self(self.0.saturating_add(dt))
    }

    /// Next tick, saturating.
    #[must_use]
    pub const fn next(self) -> Self {
        self.saturating_add(1)
    }
}

impl Add<u64> for Tick {
    type Output = Self;
    fn add(self, rhs: u64) -> Self {
        self.saturating_add(rhs)
    }
}

impl AddAssign<u64> for Tick {
    fn add_assign(&mut self, rhs: u64) {
        *self = self.saturating_add(rhs);
    }
}

impl Sub for Tick {
    type Output = u64;
    fn sub(self, rhs: Self) -> u64 {
        self.0.saturating_sub(rhs.0)
    }
}

impl fmt::Display for Tick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t{}", self.0)
    }
}

/// Cook / hull binding epoch. Canonical hulls are keyed by `(Sigil, Epoch)`.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct Epoch(pub u64);

impl Epoch {
    /// Epoch zero: the cooked Canon that shipped in the `.warp`.
    pub const ZERO: Self = Self(0);
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "e{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_orders_and_saturates() {
        assert!(Tick(1) > Tick::ZERO);
        assert_eq!(Tick(u64::MAX).next(), Tick(u64::MAX));
        assert_eq!(Tick(10) - Tick(3), 7);
        assert_eq!(Tick(3) - Tick(10), 0);
    }
}
