//! KAI-04 seeded diagnostic corpus. Gate: every required class is produced
//! from a real validator (or the KAI-04 journey/budget envelope), and ≥ 90%
//! of those diagnostics point at a semantic anchor.

use klotho_canon::{CookError, PRED_OPS_PER_EVAL, check_rite_cfg, compile_pred, cook_diffs};
use klotho_core::{PlayerId, Tick};
use klotho_ir::{
    Agency, Analog, AssistLevel, CanonDiff, Channel, Diagnostic, DiagnosticCode, FailureClass,
    IntentTarget, Law, LawBody, Name, PlayerIntent, Pred, RiteGraph, RiteNode, RiteOp, Slot,
    Status, Verb, diagnose_budget, diagnose_journey, diagnose_quality,
};
use klotho_prove::{ArtifactKind, ProveError};

use crate::error::{CompileError, check_ship_allowlist};
use crate::header::{MAX_RITE_STEPS, validate_rite, write_prefix};

fn law(id: &str, when: Pred, must: Pred) -> CanonDiff {
    CanonDiff::AddLaw(Law {
        id: Name::from(id),
        when,
        body: LawBody::Pred { must, ought: None },
    })
}

fn contradiction() -> Diagnostic {
    let diffs = [
        law("alive", Pred::EqVerb(Verb::Use), Pred::EqVerb(Verb::Use)),
        law(
            "dead",
            Pred::EqVerb(Verb::Use),
            Pred::Not(Box::new(Pred::EqVerb(Verb::Use))),
        ),
    ];
    let err = cook_diffs(&diffs).expect_err("pairwise must conflict");
    assert!(matches!(err, CookError::Contradiction(_)), "{err}");
    err.to_diagnostic()
}

fn cfg_target() -> Diagnostic {
    let graph = RiteGraph {
        id: Name::from("broken"),
        cap_steps: 8,
        cap_ticks: 8,
        entry: 0,
        nodes: vec![
            RiteNode::Op(RiteOp::Branch(Pred::SelfIs(Slot::This), 1, 99)),
            RiteNode::Op(RiteOp::Halt(Status::Success)),
        ],
    };
    let err = check_rite_cfg(&graph).expect_err("missing branch target");
    assert!(
        matches!(err, CookError::MissingTarget { from: 0, to: 99 }),
        "{err}"
    );
    err.to_diagnostic()
}

fn cap() -> Diagnostic {
    let mut b = Vec::new();
    write_prefix(&mut b, ArtifactKind::RiteChunk);
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&(MAX_RITE_STEPS.saturating_add(1)).to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    let err = validate_rite(&b).expect_err("rite cap");
    assert!(
        matches!(err, CompileError::Header(ref s) if s.contains("cap_steps")),
        "{err}"
    );
    err.to_diagnostic()
}

fn agency() -> Diagnostic {
    let intent = PlayerIntent {
        player: PlayerId(0),
        at: Tick(1),
        verb: Verb::Use,
        target: IntentTarget::None,
        analog: Analog::default(),
        agency: Agency {
            claimed: vec![Channel::Timing, Channel::Timing],
            assist: AssistLevel::None,
        },
    };
    let err = intent.validate().expect_err("duplicate channel");
    err.to_diagnostic()
}

fn provenance() -> Diagnostic {
    CompileError::prove(ProveError::UnknownLicense).to_diagnostic()
}

fn package() -> Diagnostic {
    let err = check_ship_allowlist("models/kai-benchmark.lock").expect_err("allowlist");
    err.to_diagnostic()
}

fn journey() -> Diagnostic {
    diagnose_journey(
        "open-door",
        "has-brass-key",
        "Openable",
        "Journey(has-brass-key->Openable)",
    )
}

fn budget() -> Diagnostic {
    diagnose_budget(
        "aaa_adventure",
        &["hearth-fire"],
        9_000,
        8_000,
        "Budget(9000>8000)",
    )
}

fn quality() -> Diagnostic {
    diagnose_quality("ssim", 720, 900, &["hero.albedo"], "Quality(ssim 720<900)")
}

fn seeded() -> Vec<Diagnostic> {
    vec![
        contradiction(),
        cfg_target(),
        cap(),
        agency(),
        provenance(),
        package(),
        journey(),
        budget(),
        quality(),
    ]
}

#[test]
fn seeded_corpus_covers_required_classes_and_anchors() {
    let diags = seeded();
    let mut seen = Vec::new();
    for d in &diags {
        seen.push(d.class);
        assert_eq!(
            d.to_string(),
            d.message,
            "display must stay the concise line"
        );
        assert!(!d.legal_repairs.is_empty(), "{} has no repair", d.code);
        assert!(d.witness.is_some(), "{} has no witness", d.code);
        assert!(d.cost.is_some(), "{} has no cost", d.code);
    }
    for class in [
        FailureClass::Contradiction,
        FailureClass::Cfg,
        FailureClass::Cap,
        FailureClass::Agency,
        FailureClass::Provenance,
        FailureClass::Package,
        FailureClass::Journey,
        FailureClass::Budget,
        FailureClass::Quality,
    ] {
        assert!(seen.contains(&class), "missing class {class:?}");
    }
    assert_eq!(diags[0].code.0, DiagnosticCode::CONTRADICTION);
    assert_eq!(diags[1].code.0, DiagnosticCode::CFG_TARGET);
    assert_eq!(diags[2].code.0, DiagnosticCode::CAP);
    assert_eq!(diags[3].code.0, DiagnosticCode::AGENCY);
    assert_eq!(diags[4].code.0, DiagnosticCode::PROVENANCE);
    assert_eq!(diags[5].code.0, DiagnosticCode::PACKAGE);
    assert_eq!(diags[6].code.0, DiagnosticCode::JOURNEY);
    assert_eq!(diags[7].code.0, DiagnosticCode::BUDGET);
    assert_eq!(diags[8].code.0, DiagnosticCode::QUALITY);

    let (hit, total) = Diagnostic::anchor_ratio(&diags);
    assert!(
        total > 0 && hit * 100 / total >= 90,
        "anchor ratio {hit}/{total} below 90%"
    );
    assert_eq!(hit, total, "every seeded class should blame an anchor");
}

#[test]
fn pred_too_large_is_also_a_cap() {
    let mut p = Pred::EqVerb(Verb::Use);
    for _ in 0..PRED_OPS_PER_EVAL {
        p = Pred::And(Box::new(p), Box::new(Pred::EqVerb(Verb::Use)));
    }
    let err = compile_pred(&p).expect_err("pred cap");
    let d = err.to_diagnostic();
    assert_eq!(d.code.0, DiagnosticCode::CAP);
    assert!(d.points_to_anchor());
}

#[test]
fn ship_allowlist_accepts_runtime_paths() {
    check_ship_allowlist("engine/crates/klotho-runtime/src/lib.rs").unwrap();
    check_ship_allowlist("data/kitbash/hearth.ron").unwrap();
}

#[test]
fn ship_allowlist_rejects_studio_and_weight_paths() {
    // K70: the studio tree (authoring providers, indexes, transcripts) and
    // model weights never ship. No studio crate is named (K83 firewall).
    check_ship_allowlist("studio/crates/klotho-editor/src/lib.rs").expect_err("studio tree");
    check_ship_allowlist("data/weights/critic.gguf").expect_err("weights");
}
