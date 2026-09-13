//! KAI-16: Distaff accessibility review without opening RON.

use klotho_ir::A11yProfile;

use crate::review_first_title;

#[test]
fn designer_reviews_a11y_without_ron() {
    let view = review_first_title().unwrap();
    assert!(view.to_string().contains("a11y scale"));
    assert!(view.focus_path.iter().any(|id| id == "resume"));
    assert!(view.matrix_pass);
    assert!(view.scripted_focus);
    assert!(view.human_focus);
    assert!(view.matrix_cells > 0);
    A11yProfile::first_title().validate().unwrap();
}
