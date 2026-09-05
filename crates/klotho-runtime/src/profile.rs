//! Runtime capacity and scheduling profiles.

use klotho_core::{Budget, MAX_LOCI_HEARTH, MAX_LOCI_PROCESS};

/// Coherent runtime composition selected at build or startup.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum RuntimeProfile {
    /// v1 Hearth/Ash path: synchronous proposers, 60 Hz, 4,096 loci.
    Hearth,
    /// 30 Hz authoritative adventure path with island jobs and physics.
    AaaAdventure,
    /// 60 Hz authoritative shooter path with island jobs and physics.
    AaaShooter,
}

impl RuntimeProfile {
    /// Profile selected by mutually exclusive Cargo features.
    #[must_use]
    pub const fn compiled() -> Self {
        #[cfg(feature = "aaa-shooter")]
        {
            Self::AaaShooter
        }
        #[cfg(all(not(feature = "aaa-shooter"), feature = "aaa-adventure"))]
        {
            Self::AaaAdventure
        }
        #[cfg(not(any(feature = "aaa-shooter", feature = "aaa-adventure")))]
        {
            Self::Hearth
        }
    }

    /// Deterministic work caps and telemetry target.
    #[must_use]
    pub const fn budget(self) -> Budget {
        match self {
            Self::Hearth => Budget::HEARTH,
            Self::AaaAdventure => Budget::AAA_ADVENTURE,
            Self::AaaShooter => Budget::AAA_SHOOTER,
        }
    }

    /// Packed-row capacity.
    #[must_use]
    pub const fn locus_cap(self) -> usize {
        match self {
            Self::Hearth => MAX_LOCI_HEARTH,
            Self::AaaAdventure | Self::AaaShooter => MAX_LOCI_PROCESS,
        }
    }

    /// Authoritative ticks per second.
    #[must_use]
    pub const fn auth_hz(self) -> u32 {
        match self {
            Self::AaaAdventure => 30,
            Self::Hearth | Self::AaaShooter => 60,
        }
    }

    /// Worker count used by the island proposer path.
    #[must_use]
    pub const fn workers(self) -> usize {
        match self {
            Self::Hearth => 1,
            Self::AaaAdventure | Self::AaaShooter => 8,
        }
    }

    /// Whether the runtime uses partitioned jobs and the Phys proposer.
    #[must_use]
    pub const fn uses_island_jobs(self) -> bool {
        !matches!(self, Self::Hearth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_caps_and_rates_are_coherent() {
        assert_eq!(RuntimeProfile::Hearth.locus_cap(), MAX_LOCI_HEARTH);
        assert_eq!(RuntimeProfile::AaaAdventure.locus_cap(), MAX_LOCI_PROCESS);
        assert_eq!(RuntimeProfile::AaaAdventure.auth_hz(), 30);
        assert_eq!(RuntimeProfile::AaaAdventure.budget(), Budget::AAA_ADVENTURE);
        assert_eq!(RuntimeProfile::AaaShooter.locus_cap(), MAX_LOCI_PROCESS);
        assert_eq!(RuntimeProfile::AaaShooter.auth_hz(), 60);
        assert_eq!(RuntimeProfile::AaaShooter.budget(), Budget::AAA_SHOOTER);
    }

    #[test]
    fn compiled_feature_selects_expected_profile() {
        #[cfg(feature = "aaa-shooter")]
        assert_eq!(RuntimeProfile::compiled(), RuntimeProfile::AaaShooter);
        #[cfg(all(not(feature = "aaa-shooter"), feature = "aaa-adventure"))]
        assert_eq!(RuntimeProfile::compiled(), RuntimeProfile::AaaAdventure);
        #[cfg(not(any(feature = "aaa-shooter", feature = "aaa-adventure")))]
        assert_eq!(RuntimeProfile::compiled(), RuntimeProfile::Hearth);
    }
}
