//! KAI-10 feel gates: latency lane, artifact classification, Spindle owner Pin.

use std::fs;

use klotho_anim::{Clip, ClipSet, EvidenceLane, classify_clip_change};
use klotho_input::{DeviceLane, LatencySample};
use klotho_ir::{FeelContract, Name, QuantizedCurve, Verb};

use crate::feel::evidence_copy;
use crate::{FeelCandidate, FeelSweep};

fn passing_sample(i: u64) -> LatencySample {
    LatencySample {
        sample_us: i * 1_000,
        intent_us: i * 1_000 + 500,
        present_us: i * 1_000 + 8_000,
        admit_us: i * 1_000 + 20_000,
    }
}

fn fill_pass(sweep: &mut FeelSweep, id: &Name) {
    for i in 0..20 {
        sweep.record_latency(id, passing_sample(i)).unwrap();
    }
}

#[test]
fn spindle_suite_meets_first_title_wired_lane() {
    let mut sweep = FeelSweep::spindle_action_suite().unwrap();
    let id = Name::from("spindle-use");
    fill_pass(&mut sweep, &id);
    sweep.gate_latency().unwrap();
    assert_eq!(sweep.lane, DeviceLane::FIRST_TITLE_WIRED);
    assert_eq!(sweep.lane.present_p95_us, 25_000);
    assert_eq!(sweep.lane.authority_p95_us(), 41_333);
}

#[test]
fn late_presentation_fails_the_lane() {
    let mut sweep = FeelSweep::spindle_action_suite().unwrap();
    let id = Name::from("spindle-use");
    sweep
        .record_latency(
            &id,
            LatencySample {
                sample_us: 0,
                intent_us: 1_000,
                present_us: 40_000,
                admit_us: 10_000,
            },
        )
        .unwrap();
    assert!(sweep.gate_latency().is_err());
}

#[test]
fn clip_classification_selects_visual_or_semantic_copy() {
    let use_clip = ClipSet::hearth().clips[2].clone();
    let mut joints = use_clip.clone();
    joints.joints = vec![vec![klotho_core::PoseMm::default()]];
    assert_eq!(
        classify_clip_change(Some(&use_clip), &joints, false, false),
        EvidenceLane::VisualOnly
    );
    assert!(evidence_copy(EvidenceLane::VisualOnly).starts_with("visual-only"));

    let walk = ClipSet::hearth().clips[1].clone();
    let mut root = walk.clone();
    root.samples[0].z += 1;
    assert_eq!(
        classify_clip_change(Some(&walk), &root, false, false),
        EvidenceLane::SemanticJourneys
    );
    assert!(evidence_copy(EvidenceLane::SemanticJourneys).starts_with("semantic"));

    assert_eq!(
        classify_clip_change(
            Some(&Clip::tpose(0, Verb::Look)),
            &Clip::tpose(0, Verb::Look),
            true,
            false
        ),
        EvidenceLane::SemanticJourneys
    );
}

#[test]
fn feel_owner_approves_spindle_action_suite() {
    let mut sweep = FeelSweep::spindle_action_suite().unwrap();
    let id = Name::from("spindle-use");
    fill_pass(&mut sweep, &id);
    sweep.record_play(&id, 1, 4, 0, 0).unwrap();
    sweep.rate(&id, 5).unwrap();
    let owner = Name::from("feel-owner");
    sweep.set_owner(owner.clone());
    let pin = sweep.approve(&id, owner).unwrap();
    assert_eq!(pin.by.as_str(), "feel-owner");
    assert_eq!(pin.of.as_str(), "spindle-use");
    assert_eq!(
        sweep
            .approved()
            .unwrap()
            .contract()
            .impact
            .recovery_wait_ticks,
        4
    );
}

#[test]
fn model_cannot_silently_retune_or_approve() {
    let mut sweep = FeelSweep::spindle_action_suite().unwrap();
    let id = Name::from("spindle-use");
    fill_pass(&mut sweep, &id);
    let mut retuned = FeelContract::spindle_use();
    retuned.input_buffer_ticks = 8;
    assert!(sweep.retune_forbidden(&id, &retuned));
    assert_eq!(
        sweep
            .approve(&id, Name::from("optimizer"))
            .unwrap_err()
            .to_string(),
        "feel approval requires the named combat/camera/design owner"
    );
}

#[test]
fn ab_play_is_over_immutable_distinct_branches() {
    let mut sweep = FeelSweep::new(Name::from("use"), DeviceLane::FIRST_TITLE_WIRED);
    let a = FeelContract::spindle_use();
    let mut b = FeelContract::spindle_use();
    b.accel_curve = QuantizedCurve {
        knots: vec![
            klotho_ir::CurveKnot { x: 0, y: 0 },
            klotho_ir::CurveKnot {
                x: 1000,
                y: klotho_core::VelFx::ONE.0 / 2,
            },
        ],
    };
    sweep
        .push(FeelCandidate::freeze(Name::from("a"), a).unwrap())
        .unwrap();
    sweep
        .push(FeelCandidate::freeze(Name::from("b"), b).unwrap())
        .unwrap();
    let session = sweep.ab_play(&Name::from("a"), &Name::from("b")).unwrap();
    assert_ne!(session.left.branch, session.right.branch);
    assert_eq!(session.left.contract().action.as_str(), "use");
}

#[test]
fn hit_stop_is_not_recovery_wait() {
    let c = FeelContract::spindle_use();
    assert_eq!(c.impact.hit_stop_present_ticks, 2);
    assert_eq!(c.impact.recovery_wait_ticks, 4);
    assert_ne!(
        c.impact.hit_stop_present_ticks,
        c.impact.recovery_wait_ticks
    );
}

#[test]
fn spindle_feel_fixture_is_the_typed_suite() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/spindle-slice/feel.ron");
    let text = fs::read_to_string(path).unwrap();
    let loaded: FeelContract = klotho_ir::from_ron(&text).unwrap();
    loaded.validate().unwrap();
    assert_eq!(loaded, FeelContract::spindle_use());
}
