//! A narrative project: bible + quests + dialogue + locales.

use serde::{Deserialize, Serialize};

use klotho_ir::{AnchorId, Name};

use crate::bible::StoryBible;
use crate::dialogue::DialogueModule;
use crate::error::DialogueError;
use crate::impact::ImpactGraph;
use crate::loc::LocaleCatalog;
use crate::lower::{LoweredNarrative, lower};
use crate::quest::QuestGraph;
use crate::release::release_check;

/// Closed authoring bundle for story, quests, dialogue, and loc.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NarrativeProject {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Project id.
    pub id: Name,
    /// Story bible.
    pub bible: StoryBible,
    /// Quest graph.
    pub quests: QuestGraph,
    /// Dialogue module.
    pub dialogue: DialogueModule,
    /// Locale catalogs, including all shipping locales for release.
    pub locales: Vec<LocaleCatalog>,
}

impl NarrativeProject {
    /// Validate bible, quests, dialogue, and every locale catalog.
    pub fn validate(&self) -> Result<(), DialogueError> {
        if self.id.as_str().is_empty() {
            return Err(DialogueError::Name {
                field: "project".into(),
            });
        }
        self.bible.validate()?;
        self.quests.validate(&self.bible)?;
        self.dialogue.validate(&self.bible, &self.quests)?;
        for loc in &self.locales {
            loc.validate(&self.bible, &self.dialogue)?;
        }
        Ok(())
    }

    /// Validate and run the release gate.
    pub fn validate_release(&self) -> Result<(), DialogueError> {
        self.validate()?;
        release_check(&self.dialogue, &self.locales)
    }

    /// Lower to IR after validation.
    pub fn lower(&self) -> Result<LoweredNarrative, DialogueError> {
        self.validate()?;
        lower(&self.dialogue, &self.quests)
    }

    /// Impact graph for this project.
    #[must_use]
    pub fn impact(&self) -> ImpactGraph {
        ImpactGraph::build(&self.bible, &self.quests, &self.dialogue, &self.locales)
    }

    /// Lines in narrative order (bible beat, speaker, key) — never file order.
    #[must_use]
    pub fn narrative_lines(&self) -> Vec<Name> {
        let mut rows: Vec<(u32, String, String)> = self
            .dialogue
            .lines
            .iter()
            .map(|l| {
                let order = self
                    .bible
                    .beat(&l.beat)
                    .map(|b| b.order)
                    .unwrap_or(u32::MAX);
                (
                    order,
                    l.speaker.as_str().to_owned(),
                    l.key.as_str().to_owned(),
                )
            })
            .collect();
        rows.sort();
        rows.into_iter()
            .map(|(_, _, k)| Name::from(k.as_str()))
            .collect()
    }
}
