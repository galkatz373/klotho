//! PR 04b: cook tables, Lockable key-or-rite, eval of cooked lock.use.

use klotho_canon::{
    CookError, CookedLawBody, EvalCtx, MemStore, PRED_OPS_PER_TICK, cook, cook_diffs, eval_pred,
};
use klotho_core::{Hash, LocusKind, RejectReason, Sigil};
use klotho_ir::{
    Affordance, CanonDiff, IntentDoc, Name, Pred, ProvenanceId, Rel, SeedFact, Slot, SourceKind,
    StyleIntent, Verb, from_ron,
};

fn relic(id: u128) -> Sigil {
    Sigil::pack(LocusKind::Relic, 0, id).unwrap()
}

fn actor(id: u128) -> Sigil {
    Sigil::pack(LocusKind::Actor, 0, id).unwrap()
}

#[test]
fn hearth_diffs_cook_and_lockable_has_key_or_rite() {
    let src = include_str!("../fixtures/hearth_diffs.ron");
    let diffs: Vec<CanonDiff> = from_ron(src).unwrap();
    let canon = cook_diffs(&diffs).expect("hearth diffs must cook");
    assert!(canon.affordance_id("Lockable").is_some());
    let lock = canon
        .laws
        .iter()
        .find(|l| l.name.as_str() == "lock.use")
        .expect("lock.use");
    assert!(matches!(lock.body, CookedLawBody::Pred { .. }));
    assert!(canon.rite_id("lockpick").is_some());
    let pick = canon.rites.iter().find(|r| r.name.as_str() == "lockpick");
    let pick = pick.expect("lockpick rite");
    assert!(pick.chunk.instrs.iter().any(|i| i.pc == 99));
    assert!(pick.guards.contains_key(&1));
}

#[test]
fn ash_diffs_cook() {
    let src = include_str!("../fixtures/ash.ron");
    let diffs: Vec<CanonDiff> = from_ron(src).unwrap();
    let canon = cook_diffs(&diffs).expect("ash diffs must cook");
    assert!(canon.affordance_id("Hittable").is_some());
    assert!(canon.law_id("fire.hitscan").is_some());
    assert!(canon.rite_id("respawn").is_some());
}

#[test]
fn ember_diffs_cook() {
    let src = include_str!("../fixtures/ember.ron");
    let diffs: Vec<CanonDiff> = from_ron(src).unwrap();
    let canon = cook_diffs(&diffs).expect("ember diffs must cook");
    assert!(canon.affordance_id("Fragment").is_some());
    assert!(canon.affordance_id("Destructible").is_some());
    assert!(canon.law_id("projectile.cap").is_some());
    assert!(canon.law_id("fragment.cap").is_some());
    assert!(canon.rite_id("melee").is_some());
    assert!(canon.rite_id("collapse").is_some());
    let collapse = canon
        .rites
        .iter()
        .find(|r| r.name.as_str() == "collapse")
        .expect("collapse");
    let spawns = collapse
        .chunk
        .instrs
        .iter()
        .filter(|i| matches!(i.op, klotho_ir::RiteOp::Spawn(_)))
        .count();
    assert_eq!(spawns, 64);
}

#[test]
fn lockable_requires_key_or_rite_pred() {
    let diffs = [CanonDiff::AddAffordance(Affordance {
        id: Name::from("Lockable"),
        requires: vec![Pred::Affordance(Slot::This, Name::from("Opaque"))],
        grants: vec![Name::from("Use")],
        conflicts: vec![],
    })];
    assert_eq!(
        cook_diffs(&diffs).unwrap_err(),
        CookError::LockableNeedsKeyOrRite
    );
}

#[test]
fn cooked_lock_use_evals_key_in_hand() {
    let src = include_str!("../fixtures/hearth_diffs.ron");
    let diffs: Vec<CanonDiff> = from_ron(src).unwrap();
    let canon = cook_diffs(&diffs).unwrap();
    let lock = canon
        .laws
        .iter()
        .find(|l| l.name.as_str() == "lock.use")
        .unwrap();
    let CookedLawBody::Pred { must, .. } = lock.body else {
        panic!("lock.use must be Pred");
    };
    let when = canon.pred(lock.when).unwrap();
    let must = canon.pred(must).unwrap();

    let mut store = MemStore::new();
    let player = actor(1);
    let door = relic(2);
    let key = relic(3);
    let lockable = canon.affordance_id("Lockable").unwrap();
    store.set_affordance(door, lockable, true);
    store.add_rel(door, Rel::KeyedBy, key);
    store.add_rel(key, Rel::WieldedBy, player);

    let claimed = [];
    let pins = [];
    let ctx = EvalCtx {
        store: &store,
        this: player,
        target: Some(door),
        pins: &pins,
        verb: Verb::Use,
        source: SourceKind::Player,
        claimed: &claimed,
        swept_hits_opaque_closed: false,
    };
    let mut tick = PRED_OPS_PER_TICK;
    assert!(eval_pred(when, &ctx, &mut tick).unwrap());
    assert!(eval_pred(must, &ctx, &mut tick).unwrap());
}

#[test]
fn cook_doc_unbound_pin_fails() {
    let diffs: Vec<CanonDiff> = from_ron(
        r#"[
        AddLaw(Law(id: "pride.hammer", when: EqVerb(Carry), body: Pred(
            must: SelfIs(Name("bran")), ought: None)))
    ]"#,
    )
    .unwrap();
    let doc = IntentDoc {
        style: StyleIntent::default(),
        canon_diffs: diffs,
        seed: vec![SeedFact::Locus {
            name: Name::from("player"),
            kind: LocusKind::Actor,
        }],
        minds: vec![],
        provenance: ProvenanceId(Hash::ZERO),
    };
    assert!(matches!(
        cook(&doc),
        Err(CookError::UnboundName(n)) if n == "bran"
    ));
}

#[test]
fn duplicate_law_fails() {
    let diffs: Vec<CanonDiff> = from_ron(
        r#"[
        AddLaw(Law(id: "a", when: EqVerb(Use), body: Pred(must: EqVerb(Use), ought: None))),
        AddLaw(Law(id: "a", when: EqVerb(Use), body: Pred(must: EqVerb(Use), ought: None))),
    ]"#,
    )
    .unwrap();
    assert!(matches!(cook_diffs(&diffs), Err(CookError::DuplicateId(_))));
}

#[test]
fn retract_unknown_fails() {
    let diffs: Vec<CanonDiff> = from_ron(
        r#"[
        RetractLaw(id: "missing", reason: "gone")
    ]"#,
    )
    .unwrap();
    assert!(matches!(
        cook_diffs(&diffs),
        Err(CookError::UnknownRetract(_))
    ));
}

#[test]
fn budget_reject_is_not_fault() {
    assert_eq!(RejectReason::Budget.to_string(), "Budget");
}
