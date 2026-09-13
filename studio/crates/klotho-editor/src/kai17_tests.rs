//! KAI-17: Distaff presentation review without opening RON.

use klotho_compile::{PresentProfile, QualityTier};
use klotho_ir::{CastingConsent, MaterialGraph};

use crate::review_presentation;

#[test]
fn designer_reviews_presentation_without_ron() {
    let view = review_presentation().unwrap();
    assert!(view.to_string().contains("present"));
    assert!(view.to_string().contains("1536"));
    assert_eq!(view.us_present, 11_000);
    assert_eq!(view.vram_mb, 1_536);
    assert_eq!(view.stress_tier, QualityTier::High);
    assert_eq!(view.material_bits, 0);
    assert!(!view.lock_hash.chars().all(|c| c == '0'));
    PresentProfile::high().validate().unwrap();
    MaterialGraph::organic().validate().unwrap();
    CastingConsent::first_title().validate().unwrap();
}
