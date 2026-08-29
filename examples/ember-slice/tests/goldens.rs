//! Ember goldens: melee WAIT, hit hulls, Cap 512, 64-fragment SPAWN. Same kernel binary.

use std::sync::Arc;

use ember_slice::{DUMMY_COUNT, boot, pin, plant_fragments, plant_projectiles, replay};
use klotho_canon::{cook, cook_diffs};
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{Budget, Hash, RejectReason, Tick, Vel3, VelFx};
use klotho_ir::{CanonDiff, InferIntent, IntentTarget, ModelId, Name, Rel, Verb, from_ron};
use klotho_motion::{ClipSet, Motion};
use klotho_trace::TraceBody;
use klotho_world::World;

fn intents(src: &str) -> Vec<klotho_ir::PlayerIntent> {
    from_ron(src).expect("PlayerIntent RON")
}

fn fragment_count(k: &CommitKernel) -> usize {
    let aff = k.canon().affordance_id("Fragment").unwrap();
    k.world()
        .view()
        .loci()
        .filter(|&s| k.world().view().has_affordance(s, aff))
        .count()
}

fn spawn_count(events: &[klotho_trace::TraceEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e.body, TraceBody::Spawned { .. }))
        .count()
}

#[test]
fn golden_01_melee_wait_windows_apply_damage() {
    let mut k = boot();
    let dummy = pin(&k, "dummy_0");
    let player = pin(&k, "player");
    let health = k.canon().resource_id("health").unwrap();
    let ammo = k.canon().resource_id("ammo").unwrap();
    assert_eq!(k.world().view().qty(dummy, health), 100);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_01_melee.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert_eq!(k.world().view().qty(dummy, health), 75);
    assert_eq!(k.world().view().qty(player, ammo), 10);
    assert!(k.world().view().first_rite(player).is_none());
}

#[test]
fn golden_02_infer_cannot_steal_melee_window() {
    let mut k = boot();
    replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_02_melee_start.ron")),
    );
    let player = pin(&k, "player");
    let dummy = pin(&k, "dummy_0");
    let health = k.canon().resource_id("health").unwrap();
    let machine = k.world().view().first_rite(player);
    assert!(machine.is_some());
    let pc = machine.unwrap().1.pc;
    for verb in [Verb::Use, Verb::Fire, Verb::Time] {
        k.ingest(Proposal::Infer(InferIntent {
            model: ModelId(Name::from("spoof")),
            locus: Some(player),
            verb,
            target: IntentTarget::Name(Name::from("dummy_0")),
            claimed_facts: Vec::new(),
        }));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            d.rejects
                .iter()
                .any(|(_, r)| *r == RejectReason::UnclaimedAgency),
            "verb={verb:?} {d:?}"
        );
    }
    let after = k.world().view().first_rite(player).expect("still waiting");
    assert_eq!(after.1.pc, pc);
    assert_eq!(k.world().view().qty(dummy, health), 100);
}

#[test]
fn golden_03_hit_volume_hull_damages_parent() {
    let mut k = boot();
    let dummy = pin(&k, "dummy_0");
    let hitbox = pin(&k, "dummy_0_hitbox");
    let health = k.canon().resource_id("health").unwrap();
    assert!(k.world().view().has_rel(hitbox, Rel::PartOf, dummy));
    let body = k.world().view().posed_hull(dummy).expect("body hull");
    let extra = k.world().view().posed_hull(hitbox).expect("extra hull");
    assert_ne!(body, extra);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_04_hitbox.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert_eq!(k.world().view().qty(dummy, health), 75);
}

#[test]
fn golden_04_513th_projectile_cap_rejected() {
    let mut k = boot();
    plant_projectiles(&mut k, 513);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_03_fire.ron")),
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
fn golden_05_64_fragment_collapse() {
    let mut k = boot();
    let crate_s = pin(&k, "crate");
    let integrity = k.canon().resource_id("integrity").unwrap();
    assert_eq!(k.world().view().qty(crate_s, integrity), 25);
    assert!(k.world().view().has_rel(crate_s, Rel::PartOf, crate_s));
    let before = k.world().view().loci().count();
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_05_collapse.ron")),
    );
    assert!(ds.iter().all(|d| d.rejects.is_empty()), "{ds:?}");
    assert_eq!(k.world().view().qty(crate_s, integrity), 0);
    assert!(!k.world().view().has_rel(crate_s, Rel::PartOf, crate_s));
    assert_eq!(fragment_count(&k), 64);
    assert_eq!(k.world().view().loci().count(), before + 64);
    let spawned: usize = ds.iter().map(|d| spawn_count(&d.events)).sum();
    assert_eq!(spawned, 64, "{ds:?}");
}

#[test]
fn golden_06_global_fragment_cap_128() {
    let mut k = boot();
    let crate_s = pin(&k, "crate");
    plant_fragments(&mut k, 128);
    assert_eq!(fragment_count(&k), 128);
    let ds = replay(
        &mut k,
        &intents(include_str!("../fixtures/golden_05_collapse.ron")),
    );
    assert!(
        ds.iter().any(|d| d
            .rejects
            .iter()
            .any(|(_, r)| matches!(r, RejectReason::Law(_)))),
        "{ds:?}"
    );
    assert!(k.world().view().has_rel(crate_s, Rel::PartOf, crate_s));
    assert_eq!(fragment_count(&k), 128);
}

#[test]
fn golden_07_clipset_swap_does_not_move_wait() {
    fn run(clips: ClipSet) -> (usize, i32, klotho_core::Mm) {
        let mut k = boot();
        let player = pin(&k, "player");
        k.world_mut()
            .set_vel(
                player,
                Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20)),
                0,
            )
            .unwrap();
        let dummy = pin(&k, "dummy_0");
        let health = k.canon().resource_id("health").unwrap();
        let mut motion = Motion::with_clips(clips);
        let packets = intents(include_str!("../fixtures/golden_01_melee.ron"));
        let mut hit_at = 0usize;
        for (i, pi) in packets.iter().enumerate() {
            let mut p = pi.clone();
            p.at = k.world().tick();
            k.ingest(Proposal::Player(p));
            let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut motion]).unwrap();
            assert!(d.rejects.is_empty(), "{d:?}");
            if k.world().view().qty(dummy, health) < 100 && hit_at == 0 {
                hit_at = i + 1;
            }
        }
        let hp = k.world().view().qty(dummy, health);
        let z = k.world().view().pose(player).unwrap().z;
        (hit_at, hp, z)
    }
    let hearth = run(ClipSet::hearth());
    let swapped = run(ClipSet::walk_mm(40));
    assert_eq!(hearth.0, swapped.0);
    assert_eq!(hearth.1, swapped.1);
    assert_eq!(hearth.1, 75);
    assert_ne!(hearth.2, swapped.2);
}

#[test]
fn golden_08_same_kernel_binary_as_hearth() {
    let _ember = boot();
    let hearth: Vec<CanonDiff> = from_ron(include_str!(
        "../../../crates/klotho-canon/fixtures/hearth_diffs.ron"
    ))
    .unwrap();
    let canon = cook_diffs(&hearth).unwrap();
    let _hearth = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    let ember_doc = ember_slice::ember_doc();
    let _ = cook(&ember_doc).unwrap();
    let _: fn(World) -> CommitKernel = CommitKernel::new;
}

#[test]
fn golden_09_32_dummies_in_seed() {
    let k = boot();
    let _ = pin(&k, "dummy_0");
    let _ = pin(&k, "dummy_31");
    assert_eq!(DUMMY_COUNT, 32);
    for i in 0..DUMMY_COUNT {
        let s = pin(&k, &format!("dummy_{i}"));
        assert_eq!(s.kind(), Some(klotho_core::LocusKind::Actor));
    }
}

#[test]
fn golden_10_infer_fire_is_mind_like_without_aim() {
    let mut k = boot();
    let player = pin(&k, "player");
    let ammo = k.canon().resource_id("ammo").unwrap();
    k.ingest(Proposal::Infer(InferIntent {
        model: ModelId(Name::from("ember-fill")),
        locus: Some(player),
        verb: Verb::Fire,
        target: IntentTarget::Name(Name::from("dummy_0")),
        claimed_facts: Vec::new(),
    }));
    let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
    assert!(d.rejects.is_empty(), "{d:?}");
    assert_eq!(k.world().view().qty(player, ammo), 9);
}
