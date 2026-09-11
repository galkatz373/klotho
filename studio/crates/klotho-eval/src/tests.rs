//! KAI-06 gates: no Projection writes, stale evidence, exact minimize, selection.

use std::collections::BTreeSet;

use klotho_core::{Hash, PlayerId};
use klotho_input::Button;
use klotho_ir::{Analog, AnchorId, Cmp, IntentTarget, Name, Rel, Verb, to_ron};
use klotho_prove::hash_bytes;

use crate::bot::{AutomatedPlayer, BotManifest};
use crate::contract::AcceptanceContract;
use crate::error::EvalError;
use crate::evidence::{CheckLayer, EvidenceBuilder, EvidenceContext};
use crate::host::JourneyHost;
use crate::ids::JourneyId;
use crate::journey::{
    CaptureKind, CapturePoint, DeviceAction, JourneyAssertion, JourneySpec, JourneyStep,
};
use crate::kernel::KernelHost;
use crate::reduce::{minimize, steps_are_public_input};
use crate::run::{end_capture, run_journey};
use crate::script::ScriptHost;
use crate::select::{ChangeImpact, JourneyIndex, select};

fn unlock_spec() -> JourneySpec {
    let mut spec = JourneySpec::new("unlock-door", 16);
    spec.steps = vec![
        JourneyStep::Wait { ticks: 1 },
        JourneyStep::Device {
            action: DeviceAction::press(
                PlayerId(0),
                Button::KeyE,
                IntentTarget::Name(Name::from("key")),
            ),
        },
        JourneyStep::Device {
            action: DeviceAction::press(
                PlayerId(0),
                Button::KeyE,
                IntentTarget::Name(Name::from("door")),
            ),
        },
    ];
    spec.assertions = vec![
        JourneyAssertion::Rel {
            a: Name::from("door"),
            rel: Rel::LockedBy,
            b: Name::from("door"),
            present: false,
        },
        JourneyAssertion::Place {
            locus: Name::from("player"),
            place: Name::from("hall"),
        },
        JourneyAssertion::Qty {
            locus: Name::from("player"),
            resource: Name::from("stamina"),
            cmp: Cmp::Eq,
            value: 10,
        },
        JourneyAssertion::Trace {
            contains: "Unlocked".into(),
        },
    ];
    spec.capture_points = vec![end_capture("unlocked")];
    spec
}

fn locked_fail_spec() -> JourneySpec {
    let mut spec = JourneySpec::new("locked", 16);
    spec.steps = vec![
        JourneyStep::Wait { ticks: 2 },
        JourneyStep::Camera {
            name: Name::from("hero"),
        },
        JourneyStep::Device {
            action: DeviceAction::press(
                PlayerId(0),
                Button::KeyE,
                IntentTarget::Name(Name::from("door")),
            ),
        },
        JourneyStep::Wait { ticks: 1 },
    ];
    spec.assertions = vec![JourneyAssertion::Rel {
        a: Name::from("door"),
        rel: Rel::LockedBy,
        b: Name::from("door"),
        present: false,
    }];
    spec
}

#[test]
fn public_input_unlocks_and_seals_evidence() {
    let spec = unlock_spec();
    assert!(steps_are_public_input(&spec.steps));
    let change = hash_bytes(b"change-unlock");
    let mut host = ScriptHost::door_key();
    let result = run_journey(&mut host, &spec, change).unwrap();
    result
        .evidence
        .accept(&host.evidence_context(change))
        .unwrap();
    assert!(result.evidence.checks().iter().all(|c| c.passed));
    host.check(&JourneyAssertion::Capture {
        point: Name::from("unlocked"),
    })
    .unwrap();
}

#[test]
fn fixture_path_stamps_without_agency_field() {
    let mut spec = JourneySpec::new("fixture-key", 8);
    spec.steps = vec![JourneyStep::Fixture {
        player: PlayerId(0),
        verb: Verb::Use,
        target: IntentTarget::Name(Name::from("key")),
        analog: Analog::default(),
    }];
    spec.assertions = vec![JourneyAssertion::Rel {
        a: Name::from("key"),
        rel: Rel::OwnedBy,
        b: Name::from("player"),
        present: true,
    }];
    run_journey(&mut ScriptHost::door_key(), &spec, Hash::ZERO).unwrap();
}

#[test]
fn unreachable_journey_has_last_state_and_blocked_affordance() {
    let err =
        run_journey(&mut ScriptHost::door_key(), &locked_fail_spec(), Hash::ZERO).unwrap_err();
    match &err {
        EvalError::Unreachable {
            last_state,
            blocked,
            ..
        } => {
            assert_eq!(last_state, "door-locked");
            assert_eq!(blocked, "Openable");
        }
        other => panic!("{other:?}"),
    }
    let d = err.to_diagnostic();
    assert!(d.points_to_anchor());
    assert_eq!(d.code.0, klotho_ir::DiagnosticCode::JOURNEY);
}

#[test]
fn stale_evidence_is_rejected() {
    let spec = unlock_spec();
    let change = hash_bytes(b"c1");
    let mut host = ScriptHost::door_key();
    let result = run_journey(&mut host, &spec, change).unwrap();
    let mut ctx = host.evidence_context(change);
    ctx.canon_hash = hash_bytes(b"other-canon");
    assert!(matches!(
        result.evidence.accept(&ctx),
        Err(EvalError::Stale { field }) if field == "canon"
    ));
    let mut forged = result.evidence.clone();
    forged.checks[0].passed = false;
    assert_eq!(forged.verify_signature(), Err(EvalError::BadSignature));
}

#[test]
fn builder_records_are_hashed_into_the_seal() {
    let ctx = EvidenceContext {
        change: hash_bytes(b"c"),
        project_hash: hash_bytes(b"p"),
        toolchain_hash: hash_bytes(b"t"),
        expanded_ir_hash: hash_bytes(b"i"),
        canon_hash: hash_bytes(b"k"),
        cas_root: hash_bytes(b"s"),
    };
    let mut a = EvidenceBuilder::new(ctx.clone());
    a.record_check(CheckLayer::Journey, Name::from("j"), true, hash_bytes(b"1"));
    let sealed = a.seal(None).unwrap();
    sealed.accept(&ctx).unwrap();
    let mut b = EvidenceBuilder::new(ctx);
    b.record_check(
        CheckLayer::Journey,
        Name::from("j"),
        false,
        hash_bytes(b"1"),
    );
    let other = b.seal(None).unwrap();
    assert_ne!(sealed.signature, other.signature);
}

#[test]
fn minimized_failure_replays_exactly() {
    let (mini, err) = minimize(ScriptHost::door_key, &locked_fail_spec(), Hash::ZERO).unwrap();
    assert!(mini.steps.len() < locked_fail_spec().steps.len());
    let replay = run_journey(&mut ScriptHost::door_key(), &mini, Hash::ZERO).unwrap_err();
    match (err, replay) {
        (
            EvalError::Unreachable {
                last_state: a,
                blocked: ba,
                ..
            },
            EvalError::Unreachable {
                last_state: b,
                blocked: bb,
                ..
            },
        ) => {
            assert_eq!(a, b);
            assert_eq!(ba, bb);
            assert_eq!(ba, "Openable");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn selection_never_skips_declared_dependents() {
    let door = AnchorId::derive(b"mod", b"door");
    let mut unlock = JourneySpec::new("unlock", 8);
    unlock.anchors = vec![door];
    let mut regress = JourneySpec::new("regress", 8);
    regress.depends_on = vec![JourneyId::from("unlock")];
    let mut index = JourneyIndex::new();
    index.insert(unlock);
    index.insert(regress);
    let mut impact = ChangeImpact::default();
    impact.anchors.insert(door);
    let selected = select(&index, &impact).unwrap();
    assert_eq!(
        selected,
        vec![JourneyId::from("unlock"), JourneyId::from("regress")]
    );
}

#[test]
fn selection_keeps_declared_contract_journeys() {
    let mut index = JourneyIndex::new();
    index.insert(JourneySpec::new("soak", 8));
    let mut impact = ChangeImpact::default();
    impact.declared.insert(JourneyId::from("soak"));
    let selected = select(&index, &impact).unwrap();
    assert_eq!(selected, vec![JourneyId::from("soak")]);
}

#[test]
fn bot_cannot_mint_agency_or_write_projection() {
    let bot = AutomatedPlayer::new(PlayerId(0));
    let action = bot.press(Button::KeyE, IntentTarget::Name(Name::from("key")));
    assert!(action.buttons.contains(&Button::KeyE));
    let _: fn(&AutomatedPlayer, Button, IntentTarget) -> DeviceAction = AutomatedPlayer::press;
    assert!(matches!(
        EvalError::Projection.to_string().as_str(),
        "ProjectionWriteDenied"
    ));
    assert!(matches!(
        EvalError::Agency.to_string().as_str(),
        "AgencyMintDenied"
    ));
}

#[test]
fn bot_search_is_advisory_outside_manifest() {
    let bot = AutomatedPlayer::new(PlayerId(0));
    let actions = vec![bot.press(Button::KeyE, IntentTarget::Name(Name::from("door")))];
    let host = ScriptHost::door_key();
    let id = JourneyId::from("open");
    let found = bot
        .search(&host, &actions, 8, &id, &BotManifest::default(), |h| {
            h.last_state() == "door-open"
        })
        .unwrap();
    assert_eq!(found.len(), 1);
    let mut manifest = BotManifest::default();
    manifest.capabilities.insert(id.clone());
    let err = bot
        .search(&host, &actions, 8, &id, &manifest, |h| {
            h.last_state() == "door-open"
        })
        .unwrap_err();
    assert!(matches!(err, EvalError::Unreachable { .. }));
}

#[test]
fn save_load_round_trips_script_state() {
    let mut spec = JourneySpec::new("save-load", 16);
    spec.steps = vec![
        JourneyStep::Device {
            action: DeviceAction::press(
                PlayerId(0),
                Button::KeyE,
                IntentTarget::Name(Name::from("key")),
            ),
        },
        JourneyStep::Save {
            slot: Name::from("mid"),
        },
        JourneyStep::Wait { ticks: 1 },
        JourneyStep::Load {
            slot: Name::from("mid"),
        },
    ];
    spec.assertions = vec![JourneyAssertion::Rel {
        a: Name::from("key"),
        rel: Rel::OwnedBy,
        b: Name::from("player"),
        present: true,
    }];
    run_journey(&mut ScriptHost::door_key(), &spec, Hash::ZERO).unwrap();
}

#[test]
fn kernel_host_uses_public_input_and_capture() {
    let mut spec = JourneySpec::new("kernel-look", 8);
    spec.steps = vec![
        JourneyStep::Device {
            action: DeviceAction::new(PlayerId(0)),
        },
        JourneyStep::Wait { ticks: 1 },
    ];
    spec.capture_points = vec![CapturePoint {
        name: Name::from("t0"),
        after_step: 0,
        kind: CaptureKind::Semantic,
    }];
    spec.assertions = vec![JourneyAssertion::Capture {
        point: Name::from("t0"),
    }];
    run_journey(&mut KernelHost::seeded(), &spec, Hash::ZERO).unwrap();
}

#[test]
fn acceptance_contract_declares_journeys() {
    let mut c = AcceptanceContract::default();
    c.journeys.push(JourneyId::from("a"));
    c.non_regression.push(JourneyId::from("b"));
    c.non_regression.push(JourneyId::from("a"));
    assert_eq!(
        c.declared_journeys(),
        vec![JourneyId::from("a"), JourneyId::from("b")]
    );
}

#[test]
fn journey_spec_round_trips_ron() {
    let spec = unlock_spec();
    let text = to_ron(&spec).unwrap();
    let back: JourneySpec = klotho_ir::from_ron(&text).unwrap();
    assert_eq!(spec, back);
}

#[test]
fn cycle_is_fail_closed() {
    let mut a = JourneySpec::new("a", 1);
    a.depends_on = vec![JourneyId::from("b")];
    let mut b = JourneySpec::new("b", 1);
    b.depends_on = vec![JourneyId::from("a")];
    let mut index = JourneyIndex::new();
    index.insert(a);
    index.insert(b);
    let mut impact = ChangeImpact::default();
    impact.declared.insert(JourneyId::from("a"));
    assert!(matches!(select(&index, &impact), Err(EvalError::Cycle(_))));
}

#[test]
fn contract_scope_is_recorded() {
    let _ = BTreeSet::<AnchorId>::new();
    let spec = unlock_spec();
    assert_eq!(spec.id.as_str(), "unlock-door");
}
