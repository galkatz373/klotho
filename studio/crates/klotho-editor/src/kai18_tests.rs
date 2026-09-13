//! KAI-18: Distaff quality review without opening RON.

use klotho_eval::{CapturePolicy, CriticFinding, GateResult, QualityVerdict, SSIM_FLOOR_MILLI};
use klotho_ir::Name;
use klotho_prove::hash_bytes;

use crate::review_quality;

#[test]
fn designer_reviews_quality_without_ron() {
    let mut verdict = QualityVerdict::default();
    verdict.gates.push(GateResult {
        id: Name::from("ssim.hero"),
        passed: true,
        used: 1_000,
        cap: SSIM_FLOOR_MILLI,
    });
    verdict.advise(CriticFinding {
        capture: hash_bytes(b"hero"),
        rule: Name::from("style.silhouette"),
        score_milli: 0,
        note: "critic cannot fail this".into(),
    });
    let view = review_quality(&verdict).unwrap();
    assert!(view.to_string().contains("quality"));
    assert!(view.to_string().contains("ssim"));
    assert!(view.passed);
    assert_eq!(view.infra_retries, 2);
    assert!(view.e4_cells > 0);
    CapturePolicy::first_title().metrics.verify().unwrap();
}

#[test]
fn critic_score_cannot_green_a_failed_gate() {
    let mut verdict = QualityVerdict::default();
    verdict.gates.push(GateResult {
        id: Name::from("overflow.0"),
        passed: false,
        used: 1,
        cap: 0,
    });
    verdict.advise(CriticFinding {
        capture: hash_bytes(b"ui"),
        rule: Name::from("style.layout"),
        score_milli: 1_000,
        note: "looks fine".into(),
    });
    let view = review_quality(&verdict).unwrap();
    assert!(!view.passed);
}
