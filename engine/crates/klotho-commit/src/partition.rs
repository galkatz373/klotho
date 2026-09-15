//! This-tick island partition (K58). Pure `F(view)`; not last-tick memory.
//!
//! Occupancy (`space_ix`, including Dormant / OpaqueClosed) is for blocking.
//! Island members are **phys bodies** only. Idle scenery and Place floors
//! stay out of the contact graph so a city of touching walls is not one
//! island. Oversize / too-many groups fail closed (omit, do not split).

use std::collections::BTreeMap;

use klotho_core::{
    AabbMm, BodyMode, LocusKind, MAX_ISLAND_SIZE, MAX_ISLANDS, Sigil, SimLod, Vel3, is_phys_awake,
};
use klotho_ir::Rel;
use klotho_world::WorldView;

/// Result of one K58 pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Partition {
    /// Dense-ranked contact groups, id `0..n`.
    pub islands: Vec<(u16, Vec<Sigil>)>,
    /// Groups omitted because `len > MAX_ISLAND_SIZE` (whole group, not split).
    pub omitted_too_large: u32,
    /// Groups omitted after [`MAX_ISLANDS`] (min-Sigil rank, fail closed).
    pub omitted_too_many: u32,
}

/// Union-find on **phys-body** hulls this tick.
///
/// Seed = [`SimLod::Full`] phys bodies that are awake (`sleep_ticks < 120`)
/// **or** have non-zero vel **or** `phys_req` **or** are `PilotedBy`/`AttachedTo`
/// an awake locus. Intact Canon joints also union their endpoints. Flood-fill
/// through overlapping **phys-body** hulls including sleepers (crate piles),
/// never through idle `OpaqueClosed` scenery or [`LocusKind::Place`] floors.
///
/// Island id = dense rank of min-Sigil. Oversize / extra groups are omitted
/// (fail closed), not split and not silently truncated into a live island.
#[must_use]
pub fn partition_islands(view: &WorldView<'_>) -> Partition {
    partition_with_caps(view, MAX_ISLANDS, MAX_ISLAND_SIZE)
}

#[must_use]
pub(crate) fn partition_with_caps(
    view: &WorldView<'_>,
    max_islands: u16,
    max_size: u16,
) -> Partition {
    let mut hulls: Vec<(Sigil, AabbMm)> = Vec::new();
    for s in view.loci() {
        if !is_phys_body(view, s) {
            continue;
        }
        if let Some(aabb) = posed_bounds(view, s) {
            hulls.push((s, aabb));
        }
    }
    if hulls.is_empty() {
        return Partition::default();
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
                let Some(other) = hulls.get(j as usize).map(|h| h.1) else {
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

    // Intact Canon joints couple bodies that need not overlap.
    let mut progressed = true;
    while progressed {
        progressed = false;
        for (id, joint) in view.constraints() {
            if view.constraint_state(id).is_some_and(|s| s.broken) {
                continue;
            }
            let Some(&ia) = ix_of.get(&joint.a) else {
                continue;
            };
            let Some(&ib) = ix_of.get(&joint.b) else {
                continue;
            };
            if reached[ia as usize] || reached[ib as usize] {
                union(&mut parent, &mut rank, ia, ib);
            }
            if reached[ia as usize] && !reached[ib as usize] {
                reached[ib as usize] = true;
                progressed = true;
            }
            if reached[ib as usize] && !reached[ia as usize] {
                reached[ia as usize] = true;
                progressed = true;
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

    let mut ok: Vec<(Sigil, Vec<Sigil>)> = Vec::new();
    let mut omitted_too_large = 0u32;
    let max_size_usize = usize::from(max_size);
    for mut members in groups.into_values() {
        members.sort_unstable();
        let min = members[0];
        if members.len() > max_size_usize {
            omitted_too_large = omitted_too_large.saturating_add(1);
            continue;
        }
        ok.push((min, members));
    }
    ok.sort_unstable_by_key(|(min, _)| *min);

    let cap = usize::from(max_islands);
    let omitted_too_many = u32::try_from(ok.len().saturating_sub(cap)).unwrap_or(u32::MAX);
    ok.truncate(cap);

    Partition {
        islands: ok
            .into_iter()
            .enumerate()
            .map(|(i, (_, members))| (i as u16, members))
            .collect(),
        omitted_too_large,
        omitted_too_many,
    }
}

fn posed_bounds(view: &WorldView<'_>, s: Sigil) -> Option<AabbMm> {
    let local = view.hull(s)?;
    let pose = view.pose(s)?;
    let shape = klotho_geom::cooked_shape(view.body_physics(s).shape, local).ok()?;
    let mut aabb = klotho_geom::bounds(shape, pose).ok()?;
    if let Some(policy) = view.character_physics(s) {
        let extent = policy
            .roots
            .iter()
            .map(|r| r.x.unsigned_abs().max(r.z.unsigned_abs()))
            .max()
            .unwrap_or(0) as i32;
        let pad = extent.saturating_add(1000); // bounded platform carry envelope
        aabb.min.x = aabb.min.x.saturating_sub(pad);
        aabb.min.z = aabb.min.z.saturating_sub(pad);
        aabb.max.x = aabb.max.x.saturating_add(pad);
        aabb.max.z = aabb.max.z.saturating_add(pad);
        aabb.min.y = aabb.min.y.saturating_sub(policy.step_mm + 3);
        aabb.max.y = aabb.max.y.saturating_add(policy.step_mm);
    }
    Some(aabb)
}

/// Actor, or a Relic that is not idle kinematic scenery.
fn is_phys_body(view: &WorldView<'_>, s: Sigil) -> bool {
    if view.body_physics(s).mode == BodyMode::Static {
        return false;
    }
    match s.kind() {
        Some(LocusKind::Actor) => true,
        Some(LocusKind::Relic) => {
            (view.body_physics(s).mode == BodyMode::Dynamic
                || view
                    .vel(s)
                    .is_some_and(|(v, rate)| v != Vel3::ZERO || rate != 0)
                || view.rates(s).is_some_and(|r| r != (0, 0, 0)))
                && (!is_idle_scenery(view, s) || is_attach_node(view, s))
        }
        _ => false,
    }
}

/// Idle `OpaqueClosed` relic: locked door / static wall. Occupancy only.
fn is_idle_scenery(view: &WorldView<'_>, s: Sigil) -> bool {
    if !view.opaque_closed(s) {
        return false;
    }
    let sleep = view.island(s).map(|(_, t)| t).unwrap_or(0);
    if sleep != 0 {
        return false;
    }
    if view.phys_req(s).is_some() {
        return false;
    }
    if let Some((vel, yaw_rate)) = view.vel(s) {
        if vel != Vel3::ZERO || yaw_rate != 0 {
            return false;
        }
    }
    true
}

fn is_attach_node(view: &WorldView<'_>, s: Sigil) -> bool {
    for rel in [Rel::PilotedBy, Rel::AttachedTo] {
        if view.related(s, rel).next().is_some() {
            return true;
        }
    }
    false
}

fn is_seed(view: &WorldView<'_>, s: Sigil) -> bool {
    if view.sim_lod(s) != SimLod::Full {
        return false;
    }
    if !is_phys_body(view, s) {
        return false;
    }
    let sleep = view.island(s).map(|(_, t)| t).unwrap_or(0);
    if is_phys_awake(sleep) {
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
            if is_phys_awake(sleep) {
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
        AabbMm, BlobId, Hash, IVec3, LocusKind, Mm, PhysRequest, PoseMm, SLEEP_AFTER_TICKS, Sigil,
        SimLod, Vel3, VelFx, YawMd,
    };
    use klotho_ir::{CanonDiff, Rel, from_ron};
    use klotho_world::World;

    use super::*;

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
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

    fn opaque_world() -> World {
        let d: Vec<CanonDiff> = from_ron(
            r#"[AddAffordance(Affordance(id: "Opaque", requires: [], grants: [], conflicts: []))]"#,
        )
        .unwrap();
        World::new(Arc::new(cook_diffs(&d).unwrap()), Hash::ZERO)
    }

    fn plant(w: &mut World, s: Sigil, x: i32, sleep: u16) {
        let mut m = w.mutate();
        m.insert_locus(s, s.kind().unwrap()).unwrap();
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
        let part = partition_islands(&w.view());
        assert_eq!(part.omitted_too_large, 0);
        assert_eq!(part.omitted_too_many, 0);
        assert_eq!(part.islands.len(), 3);
        assert_eq!(part.islands[0], (0, vec![b]));
        assert_eq!(part.islands[1], (1, vec![c]));
        assert_eq!(part.islands[2], (2, vec![a]));
    }

    #[test]
    fn sleeper_pile_joins_bumped_awake() {
        let mut w = world();
        let awake = relic(1);
        let sleep_near = relic(2);
        let sleep_far = relic(3);
        plant(&mut w, awake, 0, 0);
        plant(&mut w, sleep_near, 150, SLEEP_AFTER_TICKS);
        plant(&mut w, sleep_far, 50_000, SLEEP_AFTER_TICKS);
        let part = partition_islands(&w.view());
        assert_eq!(part.islands.len(), 1, "{part:?}");
        assert_eq!(part.islands[0].1, vec![awake, sleep_near]);
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
        let part = partition_islands(&w.view());
        assert_eq!(part.islands.len(), 2, "{part:?}");
        assert_eq!(part.islands[0].0, 0);
        assert_eq!(part.islands[1].0, 1);
    }

    #[test]
    fn phys_req_sleeper_is_a_seed() {
        let mut w = world();
        let s = relic(1);
        plant(&mut w, s, 0, SLEEP_AFTER_TICKS);
        w.mutate()
            .set_phys_req(
                s,
                PhysRequest {
                    lin: IVec3 { x: 1, y: 0, z: 0 },
                    ang: IVec3::ZERO,
                },
            )
            .unwrap();
        let part = partition_islands(&w.view());
        assert_eq!(part.islands, vec![(0, vec![s])]);
    }

    #[test]
    fn nonzero_vel_sleeper_is_a_seed() {
        let mut w = world();
        let s = relic(1);
        plant(&mut w, s, 0, SLEEP_AFTER_TICKS);
        w.mutate()
            .set_vel(
                s,
                Vel3::new(VelFx::from_mm_per_tick(1), VelFx::ZERO, VelFx::ZERO),
                0,
            )
            .unwrap();
        let part = partition_islands(&w.view());
        assert_eq!(part.islands, vec![(0, vec![s])]);
    }

    #[test]
    fn attached_to_awake_sleeper_is_a_seed() {
        let mut w = world();
        let parent = relic(1);
        let child = relic(2);
        plant(&mut w, parent, 0, 0);
        plant(&mut w, child, 50_000, SLEEP_AFTER_TICKS);
        w.mutate().add_rel(child, Rel::AttachedTo, parent).unwrap();
        let part = partition_islands(&w.view());
        assert_eq!(part.islands.len(), 2, "{part:?}");
        assert!(part.islands.iter().any(|(_, m)| m == &vec![child]));
    }

    #[test]
    fn isolated_sleeper_is_not_an_island() {
        let mut w = world();
        let s = relic(1);
        plant(&mut w, s, 0, SLEEP_AFTER_TICKS);
        let part = partition_islands(&w.view());
        assert!(part.islands.is_empty(), "{part:?}");
    }

    #[test]
    fn dormant_idle_body_is_not_a_seed() {
        let mut w = world();
        let s = relic(1);
        plant(&mut w, s, 0, 0);
        w.mutate().set_sim_lod(s, SimLod::Dormant).unwrap();
        let part = partition_islands(&w.view());
        assert!(part.islands.is_empty(), "{part:?}");
    }

    #[test]
    fn dormant_sleeper_still_joins_full_awake_overlap() {
        let mut w = world();
        let awake = relic(1);
        let sleeper = relic(2);
        plant(&mut w, awake, 0, 0);
        plant(&mut w, sleeper, 150, SLEEP_AFTER_TICKS);
        w.mutate().set_sim_lod(sleeper, SimLod::Dormant).unwrap();
        let part = partition_islands(&w.view());
        assert_eq!(part.islands, vec![(0, vec![awake, sleeper])]);
    }

    #[test]
    fn opaque_closed_wall_is_occupancy_not_island_member() {
        let mut w = opaque_world();
        let body = actor(1);
        let wall = relic(2);
        plant(&mut w, body, 0, 0);
        plant(&mut w, wall, 150, 0);
        let opaque = w.canon().affordance_id("Opaque").unwrap();
        {
            let mut m = w.mutate();
            m.set_affordance(wall, opaque, true).unwrap();
            m.add_rel(wall, Rel::LockedBy, wall).unwrap();
        }
        assert!(w.view().opaque_closed(wall));
        let part = partition_islands(&w.view());
        assert_eq!(part.islands, vec![(0, vec![body])], "{part:?}");
    }

    #[test]
    fn place_floor_is_not_an_island_member() {
        let mut w = world();
        let body = actor(1);
        let floor = place(2);
        plant(&mut w, body, 0, 0);
        plant(&mut w, floor, 0, 0);
        let part = partition_islands(&w.view());
        assert_eq!(part.islands, vec![(0, vec![body])], "{part:?}");
    }

    #[test]
    fn oversize_group_is_omitted_not_split() {
        let mut w = world();
        for i in 0..3u128 {
            plant(&mut w, relic(i + 1), (i as i32) * 150, 0);
        }
        let part = partition_with_caps(&w.view(), MAX_ISLANDS, 2);
        assert!(part.islands.is_empty(), "{part:?}");
        assert_eq!(part.omitted_too_large, 1);
        assert_eq!(part.omitted_too_many, 0);
    }

    #[test]
    fn extra_islands_fail_closed_not_truncated_into_live() {
        let mut w = world();
        plant(&mut w, relic(1), 0, 0);
        plant(&mut w, relic(2), 50_000, 0);
        plant(&mut w, relic(3), 100_000, 0);
        let part = partition_with_caps(&w.view(), 1, MAX_ISLAND_SIZE);
        assert_eq!(part.islands.len(), 1);
        assert_eq!(part.islands[0].1, vec![relic(1)]);
        assert_eq!(part.omitted_too_many, 2);
        assert_eq!(part.omitted_too_large, 0);
    }

    #[test]
    fn intact_constraint_joins_non_overlapping_bodies() {
        let a = relic(1);
        let b = relic(2);
        let id = relic(9);
        let mut canon = cook_diffs(&from_ron::<Vec<CanonDiff>>("[]").unwrap()).unwrap();
        assert!(canon.bind_constraint(
            id,
            klotho_core::ConstraintPhysics {
                a,
                b,
                binding: BlobId::from_bytes([7; 32]),
                ..klotho_core::ConstraintPhysics::default()
            }
        ));
        let mut w = World::new(Arc::new(canon), Hash::ZERO);
        plant(&mut w, a, 0, 0);
        plant(&mut w, b, 50_000, 0);
        let part = partition_islands(&w.view());
        assert_eq!(part.islands.len(), 1, "{part:?}");
        assert_eq!(part.islands[0].1, vec![a, b]);
    }
}
