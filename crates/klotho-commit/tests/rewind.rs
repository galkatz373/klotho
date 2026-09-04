//! Lag-comp rewind goldens. Ring is unhashed; Hit/Qty is Trace.

use std::sync::Arc;

use klotho_canon::cook;
use klotho_commit::{CommitKernel, METRIC_REWIND_TICKS_USED, Proposal};
use klotho_core::{
    AabbMm, BlobId, Budget, Hash, IVec3, LocusKind, Mm, PlayerId, PoseMm, RejectReason, Sigil,
    Tick, YawMd, look_offset,
};
use klotho_ir::{
    Agency, Analog, CanonDiff, IntentDoc, IntentTarget, Name, PlayerIntent, ProvenanceId, SeedFact,
    StyleIntent, Verb, from_ron,
};
use klotho_trace::TraceBody;
use klotho_world::{HITSCAN_RANGE_MM, World};

const FIRE_CANON: &str = r#"[
    AddAffordance(Affordance(id: "Hittable", requires: [], grants: [], conflicts: [])),
    AddAffordance(Affordance(id: "Armed", requires: [], grants: ["Fire"], conflicts: [])),
    AddLaw(Law(
        id: "fire.hitscan",
        when: EqVerb(Fire),
        body: Pred(must: And(Affordance(Self, "Armed"), Qty(Self, "ammo", Ge, 1)), ought: None),
    )),
    AddRite(RiteGraph(id: "fire", cap_steps: 8, cap_ticks: 4, entry: 0, nodes: [
        { pc: 0, op: Bind(Self) },
        { pc: 1, op: Spend("ammo", 1, 4) },
        { pc: 2, op: Emit("Hit") },
        { pc: 3, op: Complete(Success) },
        { pc: 4, op: Complete(Fail) },
    ])),
    AddRite(RiteGraph(id: "apply_hit", cap_steps: 8, cap_ticks: 4, entry: 0, nodes: [
        { pc: 0, op: Spend("health", 25, 2) },
        { pc: 1, op: Complete(Success) },
        { pc: 2, op: Complete(Fail) },
    ])),
]"#;

fn box_xz(hx: i32, hy: i32, hz: i32) -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -hx,
            y: 0,
            z: -hz,
        },
        IVec3 {
            x: hx,
            y: hy,
            z: hz,
        },
    )
}

fn fire_doc() -> IntentDoc {
    let canon_diffs: Vec<CanonDiff> = from_ron(FIRE_CANON).unwrap();
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs,
        seed: vec![
            SeedFact::Locus {
                name: Name::from("player"),
                kind: LocusKind::Actor,
            },
            SeedFact::Locus {
                name: Name::from("dummy_0"),
                kind: LocusKind::Actor,
            },
            SeedFact::Qty {
                of: Name::from("player"),
                res: Name::from("ammo"),
                value: 10,
            },
            SeedFact::Qty {
                of: Name::from("dummy_0"),
                res: Name::from("health"),
                value: 100,
            },
        ],
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

fn plant(k: &mut CommitKernel, player: Sigil, dummy: Sigil, dummy_pose: PoseMm) {
    let armed = k.canon().affordance_id("Armed").unwrap();
    let hittable = k.canon().affordance_id("Hittable").unwrap();
    let ammo = k.canon().resource_id("ammo").unwrap();
    let health = k.canon().resource_id("health").unwrap();
    {
        let mut w = k.world_mut();
        w.insert_locus(player, LocusKind::Actor).unwrap();
        w.insert_locus(dummy, LocusKind::Actor).unwrap();
        w.set_affordance(player, armed, true).unwrap();
        w.set_affordance(dummy, hittable, true).unwrap();
        w.set_qty(player, ammo, 10).unwrap();
        w.set_qty(dummy, health, 100).unwrap();
        w.set_hull(player, box_xz(400, 1800, 400), BlobId::ZERO)
            .unwrap();
        w.set_hull(dummy, box_xz(400, 1800, 400), BlobId::ZERO)
            .unwrap();
        w.set_pose(player, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
            .unwrap();
        w.set_pose(dummy, dummy_pose).unwrap();
    }
}

fn on_ray() -> PoseMm {
    PoseMm::new(Mm(0), Mm(0), Mm(3000), YawMd(0))
}

fn off_ray() -> PoseMm {
    PoseMm::new(Mm(8000), Mm(0), Mm(3000), YawMd(0))
}

fn fire_intent(at: Tick, target: IntentTarget) -> PlayerIntent {
    player_intent(Verb::Fire, at, target)
}

fn player_intent(verb: Verb, at: Tick, target: IntentTarget) -> PlayerIntent {
    PlayerIntent {
        player: PlayerId(0),
        at,
        verb,
        target,
        analog: Analog::default(),
        agency: Agency::none(),
    }
}

fn rewind_not_slo() -> Budget {
    Budget {
        rewind_ticks: 5,
        ..Budget::AAA_SHOOTER
    }
}

fn live_hitscan(k: &CommitKernel, player: Sigil) -> Option<Sigil> {
    let hittable = k.canon().affordance_id("Hittable").unwrap();
    let view = k.world().view();
    let pose = view.pose(player).unwrap();
    view.hitscan(
        pose.translation(),
        look_offset(pose.yaw, pose.pitch, HITSCAN_RANGE_MM),
        player,
        hittable,
    )
}

fn shooter_kernel() -> (CommitKernel, Sigil, Sigil) {
    let canon = cook(&fire_doc()).unwrap();
    let player = canon.pin("player").unwrap();
    let dummy = canon.pin("dummy_0").unwrap();
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    k.bind_player(PlayerId(0), player);
    plant(&mut k, player, dummy, on_ray());
    (k, player, dummy)
}

fn hit_event(events: &[klotho_trace::TraceEvent], dummy: Sigil) -> bool {
    events.iter().any(|e| {
        matches!(
            e.body,
            TraceBody::Emitted {
                a: _,
                b: Some(b),
                ..
            } if b == dummy
        ) || matches!(
            e.body,
            TraceBody::QtyChanged { id, .. } if id == dummy
        )
    })
}

#[test]
fn metric_name_is_stable() {
    assert_eq!(METRIC_REWIND_TICKS_USED, "klotho.rewind.ticks_used");
}

#[test]
fn delayed_fire_hits_strafing_dummy() {
    let (mut k, player, dummy) = shooter_kernel();
    let health = k.canon().resource_id("health").unwrap();
    k.step(Tick(1), Budget::AAA_SHOOTER, &mut []).unwrap();
    let at = k.world().tick();
    assert_eq!(at, Tick(1));
    assert_eq!(live_hitscan(&k, player), Some(dummy));
    k.world_mut().set_pose(dummy, off_ray()).unwrap();
    assert!(live_hitscan(&k, player).is_none(), "live ray must miss");
    k.ingest(Proposal::Player(fire_intent(at, IntentTarget::None)));
    let d = k.step(Tick(5), Budget::AAA_SHOOTER, &mut []).unwrap();
    assert!(d.rejects.is_empty(), "{d:?}");
    assert_eq!(k.world().view().qty(dummy, health), 75);
    assert_eq!(k.world().view().pose(dummy).unwrap(), off_ray());
    assert_eq!(k.last_rewind_ticks_used(), 5);
    assert!(hit_event(&d.events, dummy), "{d:?}");
}

#[test]
fn delayed_fire_name_still_needs_ring_ray() {
    let (mut k, player, dummy) = shooter_kernel();
    let health = k.canon().resource_id("health").unwrap();
    k.world_mut().set_pose(dummy, off_ray()).unwrap();
    k.step(Tick(1), Budget::AAA_SHOOTER, &mut []).unwrap();
    let at = k.world().tick();
    assert!(live_hitscan(&k, player).is_none());
    k.ingest(Proposal::Player(fire_intent(
        at,
        IntentTarget::Name(Name::from("dummy_0")),
    )));
    let ammo = k.canon().resource_id("ammo").unwrap();
    let d = k.step(Tick(3), Budget::AAA_SHOOTER, &mut []).unwrap();
    assert!(d.rejects.is_empty(), "{d:?}");
    assert_eq!(k.world().view().qty(player, ammo), 9);
    assert_eq!(k.world().view().qty(dummy, health), 100);
    assert!(!d.events.iter().any(|e| matches!(
        e.body,
        TraceBody::QtyChanged { id, .. } if id == dummy
    )));
}

#[test]
fn too_old_fire_nacks_stale_epoch() {
    let (mut k, _, dummy) = shooter_kernel();
    let health = k.canon().resource_id("health").unwrap();
    k.step(Tick(1), Budget::AAA_SHOOTER, &mut []).unwrap();
    k.world_mut().set_pose(dummy, off_ray()).unwrap();
    for _ in 0..14 {
        k.step(Tick(1), Budget::AAA_SHOOTER, &mut []).unwrap();
    }
    let now = k.world().tick();
    assert!(now.0 > 12);
    k.ingest(Proposal::Player(fire_intent(
        Tick(0),
        IntentTarget::Sigil(dummy),
    )));
    let d = k.step(Tick(1), Budget::AAA_SHOOTER, &mut []).unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| *r == RejectReason::StaleEpoch),
        "{d:?}"
    );
    assert_eq!(k.world().view().qty(dummy, health), 100);
    assert!(!hit_event(&d.events, dummy), "{d:?}");
}

#[test]
fn rewind_stale_is_not_eval_slo() {
    let budget = rewind_not_slo();
    assert!(budget.rewind_ticks < Budget::HEARTH.eval_slo_ticks);

    let (mut k, player, dummy) = shooter_kernel();
    let health = k.canon().resource_id("health").unwrap();
    let ammo = k.canon().resource_id("ammo").unwrap();
    k.step(Tick(1), budget, &mut []).unwrap();
    let at = k.world().tick();
    k.ingest(Proposal::Player(fire_intent(
        at,
        IntentTarget::Sigil(dummy),
    )));
    let d = k.step(Tick(8), budget, &mut []).unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| *r == RejectReason::StaleEpoch),
        "{d:?}"
    );
    assert_eq!(k.world().view().qty(dummy, health), 100);
    assert_eq!(k.world().view().qty(player, ammo), 10);

    let (mut k, _, _) = shooter_kernel();
    k.step(Tick(1), budget, &mut []).unwrap();
    let at = k.world().tick();
    k.ingest(Proposal::Player(player_intent(
        Verb::Look,
        at,
        IntentTarget::None,
    )));
    let d = k.step(Tick(8), budget, &mut []).unwrap();
    assert!(
        !d.rejects
            .iter()
            .any(|(_, r)| *r == RejectReason::StaleEpoch),
        "{d:?}"
    );

    let (mut k, _, dummy) = shooter_kernel();
    let health = k.canon().resource_id("health").unwrap();
    k.step(Tick(1), budget, &mut []).unwrap();
    let at = k.world().tick();
    k.ingest(Proposal::Player(player_intent(
        Verb::Use,
        at,
        IntentTarget::Sigil(dummy),
    )));
    let d = k.step(Tick(8), budget, &mut []).unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| *r == RejectReason::StaleEpoch),
        "{d:?}"
    );
    assert_eq!(k.world().view().qty(dummy, health), 100);

    let (mut k, _, dummy) = shooter_kernel();
    let health = k.canon().resource_id("health").unwrap();
    let door = Sigil::pack(LocusKind::Relic, 0, 99).unwrap();
    k.world_mut().insert_locus(door, LocusKind::Relic).unwrap();
    k.step(Tick(1), budget, &mut []).unwrap();
    let at = k.world().tick();
    k.ingest(Proposal::Player(player_intent(
        Verb::Use,
        at,
        IntentTarget::Sigil(door),
    )));
    let d = k.step(Tick(8), budget, &mut []).unwrap();
    assert!(
        !d.rejects
            .iter()
            .any(|(_, r)| *r == RejectReason::StaleEpoch),
        "{d:?}"
    );
    assert_eq!(k.world().view().qty(dummy, health), 100);
}

#[test]
fn missing_ring_snap_fail_closed() {
    let (mut k, _, dummy) = shooter_kernel();
    let health = k.canon().resource_id("health").unwrap();
    assert!(k.rewind_ring().is_empty());
    k.ingest(Proposal::Player(fire_intent(
        Tick(0),
        IntentTarget::Sigil(dummy),
    )));
    let d = k.step(Tick(5), Budget::AAA_SHOOTER, &mut []).unwrap();
    assert!(
        d.rejects
            .iter()
            .any(|(_, r)| *r == RejectReason::StaleEpoch),
        "{d:?}"
    );
    assert_eq!(k.world().view().qty(dummy, health), 100);
    assert!(!hit_event(&d.events, dummy), "{d:?}");
}

#[test]
fn hearth_name_fire_hits_without_ring() {
    let canon = cook(&fire_doc()).unwrap();
    let player = canon.pin("player").unwrap();
    let dummy = canon.pin("dummy_0").unwrap();
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    k.bind_player(PlayerId(0), player);
    let armed = k.canon().affordance_id("Armed").unwrap();
    let hittable = k.canon().affordance_id("Hittable").unwrap();
    let ammo = k.canon().resource_id("ammo").unwrap();
    let health = k.canon().resource_id("health").unwrap();
    {
        let mut w = k.world_mut();
        w.insert_locus(player, LocusKind::Actor).unwrap();
        w.insert_locus(dummy, LocusKind::Actor).unwrap();
        w.set_affordance(player, armed, true).unwrap();
        w.set_affordance(dummy, hittable, true).unwrap();
        w.set_qty(player, ammo, 10).unwrap();
        w.set_qty(dummy, health, 100).unwrap();
    }
    k.ingest(Proposal::Player(fire_intent(
        Tick(0),
        IntentTarget::Name(Name::from("dummy_0")),
    )));
    let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
    assert!(d.rejects.is_empty(), "{d:?}");
    assert_eq!(k.world().view().qty(dummy, health), 75);
    assert_eq!(k.world().view().qty(player, ammo), 9);
    assert_eq!(k.last_rewind_ticks_used(), 0);
    assert!(k.rewind_ring().is_empty());
}

#[test]
fn ring_cap_matches_shooter_budget() {
    let (mut k, _, _) = shooter_kernel();
    for _ in 0..20 {
        k.step(Tick(1), Budget::AAA_SHOOTER, &mut []).unwrap();
    }
    assert_eq!(k.rewind_ring().cap(), Budget::AAA_SHOOTER.rewind_ticks);
    assert_eq!(
        k.rewind_ring().len(),
        usize::from(Budget::AAA_SHOOTER.rewind_ticks)
    );
}
