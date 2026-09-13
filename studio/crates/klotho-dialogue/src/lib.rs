//! Story bible, quest coherence, compiled dialogue, and localization (KAI-15).
//!
//! Title dialogue is compiled content (K76). Branching, conditions, loc keys,
//! subtitle/VO timing, and Beats lower to existing predicates, Rites, Knows,
//! and Manifest cues. There is no runtime model dependency.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod bible;
mod dialogue;
mod error;
mod impact;
mod loc;
mod lower;
mod project;
mod quest;
mod release;
mod replay;
mod sample;

pub use bible::{
    ApprovedException, CharacterFact, GlossaryEntry, LocationFact, Presence, RatingLimits,
    SecretFact, StoryBible, ThemeFact, TimelineBeat, UnresolvedQuestion, VoiceProfile,
};
pub use dialogue::{
    ClosedCaption, DialogueChoice, DialogueCond, DialogueLine, DialogueModule, LineTiming,
    VoBinding,
};
pub use error::DialogueError;
pub use impact::{ImpactEdge, ImpactGraph, ImpactKind};
pub use loc::{
    FontContract, Gender, LinguisticApproval, LocaleCatalog, LocaleId, Message, PSEUDO_LOCALE,
    PluralForm, SHIPPING_LOCALES, ShapingScript, pseudo_locale, shaping_for,
};
pub use lower::{LoweredNarrative, lower, presentation_hash};
pub use project::NarrativeProject;
pub use quest::{QuestExclusion, QuestGraph, QuestNode, ReentryKind};
pub use release::{release_check, required_locales};
pub use replay::{ConversationReplay, replay_locales, walk};
pub use sample::observatory;

#[cfg(test)]
mod tests;
