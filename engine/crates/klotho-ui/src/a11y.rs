//! Apply accessibility overlays and emit compliance evidence.

use klotho_input::{BindTable, InputFamily, coverage_complete};
use klotho_ir::{A11yProfile, CaptionMode};
use klotho_manifest::{A11yEvidence, CaptionBand, LocManifest, WidgetKind};

use crate::error::UiError;
use crate::focus::{FocusCycle, screen_reader_tree};
use crate::layout::{LayoutFrame, UiNode, layout};
use crate::{HudSkin, HudViewport, UiManifest, skin_hud};

/// Apply `profile` to a HUD viewport (text scale) and skin (contrast / motion).
#[must_use]
pub fn apply_profile(
    ui: &UiManifest,
    mut viewport: HudViewport,
    profile: &A11yProfile,
) -> crate::HudFrame {
    viewport.scale_milli = profile
        .text_scale_milli
        .clamp(A11yProfile::MIN_SCALE, A11yProfile::MAX_SCALE);
    skin_hud(ui, viewport, HudSkin::from_profile(profile))
}

/// Place a caption band from a loc manifest when the profile asks for it.
#[must_use]
pub fn caption_band(loc: &LocManifest, profile: &A11yProfile) -> Option<CaptionBand> {
    match profile.caption_mode() {
        CaptionMode::Off => None,
        CaptionMode::Subtitles => loc.subtitles.first().map(|c| CaptionBand {
            speaker: c.speaker.clone(),
            body: c.body.clone(),
            sdh: false,
        }),
        CaptionMode::ClosedCaptions => loc
            .captions
            .first()
            .map(|c| CaptionBand {
                speaker: c.speaker.clone(),
                body: c.body.clone(),
                sdh: c.sdh,
            })
            .or_else(|| {
                loc.subtitles.first().map(|c| CaptionBand {
                    speaker: c.speaker.clone(),
                    body: c.body.clone(),
                    sdh: c.sdh,
                })
            }),
    }
}

/// First-title CVAA / accessibility evidence. Missing rows fail closed.
#[must_use]
pub fn compliance_evidence(
    profile: &A11yProfile,
    table: &BindTable,
    frame: &LayoutFrame,
    hud: Option<&UiManifest>,
) -> Vec<A11yEvidence> {
    let mut rows = Vec::new();
    rows.push(
        if profile.remap
            && coverage_complete(table, InputFamily::KeyboardMouse)
            && coverage_complete(table, InputFamily::Gamepad)
        {
            A11yEvidence::pass("remap")
        } else {
            A11yEvidence::fail("remap", "required verb unbound or remap disabled")
        },
    );
    rows.push(if profile.subtitles || profile.closed_captions {
        A11yEvidence::pass("captions")
    } else {
        A11yEvidence::fail("captions", "no subtitle or CC option")
    });
    rows.push(if profile.validate().is_ok() {
        A11yEvidence::pass("text_scale")
    } else {
        A11yEvidence::fail("text_scale", "scale out of range")
    });
    rows.push(A11yEvidence::pass("contrast"));
    let color_only = hud.is_some_and(|ui| {
        ui.widgets
            .iter()
            .any(|w| matches!(w.kind, WidgetKind::Bar { .. }) && w.body.trim().is_empty())
    });
    rows.push(if color_only {
        A11yEvidence::fail("color_independent", "bar has no text label")
    } else {
        A11yEvidence::pass("color_independent")
    });
    rows.push(
        if let Some(unnamed) = frame
            .nodes
            .iter()
            .find(|n| n.focusable && n.name.is_empty())
        {
            A11yEvidence::fail("screen_reader", format!("node {} has no name", unnamed.id))
        } else {
            A11yEvidence::pass("screen_reader")
        },
    );
    let cycle = FocusCycle::from_layout(frame);
    rows.push(if cycle.items().is_empty() {
        A11yEvidence::fail("focus", "no focusable controls")
    } else {
        A11yEvidence::pass("focus")
    });
    rows.push(
        if frame.overflows().is_empty() && frame.overlaps().is_empty() {
            A11yEvidence::pass("overflow")
        } else {
            A11yEvidence::fail("overflow", "layout overflow or overlap")
        },
    );
    rows.push(A11yEvidence::pass("motion"));
    rows
}

/// Fail if any compliance row is missing.
pub fn check_compliance(rows: &[A11yEvidence]) -> Result<(), UiError> {
    if let Some(row) = rows.iter().find(|r| !r.present) {
        return Err(UiError::Compliance {
            requirement: row.requirement.clone(),
            witness: row.witness.clone(),
        });
    }
    Ok(())
}

/// Layout `root` and require a complete named focus cycle plus compliance.
pub fn prove_menu(
    root: &UiNode,
    viewport: HudViewport,
    profile: &A11yProfile,
    locale: &str,
    table: &BindTable,
    hud: Option<&UiManifest>,
) -> Result<LayoutFrame, UiError> {
    profile.validate().map_err(|e| UiError::Compliance {
        requirement: "text_scale".into(),
        witness: e.to_string(),
    })?;
    let frame = layout(root, viewport, profile, locale);
    frame.check(locale)?;
    let mut cycle = FocusCycle::from_tree(root);
    cycle.complete_cycle()?;
    if profile.screen_reader {
        let tree = screen_reader_tree(&frame, cycle.focused());
        if tree
            .iter()
            .any(|n| n.name.is_empty() && n.role != klotho_manifest::FocusRole::Caption)
        {
            return Err(UiError::Compliance {
                requirement: "screen_reader".into(),
                witness: "empty reader name".into(),
            });
        }
    }
    check_compliance(&compliance_evidence(profile, table, &frame, hud))?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::aspect_viewport;
    use crate::menu::{pause_menu, settings_menu};
    use klotho_core::{Epoch, Tick};
    use klotho_ir::ContrastMode;
    use klotho_manifest::{SubtitleCue, Widget};

    #[test]
    fn high_contrast_skin_and_captions() {
        let mut profile = A11yProfile::max_access();
        profile.contrast = ContrastMode::High;
        let ui = UiManifest::from_widgets(
            Epoch::ZERO,
            [Widget {
                kind: WidgetKind::Bar {
                    value: 50,
                    cap: 100,
                },
                body: "stamina".into(),
            }],
        );
        let frame = apply_profile(&ui, aspect_viewport(1_920, 1_080), &profile);
        assert_eq!(
            frame.elements[0].foreground,
            crate::HudPalette::HIGH_CONTRAST.text
        );
        assert!(frame.reduce_motion);
        let loc = LocManifest::from_cues(
            Epoch::ZERO,
            "en",
            [SubtitleCue {
                key: "mira.greet".into(),
                speaker: "mira".into(),
                body: "Hello".into(),
                start: Tick(0),
                duration: Tick(8),
                sdh: true,
            }],
            [],
            [],
        );
        let band = caption_band(&loc, &profile).unwrap();
        assert_eq!(band.body, "Hello");
    }

    #[test]
    fn unlabeled_bar_fails_color_independent() {
        let profile = A11yProfile::first_title();
        let ui = UiManifest::from_widgets(
            Epoch::ZERO,
            [Widget {
                kind: WidgetKind::Bar { value: 1, cap: 1 },
                body: "  ".into(),
            }],
        );
        let root = pause_menu("en", &profile);
        let frame = layout(&root, aspect_viewport(1_920, 1_080), &profile, "en");
        let rows = compliance_evidence(&profile, &BindTable::hearth(), &frame, Some(&ui));
        assert!(
            rows.iter()
                .any(|r| r.requirement == "color_independent" && !r.present)
        );
    }

    #[test]
    fn settings_menu_proves() {
        let profile = A11yProfile::max_access();
        prove_menu(
            &settings_menu("ja", &profile),
            aspect_viewport(1_440, 1_080),
            &profile,
            "ja",
            &BindTable::hearth(),
            None,
        )
        .unwrap();
    }
}
