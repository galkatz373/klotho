//! Locale × aspect × input × accessibility capture matrix (KAI-16).

use klotho_input::{BindTable, InputFamily};
use klotho_ir::A11yProfile;

use crate::error::UiError;
use crate::focus::FocusCycle;
use crate::layout::{CAPTURE_ASPECTS, CAPTURE_LOCALES, aspect_viewport, layout};
use crate::menu::{pause_menu, remap_menu, settings_menu};

/// One cell of the supported capture matrix.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CaptureCell {
    /// Locale id.
    pub locale: String,
    /// Surface width.
    pub width: u32,
    /// Surface height.
    pub height: u32,
    /// Input family.
    pub input: InputFamily,
    /// Profile label.
    pub profile: String,
    /// Overflowing nodes.
    pub overflow: u32,
    /// Overlapping pairs.
    pub overlap: u32,
    /// Focus cycle visited every control.
    pub focus_complete: bool,
}

/// Aggregated matrix result.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CaptureReport {
    /// Every locale × aspect × input × profile cell.
    pub cells: Vec<CaptureCell>,
}

impl CaptureReport {
    /// True when every cell has zero overflow/overlap and a complete focus path.
    #[must_use]
    pub fn pass(&self) -> bool {
        self.cells
            .iter()
            .all(|c| c.overflow == 0 && c.overlap == 0 && c.focus_complete)
    }

    /// First failing cell, if any.
    #[must_use]
    pub fn first_failure(&self) -> Option<&CaptureCell> {
        self.cells
            .iter()
            .find(|c| c.overflow > 0 || c.overlap > 0 || !c.focus_complete)
    }
}

/// Profiles exercised by the matrix. Default plus the four first-title variants.
#[must_use]
pub fn capture_profiles() -> Vec<(&'static str, A11yProfile)> {
    let mut large = A11yProfile::first_title();
    large.text_scale_milli = A11yProfile::MAX_SCALE;
    let mut contrast = A11yProfile::first_title();
    contrast.contrast = klotho_ir::ContrastMode::High;
    let mut motion = A11yProfile::first_title();
    motion.reduce_motion = true;
    motion.reduce_shake = true;
    let mut captions = A11yProfile::first_title();
    captions.closed_captions = true;
    captions.screen_reader = true;
    vec![
        ("default", A11yProfile::first_title()),
        ("large_text", large),
        ("high_contrast", contrast),
        ("reduced_motion", motion),
        ("captions_reader", captions),
    ]
}

/// Run the supported matrix over pause, settings, and remap menus.
#[must_use]
pub fn run_capture_matrix(table: &BindTable) -> CaptureReport {
    let mut cells = Vec::new();
    for locale in CAPTURE_LOCALES {
        for &(width, height) in CAPTURE_ASPECTS {
            for input in [InputFamily::KeyboardMouse, InputFamily::Gamepad] {
                for (label, profile) in capture_profiles() {
                    cells.push(capture_cell(
                        locale, width, height, input, label, profile, table,
                    ));
                }
            }
        }
    }
    CaptureReport { cells }
}

fn capture_cell(
    locale: &str,
    width: u32,
    height: u32,
    input: InputFamily,
    label: &str,
    profile: A11yProfile,
    table: &BindTable,
) -> CaptureCell {
    let viewport = aspect_viewport(width, height);
    let menus = [
        pause_menu(locale, &profile),
        settings_menu(locale, &profile),
        remap_menu(locale, input, table, &profile),
    ];
    let mut overflow = 0u32;
    let mut overlap = 0u32;
    let mut focus_complete = true;
    for menu in &menus {
        let frame = layout(menu, viewport, &profile, locale);
        overflow = overflow.saturating_add(frame.overflows().len() as u32);
        overlap = overlap.saturating_add(frame.overlaps().len() as u32);
        let mut cycle = FocusCycle::from_tree(menu);
        if cycle.complete_cycle().is_err() {
            focus_complete = false;
        }
    }
    CaptureCell {
        locale: locale.to_owned(),
        width,
        height,
        input,
        profile: label.to_owned(),
        overflow,
        overlap,
        focus_complete,
    }
}

/// Hard gate used by eval / Distaff.
pub fn matrix_gate(table: &BindTable) -> Result<CaptureReport, UiError> {
    let report = run_capture_matrix(table);
    if let Some(cell) = report.first_failure() {
        return Err(UiError::Overflow {
            node: format!(
                "{}x{} {} {} {}",
                cell.width,
                cell.height,
                cell.locale,
                cell.input.as_str(),
                cell.profile
            ),
            locale: cell.locale.clone(),
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_matrix_has_zero_overflow() {
        let report = matrix_gate(&BindTable::hearth()).unwrap();
        assert_eq!(
            report.cells.len(),
            CAPTURE_LOCALES.len() * CAPTURE_ASPECTS.len() * 2 * capture_profiles().len()
        );
        assert!(report.pass());
    }
}
