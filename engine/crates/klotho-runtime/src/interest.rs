//! Apply [`klotho_interest`] onto the live world. Runtime-only (K49).

use klotho_commit::CommitKernel;
use klotho_interest::{InterestConfig, classify};

/// Write SimLod from a pure interest pass. Does not emit Residency proposals.
pub fn apply_interest(kernel: &mut CommitKernel, cfg: &InterestConfig) {
    let interest = classify(&kernel.world().view(), cfg);
    let mut w = kernel.world_mut();
    for (s, lod) in interest.lod {
        let _ = w.set_sim_lod(s, lod);
    }
}

#[cfg(test)]
mod tests {
    use hearth_slice::boot;
    use klotho_core::SimLod;

    use super::*;

    #[test]
    fn hearth_interest_keeps_player_full() {
        let mut k = boot();
        apply_interest(&mut k, &InterestConfig::default());
        let player = k.world().view().loci().next().expect("locus");
        assert_eq!(k.world().view().sim_lod(player), SimLod::Full);
    }
}
