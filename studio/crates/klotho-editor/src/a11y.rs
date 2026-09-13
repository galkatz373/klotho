//! Distaff accessibility review: settings, focus path, capture matrix.

use std::fmt;

use klotho_eval::{
    FocusHost, capture_matrix_gate, focus_journey, human_focus_journey, run_journey,
};
use klotho_input::BindTable;
use klotho_ir::A11yProfile;
use klotho_ui::{pause_menu, prove_menu, settings_menu};

use crate::EditorError;

/// Headless accessibility review. Designers never need to open RON.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct A11yReview {
    /// Profile under review.
    pub profile: A11yProfile,
    /// Pause-menu focusable ids in cycle order.
    pub focus_path: Vec<String>,
    /// Capture-matrix cell count.
    pub matrix_cells: usize,
    /// Matrix passed with zero overflow.
    pub matrix_pass: bool,
    /// Scripted focus journey completed.
    pub scripted_focus: bool,
    /// Recorded human focus journey completed.
    pub human_focus: bool,
}

impl fmt::Display for A11yReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "a11y scale {}%", self.profile.text_scale_milli / 10)?;
        writeln!(f, "focus:")?;
        for id in &self.focus_path {
            writeln!(f, "  {id}")?;
        }
        writeln!(
            f,
            "matrix {} cells {}",
            self.matrix_cells,
            if self.matrix_pass { "pass" } else { "fail" }
        )?;
        Ok(())
    }
}

/// Build the review from the first-title menus and capture matrix.
pub fn review_a11y(profile: A11yProfile) -> Result<A11yReview, EditorError> {
    profile
        .validate()
        .map_err(|e| EditorError::Boot(e.to_string()))?;
    let table = BindTable::hearth();
    let pause = pause_menu("en", &profile);
    prove_menu(
        &pause,
        klotho_ui::aspect_viewport(1_920, 1_080),
        &profile,
        "en",
        &table,
        None,
    )
    .map_err(|e| EditorError::Boot(e.to_string()))?;
    prove_menu(
        &settings_menu("en", &profile),
        klotho_ui::aspect_viewport(1_440, 1_080),
        &profile,
        "en",
        &table,
        None,
    )
    .map_err(|e| EditorError::Boot(e.to_string()))?;
    let host = FocusHost::pause();
    let focus_path = host
        .focused()
        .map(|s| {
            let mut ids = vec![s.to_owned()];
            ids.extend(
                pause
                    .children
                    .iter()
                    .filter(|c| c.focusable && c.enabled)
                    .map(|c| c.id.clone()),
            );
            ids.sort();
            ids.dedup();
            ids
        })
        .unwrap_or_default();
    let mut scripted = FocusHost::pause();
    let scripted_focus = run_journey(
        &mut scripted,
        &focus_journey(),
        klotho_core::Hash::from_bytes([16; 32]),
    )
    .is_ok();
    let mut human = FocusHost::pause();
    let human_focus = run_journey(
        &mut human,
        &human_focus_journey(),
        klotho_core::Hash::from_bytes([16; 32]),
    )
    .is_ok();
    let matrix = capture_matrix_gate().map_err(|e| EditorError::Boot(e.to_string()))?;
    Ok(A11yReview {
        profile,
        focus_path,
        matrix_cells: matrix.cells.len(),
        matrix_pass: matrix.pass(),
        scripted_focus,
        human_focus,
    })
}

/// First-title review used by Distaff tests.
pub fn review_first_title() -> Result<A11yReview, EditorError> {
    review_a11y(A11yProfile::first_title())
}
