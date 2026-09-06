//! Runtime halt boundary for applying an offline Canon epoch pack.

use std::sync::Arc;

use klotho_commit::{CommitKernel, EpochApplyError, TraceDelta};
use klotho_compile::CanonEpochPack;
#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
use klotho_net::Server;

/// Exclusive halted runtime phase for a Canon epoch transition.
///
/// Holding this guard mutably borrows the kernel, so the sim cannot call
/// `step`. Dropping or consuming it resumes normal runtime ownership.
pub struct HaltedEpoch<'a> {
    kernel: &'a mut CommitKernel,
}

/// Stop stepping `kernel` and enter the epoch-apply phase.
pub fn halt_for_epoch(kernel: &mut CommitKernel) -> HaltedEpoch<'_> {
    HaltedEpoch { kernel }
}

/// Halt a dedicated server and its kernel under one exclusive transition.
#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
pub fn halt_server_for_epoch<'a>(
    kernel: &'a mut CommitKernel,
    server: &'a mut Server,
) -> HaltedServerEpoch<'a> {
    HaltedServerEpoch { kernel, server }
}

/// Exclusive dedicated-server epoch phase. Clients are required to rejoin.
#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
pub struct HaltedServerEpoch<'a> {
    kernel: &'a mut CommitKernel,
    server: &'a mut Server,
}

/// Failure to apply an epoch consistently to the kernel and net authority.
#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ServerEpochApplyError {
    /// The server was not advertising the pack's base identity.
    ServerMismatch,
    /// The sole mutator refused the pack before writing.
    Kernel(EpochApplyError),
}

#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
impl core::fmt::Display for ServerEpochApplyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ServerMismatch => write!(f, "ServerMismatch"),
            Self::Kernel(error) => write!(f, "Kernel({error})"),
        }
    }
}

#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
impl core::error::Error for ServerEpochApplyError {}

#[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
impl HaltedServerEpoch<'_> {
    /// Commit `pack`, update Hello identity, and evict all joined clients so
    /// they must download the pack and rejoin (or disconnect).
    pub fn apply(&mut self, pack: &CanonEpochPack) -> Result<TraceDelta, ServerEpochApplyError> {
        if self.server.canon_hash() != pack.from_canon_hash
            || self.server.epoch() != pack.from_epoch
        {
            return Err(ServerEpochApplyError::ServerMismatch);
        }
        let delta = self
            .kernel
            .apply_canon_epoch(
                pack.from_canon_hash,
                pack.from_epoch,
                pack.canon_hash,
                pack.epoch,
                Arc::new(pack.canon.clone()),
                &pack.map,
            )
            .map_err(ServerEpochApplyError::Kernel)?;
        self.server
            .install_epoch(pack.canon_hash, pack.epoch)
            .expect("kernel and server epoch successor were prevalidated");
        self.server
            .set_prefix(self.kernel.world().trace_prefix_hash());
        Ok(delta)
    }

    /// Finish the halt phase.
    pub fn resume(self) {}
}

impl HaltedEpoch<'_> {
    /// Apply one validated pack. Pending old-Canon proposals are discarded by
    /// the CommitKernel; the returned delta contains any evicted Rite ends.
    pub fn apply(&mut self, pack: &CanonEpochPack) -> Result<TraceDelta, EpochApplyError> {
        self.kernel.apply_canon_epoch(
            pack.from_canon_hash,
            pack.from_epoch,
            pack.canon_hash,
            pack.epoch,
            Arc::new(pack.canon.clone()),
            &pack.map,
        )
    }

    /// Finish the halt phase and return the sole mutator to the caller.
    pub fn resume(self) {}
}

#[cfg(test)]
mod tests {
    use klotho_compile::{cook_doc, cook_epoch_pack};
    use klotho_core::{Epoch, Hash, LocusKind, ResourceId, Tick};
    use klotho_ir::{
        CanonDiff, IntentDoc, Law, LawBody, Name, Pred, ProvenanceId, RiteGraph, RiteNode, RiteOp,
        SeedFact, Status, StyleIntent, Verb,
    };
    use klotho_save::{SaveError, check_load, pause_save};
    use klotho_trace::{RiteEnd, TraceBody, TraceEvent};

    use super::*;
    use crate::kernel_from_cooked;

    fn base_with_leading_resource() -> klotho_compile::Cooked {
        let doc = IntentDoc {
            style: StyleIntent::default(),
            canon_diffs: vec![
                CanonDiff::AddLaw(Law {
                    id: Name::from("live.remove_me"),
                    when: Pred::EqVerb(Verb::Use),
                    body: LawBody::Ramp {
                        res: Name::from("live_obsolete"),
                        per_tick: 0,
                        quantum: 1,
                        cap: 1,
                    },
                }),
                CanonDiff::AddLaw(Law {
                    id: Name::from("live.keep"),
                    when: Pred::EqVerb(Verb::Use),
                    body: LawBody::Ramp {
                        res: Name::from("health"),
                        per_tick: 0,
                        quantum: 1,
                        cap: 100,
                    },
                }),
                CanonDiff::AddRite(RiteGraph {
                    id: Name::from("live.wait"),
                    cap_steps: 4,
                    cap_ticks: 60,
                    entry: 0,
                    nodes: vec![
                        RiteNode::Op(RiteOp::Wait(30, None)),
                        RiteNode::Op(RiteOp::Halt(Status::Success)),
                    ],
                }),
            ],
            seed: vec![SeedFact::Locus {
                name: Name::from("player"),
                kind: LocusKind::Actor,
            }],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        };
        cook_doc(&doc).unwrap()
    }

    #[test]
    fn halt_remaps_resource_evicts_wait_and_invalidates_old_save() {
        let cooked = base_with_leading_resource();
        let old_health = cooked.canon.resource_id("health").unwrap();
        assert_ne!(old_health, ResourceId(0));
        let wait_rite = cooked.canon.rite_id("live.wait").unwrap();
        let player = cooked.canon.pin("player").unwrap();
        let mut kernel = kernel_from_cooked(&cooked).unwrap();
        kernel.world_mut().set_qty(player, old_health, 777).unwrap();
        kernel.world_mut().append(TraceEvent::new(
            Tick::ZERO,
            TraceBody::RiteBegan {
                actor: player,
                rite: wait_rite.0,
                target: None,
            },
        ));
        let old_snap = kernel.snapshot();
        let old_save = pause_save(&old_snap).unwrap();
        let old_prefix = old_snap.trace_prefix_hash;

        let pack = cook_epoch_pack(
            &cooked,
            Epoch::ZERO,
            &[
                CanonDiff::RetractLaw {
                    id: Name::from("live.remove_me"),
                    reason: "resource table repack".into(),
                },
                CanonDiff::RetractRite {
                    id: Name::from("live.wait"),
                    reason: "rite removed by live patch".into(),
                },
            ],
        )
        .unwrap();
        let new_health = pack.canon.resource_id("health").unwrap();
        assert_ne!(old_health, new_health);

        let mut halted = halt_for_epoch(&mut kernel);
        let delta = halted.apply(&pack).unwrap();
        halted.resume();

        assert_eq!(kernel.world().epoch(), Epoch(1));
        assert_eq!(kernel.world().canon_hash(), pack.canon_hash);
        assert_eq!(kernel.world().view().qty(player, new_health), 777);
        assert!(kernel.world().view().first_rite(player).is_none());
        assert!(delta.events.iter().any(|event| matches!(
            event.body,
            TraceBody::RiteEnded {
                actor,
                rite,
                status: RiteEnd::Evicted,
            } if actor == player && rite == wait_rite.0
        )));
        assert_eq!(
            check_load(&old_save, old_prefix, pack.canon_hash),
            Err(SaveError::CanonMismatch)
        );
    }

    #[test]
    fn wrong_ancestry_is_atomic() {
        let cooked = base_with_leading_resource();
        let mut kernel = kernel_from_cooked(&cooked).unwrap();
        let mut pack = cook_epoch_pack(&cooked, Epoch::ZERO, &[]).unwrap();
        pack.from_epoch = Epoch(9);
        let before_hash = kernel.world().canon_hash();
        let before_prefix = kernel.world().trace_prefix_hash();
        let mut halted = halt_for_epoch(&mut kernel);
        assert_eq!(halted.apply(&pack), Err(EpochApplyError::EpochMismatch));
        halted.resume();
        assert_eq!(kernel.world().canon_hash(), before_hash);
        assert_eq!(kernel.world().trace_prefix_hash(), before_prefix);
    }

    #[cfg(any(feature = "net", feature = "net-listen", feature = "net-dedicated"))]
    #[test]
    fn dedicated_halt_updates_hello_identity_and_prefix() {
        let cooked = base_with_leading_resource();
        let pack = cook_epoch_pack(&cooked, Epoch::ZERO, &[]).unwrap();
        let mut kernel = kernel_from_cooked(&cooked).unwrap();
        let mut server = Server::new(cooked.canon_hash, Epoch::ZERO, 60).unwrap();
        let mut halted = halt_server_for_epoch(&mut kernel, &mut server);
        halted.apply(&pack).unwrap();
        halted.resume();
        assert_eq!(server.canon_hash(), pack.canon_hash);
        assert_eq!(server.epoch(), Epoch(1));
        assert_eq!(server.prefix(), kernel.world().trace_prefix_hash());
    }
}
