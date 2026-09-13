//! KAI-18 gates: seeded multimodal faults, flake policy, farm SLOs, critic isolation.

use klotho_anim::FOOT_SLIDE_CAP_MM;
use klotho_audio::{AudioStats, MixFrame};
use klotho_cinematic::seeded_hidden_hero;
use klotho_core::{Hash, IVec3};
use klotho_ir::{CameraResponse, Name};
use klotho_prove::hash_bytes;
use klotho_ui::{seeded_focus_fault, seeded_overflow_fault};

use crate::capture::{CapturePolicy, CaptureSet, CaptureView, compare_captures};
use crate::critic::{CriticFinding, QualityVerdict};
use crate::error::EvalError;
use crate::flake::{Attempt, AttemptKind, FlakePolicy, FlakeQuarantine, InfraFault};
use crate::lane::{Coverage, EvalTier, FarmInventory, derive_slo};
use crate::metrics::{MetricPlugin, RgbaFrame, lpips_milli, ssim_milli};
use crate::quality::{
    LPIPS_CEILING_MILLI, SSIM_FLOOR_MILLI, animation_gates, audio_gates, camera_gates,
    loc_a11y_cell, test_frame, visual_gates,
};

fn set(policy: CapturePolicy, rgb: [u8; 3]) -> CaptureSet {
    CaptureSet::new(
        policy,
        vec![CaptureView {
            name: Name::from("hero"),
            frame: RgbaFrame::solid(16, 16, [rgb[0], rgb[1], rgb[2], 255]),
        }],
    )
    .unwrap()
}

#[test]
fn seeded_visual_faults_are_detected() {
    let policy = CapturePolicy::test_lane();
    let reference = set(policy.clone(), [32, 48, 64]);
    let close = set(policy.clone(), [32, 48, 64]);
    let ok = visual_gates(&reference, &close, 0, 0, 0).unwrap();
    assert!(ok.passed());

    let shifted = set(policy, [255, 255, 255]);
    let miss = visual_gates(&reference, &shifted, 0, 0, 0).unwrap();
    assert!(!miss.passed());
    assert!(miss.gates.iter().any(|g| {
        (g.id.as_str().starts_with("ssim.") || g.id.as_str().starts_with("lpips.")) && !g.passed
    }));

    let missing = visual_gates(&reference, &close, 1, 0, 0).unwrap();
    assert!(!missing.passed());
}

#[test]
fn unpinned_metric_plugin_cannot_score() {
    let mut plugin = MetricPlugin::ssim_v1();
    plugin.lock = Hash::ZERO;
    let a = test_frame([8, 8, 8]);
    let b = test_frame([8, 8, 8]);
    assert!(matches!(
        ssim_milli(&plugin, &a, &b),
        Err(EvalError::Capture(_))
    ));
    let mut lpips = MetricPlugin::lpips_v1();
    lpips.id = Name::from("critic-lpips");
    assert!(lpips_milli(&lpips, &a, &b).is_err());
}

#[test]
fn cross_backend_captures_are_not_compared() {
    let metal = CapturePolicy::test_lane();
    let mut vulkan = metal.clone();
    vulkan.backend = Name::from("vulkan");
    let a = set(metal, [10, 10, 10]);
    let b = set(vulkan, [10, 10, 10]);
    assert!(compare_captures(&a, &b).is_err());
}

#[test]
fn identical_frames_meet_ssim_floor() {
    let plugin = MetricPlugin::ssim_v1();
    let a = test_frame([40, 80, 120]);
    let b = test_frame([40, 80, 120]);
    assert!(ssim_milli(&plugin, &a, &b).unwrap() >= SSIM_FLOOR_MILLI);
    assert!(lpips_milli(&MetricPlugin::lpips_v1(), &a, &b).unwrap() <= LPIPS_CEILING_MILLI);
}

#[test]
fn seeded_animation_camera_audio_loc_faults_are_detected() {
    let anim_ok = animation_gates(
        true,
        IVec3::ZERO,
        IVec3 { x: 4, y: 0, z: 0 },
        IVec3::ZERO,
        IVec3 { x: 0, y: 0, z: 20 },
        6,
        6,
    );
    assert!(anim_ok.passed());
    let slide = animation_gates(
        true,
        IVec3::ZERO,
        IVec3 {
            x: FOOT_SLIDE_CAP_MM + 20,
            y: 0,
            z: 0,
        },
        IVec3::ZERO,
        IVec3 { x: 0, y: 0, z: 20 },
        6,
        7,
    );
    assert!(!slide.passed());

    let hidden = camera_gates(seeded_hidden_hero(), CameraResponse::first_title(), false);
    assert!(!hidden.passed());
    assert!(
        hidden
            .gates
            .iter()
            .any(|g| g.id.as_str() == "hero_visible" && !g.passed)
    );

    let audio_ok = audio_gates(
        &AudioStats::measure(
            &MixFrame {
                pcm: vec![800, -800],
                voices: 1,
            },
            0,
            0,
        ),
        32,
    );
    assert!(audio_ok.passed());
    let missing = audio_gates(
        &AudioStats::measure(
            &MixFrame {
                pcm: vec![800, -800],
                voices: 1,
            },
            1,
            0,
        ),
        32,
    );
    assert!(!missing.passed());

    assert!(!loc_a11y_cell(&seeded_overflow_fault()).passed());
    assert!(!loc_a11y_cell(&seeded_focus_fault()).passed());
}

#[test]
fn known_infrastructure_flakes_cannot_retry_a_comparison_failure() {
    let policy = FlakePolicy::kai_farm_a();
    let comparison = [Attempt {
        kind: AttemptKind::Comparison,
        index: 0,
    }];
    assert!(policy.may_retry(&comparison, 1).is_err());
    assert!(
        policy
            .relabel(
                &AttemptKind::Comparison,
                &AttemptKind::Infrastructure(InfraFault::GpuReset)
            )
            .is_err()
    );

    let infra = [Attempt {
        kind: AttemptKind::Infrastructure(InfraFault::GpuReset),
        index: 0,
    }];
    policy.may_retry(&infra, 1).unwrap();

    let exhausted = [
        Attempt {
            kind: AttemptKind::Infrastructure(InfraFault::WorkerTimeout),
            index: 0,
        },
        Attempt {
            kind: AttemptKind::Infrastructure(InfraFault::WorkerTimeout),
            index: 1,
        },
        Attempt {
            kind: AttemptKind::Infrastructure(InfraFault::WorkerTimeout),
            index: 2,
        },
    ];
    assert!(policy.may_retry(&exhausted, 1).is_err());
    assert!(FlakeQuarantine::admit("package", 10, 1, 20).is_err());
    FlakeQuarantine::admit("present.ssim", 10, 1, 20).unwrap();
}

#[test]
fn adding_locales_raises_slo_and_cannot_drop_coverage() {
    let farm = FarmInventory::KAI_FARM_A;
    let required = Coverage::e4_first_title();
    let base = derive_slo(&farm, EvalTier::E4, &required, 250);
    let mut more = required.clone();
    more.locales += 1;
    let grown = derive_slo(&farm, EvalTier::E4, &more, 250);
    assert!(grown.p50_ms > base.p50_ms);
    assert!(grown.cells > base.cells);

    let mut reduced = required.clone();
    reduced.locales = required.locales.saturating_sub(1);
    assert!(reduced.contains(&required).is_err());
    required.contains(&required).unwrap();
}

#[test]
fn critic_cannot_change_pass_fail_or_approval() {
    let mut verdict = QualityVerdict::default();
    verdict.gates.push(crate::critic::GateResult {
        id: Name::from("ssim.hero"),
        passed: false,
        used: 100,
        cap: SSIM_FLOOR_MILLI,
    });
    assert!(!verdict.passed());
    verdict.advise(CriticFinding {
        capture: hash_bytes(b"hero"),
        rule: Name::from("style.silhouette"),
        score_milli: 1_000,
        note: "looks great".into(),
    });
    assert!(!verdict.passed());
    assert!(verdict.approval.is_none());
    verdict.approve(Name::from("art_owner"));
    assert_eq!(
        verdict.approval.as_ref().map(Name::as_str),
        Some("art_owner")
    );
    assert!(!verdict.passed());
}
