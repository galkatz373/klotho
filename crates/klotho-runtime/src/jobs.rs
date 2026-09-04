//! Jobs path: partition, propose_island, concat, ingest. Sync proposers stay
//! on `CommitKernel::step` for Hearth.

use std::collections::BTreeMap;
use std::time::Instant;

use klotho_commit::{CommitKernel, IslandProposer};
use klotho_core::{NO_ISLAND, Sigil};
use klotho_jobs::propose_islands;

/// Static registration order. Gameplay never schedules jobs (K46).
///
/// Callers compose `proposers` explicitly — pass `&klotho_phys::Phys` when
/// physics is desired. Nothing is appended implicitly: an implicit Phys
/// plus an explicit one would only manufacture Conflict nacks.
///
/// The caller must have partitioned this tick (`Sim::tick` always runs
/// `phase_partition` before jobs). This groups the live island assignment
/// instead of re-running the union-find: a second `partition()` would redo
/// identical work for an identical result.
pub fn ingest_island_jobs(
    kernel: &mut CommitKernel,
    n_workers: usize,
    proposers: &[&dyn IslandProposer],
) -> u32 {
    let t0 = Instant::now();
    let batch = {
        let view = kernel.world().view();
        let mut groups: BTreeMap<u16, Vec<Sigil>> = BTreeMap::new();
        for s in view.loci() {
            if let Some((id, _)) = view.island(s) {
                if id != NO_ISLAND {
                    groups.entry(id).or_default().push(s);
                }
            }
        }
        let islands: Vec<(u16, Vec<Sigil>)> = groups.into_iter().collect();
        propose_islands(n_workers, &islands, proposers, &view)
    };
    for (p, ix) in batch {
        kernel.ingest_from(p, ix);
    }
    u32::try_from(t0.elapsed().as_micros()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use hearth_slice::boot;
    use klotho_space::Space;

    use super::*;

    #[test]
    fn hearth_jobs_path_is_wired() {
        let mut k = boot();
        k.partition();
        let space = Space;
        let _us = ingest_island_jobs(&mut k, 1, &[&space]);
        let _us8 = ingest_island_jobs(&mut k, 8, &[&space]);
    }
}
