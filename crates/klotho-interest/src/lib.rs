//! Pure `F(view)` interest → [`SimLod`]. Depends on world + core only (K49).
//!
//! Sleepers and Dormant loci stay in `space_ix`. This crate never writes.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;

use klotho_core::{LOD_PERIOD, LocusKind, NO_ISLAND, PoseMm, Sigil, SimLod};
use klotho_world::WorldView;

/// Radii for Full / Far rings, millimetres.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct InterestConfig {
    /// Inside this Chebyshev XZ range of an observer → [`SimLod::Full`].
    pub full_mm: i32,
    /// Inside this range (and outside `full_mm`) → [`SimLod::Far`].
    pub far_mm: i32,
}

impl Default for InterestConfig {
    fn default() -> Self {
        Self {
            full_mm: 20_000,
            far_mm: 80_000,
        }
    }
}

/// Place residency hint. Runtime wraps as a proposal later.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum ResidencyCommand {
    /// Observer needs this Place.
    Load(Sigil),
    /// No observer is within the Far ring of this Place.
    Evict(Sigil),
}

/// Result of one interest pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Interest {
    /// Lod per locus that has a pose.
    pub lod: BTreeMap<Sigil, SimLod>,
    /// Place load/evict commands (deterministic Place-sigil order).
    pub residency: Vec<ResidencyCommand>,
}

/// Classify every posed locus. Island-mates of a Full body are Full.
#[must_use]
pub fn classify(view: &WorldView<'_>, cfg: &InterestConfig) -> Interest {
    let observers: Vec<PoseMm> = view
        .loci()
        .filter(|&s| {
            matches!(s.kind(), Some(LocusKind::Observer | LocusKind::Actor))
                && view.pose(s).is_some()
        })
        .filter_map(|s| view.pose(s))
        .collect();

    let mut lod: BTreeMap<Sigil, SimLod> = BTreeMap::new();
    for s in view.loci() {
        let Some(pose) = view.pose(s) else {
            continue;
        };
        lod.insert(s, ring(&observers, pose, cfg));
    }

    let mut island_full: BTreeMap<u16, bool> = BTreeMap::new();
    for s in view.loci() {
        if lod.get(&s) == Some(&SimLod::Full) {
            if let Some((id, _)) = view.island(s) {
                if id != NO_ISLAND {
                    island_full.insert(id, true);
                }
            }
        }
    }
    for s in view.loci() {
        let Some((id, _)) = view.island(s) else {
            continue;
        };
        if id == NO_ISLAND {
            continue;
        }
        if island_full.get(&id) == Some(&true) {
            if let Some(slot) = lod.get_mut(&s) {
                *slot = SimLod::Full;
            }
        }
    }

    let mut residency = Vec::new();
    for s in view.loci() {
        if s.kind() != Some(LocusKind::Place) {
            continue;
        }
        let Some(pose) = view.pose(s) else {
            continue;
        };
        match ring(&observers, pose, cfg) {
            SimLod::Dormant => residency.push(ResidencyCommand::Evict(s)),
            SimLod::Full | SimLod::Far => residency.push(ResidencyCommand::Load(s)),
        }
    }

    Interest { lod, residency }
}

/// True when Space/Mind should skip this locus this tick.
#[must_use]
pub fn skip_propose(view: &WorldView<'_>, s: Sigil) -> bool {
    match view.sim_lod(s) {
        SimLod::Dormant => true,
        SimLod::Far => view.tick().0 % u64::from(LOD_PERIOD) != 0,
        SimLod::Full => false,
    }
}

fn ring(observers: &[PoseMm], pose: PoseMm, cfg: &InterestConfig) -> SimLod {
    if observers.is_empty() {
        return SimLod::Full;
    }
    let mut best = i32::MAX;
    for o in observers {
        best = best.min(chebyshev_xz(*o, pose));
    }
    if best <= cfg.full_mm {
        SimLod::Full
    } else if best <= cfg.far_mm {
        SimLod::Far
    } else {
        SimLod::Dormant
    }
}

fn chebyshev_xz(a: PoseMm, b: PoseMm) -> i32 {
    (a.x.0.abs_diff(b.x.0) as i32).max(a.z.0.abs_diff(b.z.0) as i32)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_core::{
        AabbMm, BlobId, Hash, IVec3, LOD_PERIOD, LocusKind, Mm, PoseMm, Sigil, SimLod, YawMd,
    };
    use klotho_world::World;

    use super::*;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    fn hull() -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -100,
                y: 0,
                z: -100,
            },
            IVec3 {
                x: 100,
                y: 500,
                z: 100,
            },
        )
    }

    fn world() -> World {
        World::new(Arc::new(klotho_canon::cook_diffs(&[]).unwrap()), Hash::ZERO)
    }

    fn plant(w: &mut World, s: Sigil, x: i32) {
        let mut m = w.mutate();
        m.insert_locus(s, s.kind().unwrap()).unwrap();
        m.set_hull(s, hull(), BlobId::ZERO).unwrap();
        m.set_pose(s, PoseMm::new(Mm(x), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
    }

    #[test]
    fn far_and_dormant_rings() {
        let mut w = world();
        let player = actor(1);
        let near = relic(2);
        let mid = relic(3);
        let far = relic(4);
        plant(&mut w, player, 0);
        plant(&mut w, near, 1_000);
        plant(&mut w, mid, 40_000);
        plant(&mut w, far, 200_000);
        {
            let mut m = w.mutate();
            m.set_island(player, 0, 0).unwrap();
            m.set_island(near, 0, 0).unwrap();
            m.set_island(mid, 1, 0).unwrap();
            m.set_island(far, 2, 0).unwrap();
        }
        let out = classify(&w.view(), &InterestConfig::default());
        assert_eq!(out.lod.get(&player), Some(&SimLod::Full));
        assert_eq!(out.lod.get(&near), Some(&SimLod::Full));
        assert_eq!(out.lod.get(&mid), Some(&SimLod::Far));
        assert_eq!(out.lod.get(&far), Some(&SimLod::Dormant));
    }

    #[test]
    fn island_wake_promotes_mates() {
        let mut w = world();
        let player = actor(1);
        let crate_ = relic(2);
        plant(&mut w, player, 0);
        plant(&mut w, crate_, 40_000);
        {
            let mut m = w.mutate();
            m.set_island(player, 3, 0).unwrap();
            m.set_island(crate_, 3, 12).unwrap();
        }
        let out = classify(&w.view(), &InterestConfig::default());
        assert_eq!(out.lod.get(&crate_), Some(&SimLod::Full));
    }

    #[test]
    fn place_residency_load_and_evict() {
        let mut w = world();
        let player = actor(1);
        let here = place(2);
        let away = place(3);
        plant(&mut w, player, 0);
        plant(&mut w, here, 0);
        plant(&mut w, away, 200_000);
        let out = classify(&w.view(), &InterestConfig::default());
        assert!(out.residency.contains(&ResidencyCommand::Load(here)));
        assert!(out.residency.contains(&ResidencyCommand::Evict(away)));
    }

    #[test]
    fn skip_propose_dormant_and_far_period() {
        let mut w = world();
        let s = relic(1);
        plant(&mut w, s, 0);
        w.mutate().set_sim_lod(s, SimLod::Dormant).unwrap();
        assert!(skip_propose(&w.view(), s));
        w.mutate().set_sim_lod(s, SimLod::Far).unwrap();
        w.mutate().set_tick(klotho_core::Tick(1));
        assert_ne!(1 % u64::from(LOD_PERIOD), 0);
        assert!(skip_propose(&w.view(), s));
        w.mutate().set_tick(klotho_core::Tick(6));
        assert!(!skip_propose(&w.view(), s));
        w.mutate().set_sim_lod(s, SimLod::Full).unwrap();
        w.mutate().set_tick(klotho_core::Tick(1));
        assert!(!skip_propose(&w.view(), s));
    }

    #[test]
    fn lod_period_is_six() {
        assert_eq!(LOD_PERIOD, 6);
    }

    #[test]
    fn no_island_does_not_wake_unrelated_scenery() {
        let mut w = world();
        let player = actor(1);
        let wall = relic(2);
        plant(&mut w, player, 0);
        plant(&mut w, wall, 40_000);
        {
            let mut m = w.mutate();
            m.set_island(player, NO_ISLAND, 0).unwrap();
            m.set_island(wall, NO_ISLAND, 0).unwrap();
        }
        let out = classify(&w.view(), &InterestConfig::default());
        assert_eq!(out.lod.get(&player), Some(&SimLod::Full));
        assert_eq!(out.lod.get(&wall), Some(&SimLod::Far));
    }
}
