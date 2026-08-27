//! Jobs path: partition, propose_island, concat, ingest. Sync proposers stay
//! on `CommitKernel::step` for Hearth.

use std::time::Instant;

use klotho_commit::{CommitKernel, IslandProposer};
use klotho_jobs::propose_islands;

/// Static registration order. Gameplay never schedules jobs (K46).
pub fn ingest_island_jobs(
    kernel: &mut CommitKernel,
    n_workers: usize,
    proposers: &[&dyn IslandProposer],
) -> u32 {
    let t0 = Instant::now();
    let islands = kernel.partition();
    let batch = {
        let view = kernel.world().view();
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
        let space = Space;
        let _us = ingest_island_jobs(&mut k, 1, &[&space]);
        let _us8 = ingest_island_jobs(&mut k, 8, &[&space]);
    }
}
