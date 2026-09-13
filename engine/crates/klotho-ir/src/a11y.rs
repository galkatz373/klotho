//! First-title accessibility profile (KAI-16).
//!
//! Presentation and input overlays. Never a Projection column and never an
//! executable widget script.

use serde::{Deserialize, Serialize};

use crate::error::IrError;
use crate::feel::FeelAccessibility;

/// Contrast presentation. Color is never the only state cue.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContrastMode {
    /// Production palette.
    #[default]
    Default,
    /// High-contrast tokens plus numeric/shape labels.
    High,
}

impl ContrastMode {
    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::High => "high",
        }
    }
}

/// Subtitle versus closed-caption / SDH presentation.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionMode {
    /// No caption band.
    Off,
    /// Dialogue subtitles.
    #[default]
    Subtitles,
    /// Subtitles plus SDH / closed captions.
    ClosedCaptions,
}

impl CaptionMode {
    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Subtitles => "subtitles",
            Self::ClosedCaptions => "closed_captions",
        }
    }
}

/// Authorable accessibility overlay. Mini-Tapestry `accessibility.ron` is this type.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct A11yProfile {
    /// Full action remapping is required and offered.
    pub remap: bool,
    /// Hold becomes toggle for hold actions.
    pub hold_to_toggle: bool,
    /// Dialogue subtitle band.
    pub subtitles: bool,
    /// SDH / closed captions. Defaults off so existing fixtures stay valid.
    #[serde(default)]
    pub closed_captions: bool,
    /// UI scale in thousandths. Clamped to 750..=2000.
    pub text_scale_milli: u16,
    /// Contrast variant.
    pub contrast: ContrastMode,
    /// Suppress non-essential motion in menus and Manifest transitions.
    pub reduce_motion: bool,
    /// Zero presented camera shake.
    pub reduce_shake: bool,
    /// Emit screen-reader metadata for menu flows.
    #[serde(default)]
    pub screen_reader: bool,
}

impl Default for A11yProfile {
    fn default() -> Self {
        Self::first_title()
    }
}

impl A11yProfile {
    /// Inclusive lower UI scale.
    pub const MIN_SCALE: u16 = 750;
    /// Inclusive upper UI scale.
    pub const MAX_SCALE: u16 = 2_000;

    /// Readable first-title defaults: remapping and subtitles on, 100% scale.
    #[must_use]
    pub const fn first_title() -> Self {
        Self {
            remap: true,
            hold_to_toggle: false,
            subtitles: true,
            closed_captions: false,
            text_scale_milli: 1_000,
            contrast: ContrastMode::Default,
            reduce_motion: false,
            reduce_shake: false,
            screen_reader: false,
        }
    }

    /// High-contrast, large-text, reduced-motion, captions, and reader.
    #[must_use]
    pub const fn max_access() -> Self {
        Self {
            remap: true,
            hold_to_toggle: true,
            subtitles: true,
            closed_captions: true,
            text_scale_milli: Self::MAX_SCALE,
            contrast: ContrastMode::High,
            reduce_motion: true,
            reduce_shake: true,
            screen_reader: true,
        }
    }

    /// Caption presentation derived from the two booleans.
    #[must_use]
    pub const fn caption_mode(&self) -> CaptionMode {
        if self.closed_captions {
            CaptionMode::ClosedCaptions
        } else if self.subtitles {
            CaptionMode::Subtitles
        } else {
            CaptionMode::Off
        }
    }

    /// Feel overlay for the same profile. Recovery WAIT is not rewritten.
    #[must_use]
    pub const fn feel(&self) -> FeelAccessibility {
        FeelAccessibility {
            reduce_shake: self.reduce_shake,
            reduce_haptics: self.reduce_motion,
            hold_to_toggle: self.hold_to_toggle,
            aim_assist_required: false,
        }
    }

    /// Fail closed on an out-of-range scale.
    pub fn validate(&self) -> Result<(), IrError> {
        if self.text_scale_milli < Self::MIN_SCALE || self.text_scale_milli > Self::MAX_SCALE {
            return Err(IrError::InvalidA11y {
                field: "text_scale_milli".into(),
                reason: format!(
                    "{} not in {}..={}",
                    self.text_scale_milli,
                    Self::MIN_SCALE,
                    Self::MAX_SCALE
                ),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_ron, to_ron};

    #[test]
    fn first_title_validates_and_round_trips() {
        let p = A11yProfile::first_title();
        p.validate().unwrap();
        let encoded = to_ron(&p).unwrap();
        let decoded: A11yProfile = from_ron(&encoded).unwrap();
        assert_eq!(decoded, p);
        assert_eq!(p.caption_mode(), CaptionMode::Subtitles);
        assert!(!p.feel().reduce_shake);
    }

    #[test]
    fn mini_tapestry_ron_parses() {
        let src = r#"(
            remap: true,
            hold_to_toggle: true,
            subtitles: true,
            text_scale_milli: 1000,
            contrast: default,
            reduce_motion: true,
            reduce_shake: true,
        )"#;
        let p: A11yProfile = from_ron(src).unwrap();
        assert!(p.remap);
        assert!(p.hold_to_toggle);
        assert!(!p.screen_reader);
        assert!(!p.closed_captions);
        assert_eq!(p.contrast, ContrastMode::Default);
    }

    #[test]
    fn scale_out_of_range_fails() {
        let mut p = A11yProfile::first_title();
        p.text_scale_milli = 500;
        assert!(p.validate().is_err());
        p.text_scale_milli = 2_001;
        assert!(p.validate().is_err());
    }

    #[test]
    fn max_access_is_high_contrast_captions_and_reader() {
        let p = A11yProfile::max_access();
        p.validate().unwrap();
        assert_eq!(p.contrast, ContrastMode::High);
        assert_eq!(p.caption_mode(), CaptionMode::ClosedCaptions);
        assert!(p.screen_reader);
        assert!(p.feel().reduce_shake);
        assert!(p.feel().hold_to_toggle);
    }
}
