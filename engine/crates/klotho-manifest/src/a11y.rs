//! Declarative accessibility descriptors (KAI-16). Disposable presentation.

/// Semantic role for menu focus and screen-reader trees.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum FocusRole {
    /// Top-level menu or settings sheet.
    Menu,
    /// One focusable row.
    Item,
    /// Activate / confirm control.
    Button,
    /// Bounded numeric control.
    Slider,
    /// Binary control.
    Toggle,
    /// Group header. Not focusable unless marked.
    Group,
    /// Subtitle / CC band.
    Caption,
    /// Persistent HUD status. Not in the menu focus cycle.
    Status,
}

impl FocusRole {
    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Menu => "menu",
            Self::Item => "item",
            Self::Button => "button",
            Self::Slider => "slider",
            Self::Toggle => "toggle",
            Self::Group => "group",
            Self::Caption => "caption",
            Self::Status => "status",
        }
    }
}

/// Screen-reader metadata for one menu node. Empty `name` fails compliance.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct ScreenReaderNode {
    /// Stable node id.
    pub id: String,
    /// Role announced to the platform reader.
    pub role: FocusRole,
    /// Accessible name.
    pub name: String,
    /// Current value (`on`, `200%`, glyph).
    pub value: String,
    /// How to operate the control.
    pub hint: String,
    /// Whether this node holds focus.
    pub focused: bool,
}

/// Subtitle / CC band placed in the safe area. Presentation only.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct CaptionBand {
    /// Speaker label.
    pub speaker: String,
    /// Localized body.
    pub body: String,
    /// True when SDH non-speech is included.
    pub sdh: bool,
}

/// One CVAA / first-title accessibility evidence row.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct A11yEvidence {
    /// Requirement id (`remap`, `captions`, `text_scale`, `contrast`,
    /// `color_independent`, `screen_reader`, `focus`, `overflow`).
    pub requirement: String,
    /// Whether the requirement is satisfied.
    pub present: bool,
    /// Short witness (`missing Use on gamepad`, `node settings has no name`).
    pub witness: String,
}

impl A11yEvidence {
    /// Passing row.
    #[must_use]
    pub fn pass(requirement: impl Into<String>) -> Self {
        Self {
            requirement: requirement.into(),
            present: true,
            witness: String::new(),
        }
    }

    /// Failing row with a witness.
    #[must_use]
    pub fn fail(requirement: impl Into<String>, witness: impl Into<String>) -> Self {
        Self {
            requirement: requirement.into(),
            present: false,
            witness: witness.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_pass_and_fail() {
        let ok = A11yEvidence::pass("remap");
        assert!(ok.present);
        let bad = A11yEvidence::fail("captions", "no CC option");
        assert!(!bad.present);
        assert_eq!(FocusRole::Button.as_str(), "button");
    }
}
