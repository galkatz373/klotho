//! Dialogue, quest, and localization failures. Never a kernel fault.

use core::fmt;

use klotho_ir::{AnchorId, Diagnostic, FailureClass, diagnose_named};

/// Why a story, quest, dialogue, or loc check failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum DialogueError {
    /// Empty or illegal identifier.
    Name {
        /// Field.
        field: String,
    },
    /// Story-bible continuity contradiction.
    Continuity {
        /// Conflicting fact or character.
        token: String,
        /// Why the facts cannot hold together.
        reason: String,
    },
    /// Quest graph is impossible, cyclic, or orphaned.
    Quest {
        /// Quest or fact id.
        token: String,
        /// Why the graph is illegal.
        reason: String,
    },
    /// Dialogue line, choice, or condition is illegal.
    Dialogue {
        /// Line key or speaker.
        token: String,
        /// Why the line failed.
        reason: String,
    },
    /// Localization key, locale, or message data is illegal.
    Loc {
        /// Key or locale.
        token: String,
        /// Why the catalog failed.
        reason: String,
    },
    /// Missing shipping key, VO, CC, font, approval, or rights.
    Release {
        /// Missing artifact.
        token: String,
        /// Required evidence class.
        missing: String,
    },
    /// A canon/bible edit left stale dependent evidence.
    Stale {
        /// Dependent kind (`dialogue`, `quest`, `vo`, `loc`, `journey`).
        kind: String,
        /// Dependent token.
        token: String,
    },
    /// Conversation replay diverged across locales.
    Replay {
        /// Locale that diverged.
        locale: String,
        /// Why the branch was not identical.
        reason: String,
    },
}

impl DialogueError {
    /// Shared diagnostic envelope.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        let message = self.to_string();
        match self {
            Self::Name { field } => diagnose_named(
                "DIALOGUE.Name",
                FailureClass::Schema,
                "name",
                field,
                message,
            ),
            Self::Continuity { token, .. } => diagnose_named(
                "DIALOGUE.Continuity",
                FailureClass::Contradiction,
                "bible",
                token,
                message,
            ),
            Self::Quest { token, .. } => diagnose_named(
                "DIALOGUE.Quest",
                FailureClass::Contradiction,
                "quest",
                token,
                message,
            ),
            Self::Dialogue { token, .. } => diagnose_named(
                "DIALOGUE.Line",
                FailureClass::Schema,
                "line",
                token,
                message,
            ),
            Self::Loc { token, .. } => {
                diagnose_named("DIALOGUE.Loc", FailureClass::Schema, "loc", token, message)
            }
            Self::Release { token, .. } => diagnose_named(
                "DIALOGUE.Release",
                FailureClass::Provenance,
                "release",
                token,
                message,
            ),
            Self::Stale { token, .. } => diagnose_named(
                "DIALOGUE.Stale",
                FailureClass::Reproducibility,
                "evidence",
                token,
                message,
            ),
            Self::Replay { locale, .. } => diagnose_named(
                "DIALOGUE.Replay",
                FailureClass::Journey,
                "locale",
                locale,
                message,
            ),
        }
    }

    /// Primary blame anchor.
    #[must_use]
    pub fn primary(&self) -> AnchorId {
        self.to_diagnostic().primary
    }
}

impl fmt::Display for DialogueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name { field } => write!(f, "EmptyName({field})"),
            Self::Continuity { token, reason } => write!(f, "Continuity({token}: {reason})"),
            Self::Quest { token, reason } => write!(f, "Quest({token}: {reason})"),
            Self::Dialogue { token, reason } => write!(f, "Dialogue({token}: {reason})"),
            Self::Loc { token, reason } => write!(f, "Loc({token}: {reason})"),
            Self::Release { token, missing } => write!(f, "Release({token} missing {missing})"),
            Self::Stale { kind, token } => write!(f, "Stale({kind}:{token})"),
            Self::Replay { locale, reason } => write!(f, "Replay({locale}: {reason})"),
        }
    }
}

impl core::error::Error for DialogueError {}
