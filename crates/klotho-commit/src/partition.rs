//! This-tick island partition (K58). Pure `F(view)`; not last-tick memory.

use std::collections::BTreeMap;

use klotho_core::{MAX_ISLANDS, Sigil, Vel3};
use klotho_ir::Rel;
use klotho_world::WorldView;

/// Union-find on posed hulls this tick.
///
/// Seed = Full-lod hulls (missing lod is Full) that are awake (`sleep_ticks == 0`)
/// **or** have non-zero vel **or** `phys_req` **or** are `PilotedBy`/`AttachedTo`
/// an awake locus. Flood-fill through overlapping hulls including sleepers.
/// Island id = dense rank of min-Sigil. Extra components past [`MAX_ISLANDS`]
/// are dropped (fail closed).
#[must_use]
pub fn partition_islands(view: &WorldView<'_>) -> Vec<(u16, Vec<Sigil>)> {
    let mut hulls: Vec<(Sigil, klotho_core::AabbMm)> = Vec::new();
    for s in view.loci() {
        if let Some(aabb) = view.posed_hull(s) {
            hulls.push((s, aabb));
        }
    }
    if hulls.is_empty() {
        return Vec::new();
    }

    let mut ix_of: BTreeMap<Sigil, u32> = BTreeMap::new();
    for (i, (s, _)) in hulls.iter().enumerate() {
        ix_of.insert(*s, i as u32);
    }

    let n = hulls.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let mut rank: Vec<u8> = vec![0; n];

    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            let p = parent[x as usize];
            parent[x as usize] = parent[p as usize];
            x = p;
        }
        x
    }
    fn union(parent: &mut [u32], rank: &mut [u8], a: u32, b: u32) {
        let mut ra = find(parent, a);
        let mut rb = find(parent, b);
        if ra == rb {
            return;
        }
        if rank[ra as usize] < rank[rb as usize] {
            core::mem::swap(&mut ra, &mut rb);
        }
        parent[rb as usize] = ra;
        if rank[ra as usize] == rank[rb as usize] {
            rank[ra as usize] = rank[ra as usize].saturating_add(1);
        }
    }

    let mut seed = vec![false; n];
    for (i, (s, _)) in hulls.iter().enumerate() {
        if is_seed(view, *s) {
            seed[i] = true;
        }
    }

    let mut reached = vec![false; n];
    let mut stack: Vec<u32> = Vec::new();
    for (i, is_seed) in seed.iter().enumerate() {
        if !is_seed {
            continue;
        }
        stack.push(i as u32);
        while let Some(u) = stack.pop() {
            let ui = u as usize;
            if reached[ui] {
                continue;
            }
            reached[ui] = true;
            let aabb = hulls[ui].1;
            for o in view.space_candidates(aabb, false) {
                let Some(&j) = ix_of.get(&o) else {
                    continue;
                };
                if j == u {
                    continue;
                }
                let Some(other) = view.posed_hull(o) else {
                    continue;
                };
                if !aabb.intersects(other) {
                    continue;
                }
                union(&mut parent, &mut rank, u, j);
                if !reached[j as usize] {
                    stack.push(j);
                }
            }
        }
    }

    let mut groups: BTreeMap<u32, Vec<Sigil>> = BTreeMap::new();
    for i in 0..n {
        if !reached[i] {
            continue;
        }
        let root = find(&mut parent, i as u32);
        groups.entry(root).or_default().push(hulls[i].0);
    }

    let mut ranked: Vec<(Sigil, Vec<Sigil>)> = groups
        .into_values()
        .map(|mut members| {
            members.sort_unstable();
            let min = members[0];
            (min, members)
        })
        .collect();
    ranked.sort_unstable_by_key(|(min, _)| *min);

    ranked
        .into_iter()
        .take(usize::from(MAX_ISLANDS).saturating_add(1))
        .enumerate()
        .map(|(i, (_, members))| (i as u16, members))
        .collect()
}

fn is_seed(view: &WorldView<'_>, s: Sigil) -> bool {
    // AAA-05 lod column is not here; missing lod is Full.
    let sleep = view.island(s).map(|(_, t)| t).unwrap_or(0);
    if sleep == 0 {
        return true;
    }
    if let Some((vel, yaw_rate)) = view.vel(s) {
        if vel != Vel3::ZERO || yaw_rate != 0 {
            return true;
        }
    }
    if view.phys_req(s).is_some() {
        return true;
    }
    attached_to_awake(view, s)
}

fn attached_to_awake(view: &WorldView<'_>, s: Sigil) -> bool {
    for rel in [Rel::PilotedBy, Rel::AttachedTo] {
        for other in view.related(s, rel) {
            let sleep = view.island(other).map(|(_, t)| t).unwrap_or(0);
            if sleep == 0 {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{
        AabbMm, BlobId, Hash, IVec3, LocusKind, Mm, PhysRequest, PoseMm, Sigil, Vel3, VelFx, YawMd,
    };
    use klotho_ir::{CanonDiff, Rel, from_ron};
    use klotho_world::World;

    use super::*;

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn box_mm(half: i32) -> AabbMm {
        AabbMm::new(
            IVec3 {
                x: -half,
                y: 0,
                z: -half,
            },
            IVec3 {
                x: half,
                y: 500,
                z: half,
            },
        )
    }

    fn world() -> World {
        let d: Vec<CanonDiff> = from_ron("[]").unwrap();
        World::new(Arc::new(cook_diffs(&d).unwrap()), Hash::ZERO)
    }

    fn plant(w: &mut World, s: Sigil, x: i32, sleep: u16) {
        let mut m = w.mutate();
        m.insert_locus(s, LocusKind::Relic).unwrap();
        m.set_hull(s, box_mm(100), BlobId::ZERO).unwrap();
        m.set_pose(s, PoseMm::new(Mm(x), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        m.set_island(s, 99, sleep).unwrap();
    }

    #[test]
    fn disjoint_awake_hulls_are_dense_min_sigil_rank() {
        let mut w = world();
        let a = relic(3);
        let b = relic(1);
        let c = relic(2);
        plant(&mut w, a, 10_000, 0);
        plant(&mut w, b, 0, 0);
        plant(&mut w, c, 20_000, 0);
        let islands = partition_islands(&w.view());
        assert_eq!(islands.len(), 3);
        assert_eq!(islands[0], (0, vec![b]));
        assert_eq!(islands[1], (1, vec![c]));
        assert_eq!(islands[2], (2, vec![a]));
    }

    #[test]
    fn sleeper_pile_joins_bumped_awake() {
        let mut w = world();
        let awake = relic(1);
        let sleep_near = relic(2);
        let sleep_far = relic(3);
        plant(&mut w, awake, 0, 0);
        plant(&mut w, sleep_near, 150, 12);
        plant(&mut w, sleep_far, 50_000, 12);
        let islands = partition_islands(&w.view());
        assert_eq!(islands.len(), 1, "{islands:?}");
        assert_eq!(islands[0].1, vec![awake, sleep_near]);
    }

    #[test]
    fn last_tick_island_id_is_ignored() {
        let mut w = world();
        let a = relic(1);
        let b = relic(2);
        plant(&mut w, a, 0, 0);
        plant(&mut w, b, 50_000, 0);
        {
            let mut m = w.mutate();
            m.set_island(a, 7, 0).unwrap();
            m.set_island(b, 7, 0).unwrap();
        }
        let islands = partition_islands(&w.view());
        assert_eq!(islands.len(), 2, "{islands:?}");
        assert_eq!(islands[0].0, 0);
        assert_eq!(islands[1].0, 1);
    }

    #[test]
    fn phys_req_sleeper_is_a_seed() {
        let mut w = world();
        let s = relic(1);
        plant(&mut w, s, 0, 12);
        w.mutate()
            .set_phys_req(
                s,
                PhysRequest {
                    lin: IVec3 { x: 1, y: 0, z: 0 },
                    ang: IVec3::ZERO,
                },
            )
            .unwrap();
        let islands = partition_islands(&w.view());
        assert_eq!(islands, vec![(0, vec![s])]);
    }

    #[test]
    fn nonzero_vel_sleeper_is_a_seed() {
        let mut w = world();
        let s = relic(1);
        plant(&mut w, s, 0, 12);
        w.mutate()
            .set_vel(
                s,
                Vel3::new(VelFx::from_mm_per_tick(1), VelFx::ZERO, VelFx::ZERO),
                0,
            )
            .unwrap();
        let islands = partition_islands(&w.view());
        assert_eq!(islands, vec![(0, vec![s])]);
    }

    #[test]
    fn attached_to_awake_sleeper_is_a_seed() {
        let mut w = world();
        let parent = relic(1);
        let child = relic(2);
        plant(&mut w, parent, 0, 0);
        plant(&mut w, child, 50_000, 12);
        w.mutate().add_rel(child, Rel::AttachedTo, parent).unwrap();
        let islands = partition_islands(&w.view());
        assert_eq!(islands.len(), 2, "{islands:?}");
        assert!(islands.iter().any(|(_, m)| m == &vec![child]));
    }

    #[test]
    fn isolated_sleeper_is_not_an_island() {
        let mut w = world();
        let s = relic(1);
        plant(&mut w, s, 0, 12);
        let islands = partition_islands(&w.view());
        assert!(islands.is_empty(), "{islands:?}");
    }
}
