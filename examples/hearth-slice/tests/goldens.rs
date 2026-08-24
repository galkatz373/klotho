//! Appendix A goldens 1–7. Golden 8 needs PR 10 (space admission).

use hearth_slice::{boot, pin, replay};
use klotho_commit::Proposal;
use klotho_core::{Budget, LocusKind, RejectReason, Tick};
use klotho_ir::{FactId, InferIntent, IntentTarget, ModelId, Name, Rel, Verb, from_ron};

fn intents(src: &str) -> Vec<klotho_ir::PlayerIntent> {
    from_ron(src).expect("PlayerIntent RON")
}

#[test]
fn golden_01_lockpick_two_windows_unlocks() {
    let mut k = boot();
    let door = pin(&k, "oak_door");
    assert!(k.world().view().has_rel(door, Rel::LockedBy, door));
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_01_lockpick.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert!(!k.world().view().has_rel(door, Rel::LockedBy, door));
    let unlocked = k.world().trace_prefix_hash();
    assert_ne!(unlocked, klotho_core::Hash::ZERO);
}

#[test]
fn golden_02_lockpick_infer_spoof_unclaimed_agency() {
    let mut k = boot();
    replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_02_lockpick_start.ron")),
    );
    let player = pin(&k, "player");
    assert!(k.world().view().first_rite(player).is_some());
    k.ingest(Proposal::Infer(InferIntent {
        model: ModelId(Name::from("spoof")),
        locus: Some(player),
        verb: Verb::Use,
        target: IntentTarget::Name(Name::from("oak_door")),
        claimed_facts: Vec::<FactId>::new(),
    }));
    let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| *r == RejectReason::UnclaimedAgency),
        "{d:?}"
    );
}

#[test]
fn golden_03_carry_barrel_mass_conserved() {
    let mut k = boot();
    let player = pin(&k, "player");
    let barrel = pin(&k, "barrel_0");
    let mass = k.canon().resource_id("mass_g").unwrap();
    let before_p = k.world().view().qty(player, mass);
    let before_b = k.world().view().qty(barrel, mass);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_03_carry.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert!(k.world().view().has_rel(barrel, Rel::WieldedBy, player));
    assert_eq!(k.world().view().qty(player, mass), before_p);
    assert_eq!(k.world().view().qty(barrel, mass), before_b);
}

#[test]
fn golden_04_ninth_ignite_cap_rejected() {
    let mut k = boot();
    let heat = k.canon().resource_id("heat").unwrap();
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_04_ignite.ron")),
    );
    assert_eq!(ds.len(), 9);
    for d in &ds[..8] {
        assert!(d.rejects.is_empty(), "{d:?}");
    }
    assert!(
        ds[8]
            .rejects
            .iter()
            .any(|(_, r)| matches!(r, RejectReason::Law(_))),
        "{:?}",
        ds[8]
    );
    let mut burning = 0;
    for i in 0..9 {
        let b = pin(&k, &format!("barrel_{i}"));
        if k.world().view().qty(b, heat) >= 400 {
            burning += 1;
        }
    }
    assert_eq!(burning, 8);
}

#[test]
fn golden_05_trade_hammer_rejected() {
    let mut k = boot();
    let hammer = pin(&k, "fathers_hammer");
    let player = pin(&k, "player");
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_05_trade_hammer.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert!(!k.world().view().has_rel(hammer, Rel::Owes, player));
}

#[test]
fn golden_06_trade_accept_owes_then_pay() {
    let mut k = boot();
    let ingot = pin(&k, "ingot");
    let player = pin(&k, "player");
    let copper = k.canon().resource_id("copper").unwrap();
    let before = k.world().view().qty(player, copper);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_06a_trade_accept.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert!(k.world().view().has_rel(ingot, Rel::Owes, player));
    assert_eq!(k.world().view().qty(player, copper), before);
}

#[test]
fn golden_06_trade_refuse_reldel() {
    let mut k = boot();
    let ingot = pin(&k, "ingot");
    let player = pin(&k, "player");
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_06b_trade_refuse.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert!(!k.world().view().has_rel(ingot, Rel::Owes, player));
}

#[test]
fn golden_07_player_carry_hammer_pride_reject() {
    let mut k = boot();
    let hammer = pin(&k, "fathers_hammer");
    let player = pin(&k, "player");
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_07_pride.ron")),
    );
    assert!(
        ds.iter().any(|d| d
            .rejects
            .iter()
            .any(|(_, r)| matches!(r, RejectReason::Law(_)))),
        "{ds:?}"
    );
    assert!(!k.world().view().has_rel(hammer, Rel::WieldedBy, player));
}

#[test]
fn appendix_a_cooks() {
    let k = boot();
    assert!(k.canon().rite_id("lockpick").is_some());
    assert!(k.canon().rite_id("carry.pick").is_some());
    assert!(k.canon().rite_id("trade.offer").is_some());
    let _ = LocusKind::Actor;
}
