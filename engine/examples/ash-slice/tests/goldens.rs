//! Appendix B goldens 1–6. Same `klotho-commit` binary as Hearth (K26).

use std::sync::Arc;

use ash_slice::{boot, pin, plant_projectiles, replay};
use klotho_canon::{cook, cook_diffs};
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{Budget, Hash, RejectReason, Tick};
use klotho_ir::{CanonDiff, InferIntent, IntentTarget, ModelId, Name, Rel, Verb, from_ron};
use klotho_world::World;

fn intents(src: &str) -> Vec<klotho_ir::PlayerIntent> {
    from_ron(src).expect("PlayerIntent RON")
}

#[test]
fn golden_01_fire_spends_ammo() {
    let mut k = boot();
    let player = pin(&k, "player");
    let ammo = k.canon().resource_id("ammo").unwrap();
    assert_eq!(k.world().view().qty(player, ammo), 10);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_01_fire.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert_eq!(k.world().view().qty(player, ammo), 9);
}

#[test]
fn golden_02_hit_reduces_health() {
    let mut k = boot();
    let dummy = pin(&k, "dummy_0");
    let health = k.canon().resource_id("health").unwrap();
    assert_eq!(k.world().view().qty(dummy, health), 100);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_01_fire.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert_eq!(k.world().view().qty(dummy, health), 75);
}

#[test]
fn golden_03_zero_health_is_dead() {
    let mut k = boot();
    let dummy = pin(&k, "dummy_0");
    let health = k.canon().resource_id("health").unwrap();
    let fire = intents(include_str!("../fixtures/golden_01_fire.ron"));
    for _ in 0..4 {
        let ds = replay(&mut k, &fire);
        assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    }
    assert_eq!(k.world().view().qty(dummy, health), 0);
    assert!(k.world().view().has_rel(dummy, Rel::Dead, dummy));
}

#[test]
fn golden_04_101st_projectile_cap_rejected() {
    let mut k = boot();
    plant_projectiles(&mut k, 101);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_01_fire.ron")),
    );
    assert!(
        ds.iter().any(|d| d
            .rejects
            .iter()
            .any(|(_, r)| matches!(r, RejectReason::Law(_)))),
        "{ds:?}"
    );
}

#[test]
fn golden_05_infer_fire_is_mind_like_without_player_channel() {
    let mut k = boot();
    let player = pin(&k, "player");
    let ammo = k.canon().resource_id("ammo").unwrap();
    k.ingest(Proposal::Infer(InferIntent {
        model: ModelId(Name::from("ash-fill")),
        locus: Some(player),
        verb: Verb::Fire,
        target: IntentTarget::Name(Name::from("dummy_0")),
        claimed_facts: Vec::new(),
    }));
    let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
    assert!(d.rejects.is_empty(), "{d:?}");
    assert_eq!(k.world().view().qty(player, ammo), 9);
}

#[test]
fn golden_06_same_kernel_binary_as_hearth() {
    let _ash = boot();
    let hearth: Vec<CanonDiff> = from_ron(include_str!(
        "../../../crates/klotho-canon/fixtures/hearth_diffs.ron"
    ))
    .unwrap();
    let canon = cook_diffs(&hearth).unwrap();
    let _hearth = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    let ash_doc = ash_slice::ash_doc();
    let _ = cook(&ash_doc).unwrap();
    let _: fn(World) -> CommitKernel = CommitKernel::new;
}
