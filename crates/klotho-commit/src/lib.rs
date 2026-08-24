//! CommitKernel: the only type that mutates committed Projection + Trace (K21).
//!
//! Each proposal is one transaction. A same-tick rite burst is one transaction.
//! `WAIT` commits and yields. Laws run on the would-be post-state; failure
//! discards the speculative delta.
//!
//! Enables `klotho-world/mutate`. Does not depend on space/motion/mind crates.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod admit;
mod kernel;
mod laws;
mod proposal;
mod rite;
mod swept;

pub use admit::{AdmitBuf, SyncProposer};
pub use kernel::CommitKernel;
pub use klotho_core::{KernelFault, RejectReason};
pub use klotho_trace::TraceDelta;
pub use proposal::Proposal;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{Budget, Hash, LocusKind, PlayerId, ResourceId, Sigil, Tick};
    use klotho_ir::{
        Agency, Analog, CanonDiff, Channel, IntentTarget, MindIntent, PlayerIntent, Verb, from_ron,
    };

    use super::*;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn cook(src: &str) -> klotho_canon::Canon {
        let d: Vec<CanonDiff> = from_ron(src).unwrap();
        cook_diffs(&d).unwrap()
    }

    fn kernel_with(src: &str, stamina: i32) -> (CommitKernel, Sigil, ResourceId) {
        let canon = cook(src);
        let stamina_id = canon.resource_id("stamina").unwrap_or(ResourceId(0));
        let mut k = CommitKernel::new(klotho_world::World::new(Arc::new(canon), Hash::ZERO));
        let s = actor(1);
        k.bind_player(PlayerId(0), s);
        k.world_mut().insert_locus(s, LocusKind::Actor).unwrap();
        if stamina != 0 {
            k.world_mut().set_qty(s, stamina_id, stamina).unwrap();
        }
        (k, s, stamina_id)
    }

    fn player_use() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(0),
            at: Tick(0),
            verb: Verb::Use,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: Agency::none(),
        }
    }

    const SPEND_LAW: &str = r#"[
        AddLaw(Law(id: "need.stamina", when: EqVerb(Use), body: Pred(
            must: Qty(Self, "stamina", Ge, 1), ought: None))),
        AddRite(RiteGraph(id: "spend", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
            Spend("stamina", 10, 2),
            Complete(Success),
            Complete(Fail),
        ])),
    ]"#;

    #[test]
    fn spend_then_law_fail_leaves_qty_unchanged() {
        let (mut k, s, stamina) = kernel_with(SPEND_LAW, 10);
        k.ingest(Proposal::Player(player_use()));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(_, r)| matches!(r, RejectReason::Law(_))),
            "{d:?}"
        );
        assert_eq!(k.world().view().qty(s, stamina), 10);
        assert!(d.events.is_empty());
    }

    #[test]
    fn spend_without_blocking_law_commits() {
        let src = r#"[
            AddRite(RiteGraph(id: "spend", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Spend("stamina", 10, 2),
                Complete(Success),
                Complete(Fail),
            ])),
        ]"#;
        let (mut k, s, stamina) = kernel_with(src, 10);
        k.ingest(Proposal::Player(player_use()));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert_eq!(k.world().view().qty(s, stamina), 0);
    }

    #[test]
    fn wait_channel_rejects_mind() {
        let src = r#"[
            AddRite(RiteGraph(id: "lock", cap_steps: 8, cap_ticks: 180, entry: 0, nodes: [
                Wait(45, Some(Timing)),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, s, _) = kernel_with(src, 0);
        k.ingest(Proposal::Player(player_use()));
        let d1 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d1.rejects.is_empty(), "{d1:?}");
        assert!(k.world().view().first_rite(s).is_some());

        k.ingest(Proposal::Mind(MindIntent {
            locus: s,
            verb: Verb::Use,
            target: IntentTarget::None,
            utility: 0,
        }));
        let d2 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d2.rejects
                .iter()
                .any(|(_, r)| *r == RejectReason::UnclaimedAgency),
            "{d2:?}"
        );
    }

    #[test]
    fn player_timing_resumes_wait() {
        let src = r#"[
            AddRite(RiteGraph(id: "lock", cap_steps: 8, cap_ticks: 180, entry: 0, nodes: [
                Wait(45, Some(Timing)),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, s, _) = kernel_with(src, 0);
        k.ingest(Proposal::Player(player_use()));
        k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        let mut p = player_use();
        p.verb = Verb::Time;
        p.agency = Agency {
            claimed: vec![Channel::Timing],
            assist: Default::default(),
        };
        k.ingest(Proposal::Player(p));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().first_rite(s).is_none());
    }

    #[test]
    fn carry_does_not_start_lockpick() {
        let src = r#"[
            AddRite(RiteGraph(id: "lockpick", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Wait(45, Some(Timing)),
                Complete(Success),
            ])),
            AddRite(RiteGraph(id: "carry.pick", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                RelAdd(Target, WieldedBy, Self),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, s, _) = kernel_with(src, 0);
        let barrel = actor(2);
        k.world_mut()
            .insert_locus(barrel, LocusKind::Relic)
            .unwrap();
        let mut p = player_use();
        p.verb = Verb::Carry;
        p.target = IntentTarget::Sigil(barrel);
        k.ingest(Proposal::Player(p));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().first_rite(s).is_none());
        assert!(
            k.world()
                .view()
                .has_rel(barrel, klotho_ir::Rel::WieldedBy, s)
        );
    }

    #[test]
    fn cap_rejects_ninth_marked_locus() {
        let src = r#"[
            AddLaw(Law(id: "fire.bound", when: EqVerb(Use),
                body: Cap(mark: Qty(Self, "heat", Ge, 400), n: 1, require_rel: None))),
            AddRite(RiteGraph(id: "ignite", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                Bind(Target),
                Setq(Target, "heat", 400),
                Complete(Success),
            ])),
        ]"#;
        let (mut k, _, _) = kernel_with(src, 0);
        let a = actor(2);
        let b = actor(3);
        k.world_mut().insert_locus(a, LocusKind::Relic).unwrap();
        k.world_mut().insert_locus(b, LocusKind::Relic).unwrap();
        let mut p = player_use();
        p.target = IntentTarget::Sigil(a);
        k.ingest(Proposal::Player(p.clone()));
        let d1 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d1.rejects.is_empty(), "{d1:?}");
        p.target = IntentTarget::Sigil(b);
        k.ingest(Proposal::Player(p));
        let d2 = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d2.rejects
                .iter()
                .any(|(_, r)| matches!(r, RejectReason::Law(_))),
            "{d2:?}"
        );
    }

    #[test]
    fn player_without_channel_may_resume_wait() {
        let src = r#"[
            AddRite(RiteGraph(id: "trade.offer", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
                { pc: 0, op: Wait(12, Some(DialogueChoice)) },
                { pc: 1, op: Guard(AgencyClaimed(DialogueChoice), fail: 3) },
                { pc: 2, op: Complete(Success) },
                { pc: 3, op: Complete(Fail) },
            ])),
        ]"#;
        let (mut k, s, _) = kernel_with(src, 0);
        let mut talk = player_use();
        talk.verb = Verb::Talk;
        k.ingest(Proposal::Player(talk.clone()));
        k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(k.world().view().first_rite(s).is_some());
        k.ingest(Proposal::Player(talk));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().first_rite(s).is_none());
    }
}
