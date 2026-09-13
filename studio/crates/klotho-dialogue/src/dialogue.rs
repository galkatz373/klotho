//! Compiled dialogue modules: stable keys, conditions, choices, VO/CC.

use serde::{Deserialize, Serialize};

use klotho_core::Tick;
use klotho_ir::{AnchorId, Name};
use klotho_prove::{BlobId, ReleaseRights};

use crate::bible::StoryBible;
use crate::error::DialogueError;
use crate::quest::QuestGraph;

fn check_name(field: &str, name: &Name) -> Result<(), DialogueError> {
    if name.as_str().is_empty() {
        Err(DialogueError::Name {
            field: field.to_owned(),
        })
    } else {
        Ok(())
    }
}

/// Condition over Knows / quest completion. Closed, not free text.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DialogueCond {
    /// Always available.
    Always,
    /// Player Knows this fact.
    Knows(Name),
    /// Named quest completed (its grants are in the live Knows set).
    Quest(Name),
    /// Conjunction.
    All(Vec<DialogueCond>),
    /// Disjunction.
    Any(Vec<DialogueCond>),
    /// Negation.
    Not(Box<DialogueCond>),
}

impl DialogueCond {
    fn check(&self) -> Result<(), DialogueError> {
        match self {
            Self::Always => Ok(()),
            Self::Knows(n) | Self::Quest(n) => check_name("condition", n),
            Self::All(xs) | Self::Any(xs) => {
                for x in xs {
                    x.check()?;
                }
                Ok(())
            }
            Self::Not(x) => x.check(),
        }
    }

    /// Facts this condition reads.
    #[must_use]
    pub fn reads(&self) -> Vec<Name> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out.sort();
        out.dedup();
        out
    }

    fn collect(&self, out: &mut Vec<Name>) {
        match self {
            Self::Always => {}
            Self::Knows(n) | Self::Quest(n) => out.push(n.clone()),
            Self::All(xs) | Self::Any(xs) => {
                for x in xs {
                    x.collect(out);
                }
            }
            Self::Not(x) => x.collect(out),
        }
    }

    /// Evaluate against a live Knows set (fact names).
    #[must_use]
    pub fn holds(&self, knows: &[Name]) -> bool {
        match self {
            Self::Always => true,
            Self::Knows(n) | Self::Quest(n) => knows.iter().any(|k| k == n),
            Self::All(xs) => xs.iter().all(|x| x.holds(knows)),
            Self::Any(xs) => xs.iter().any(|x| x.holds(knows)),
            Self::Not(x) => !x.holds(knows),
        }
    }
}

/// Player-facing choice. Selecting it grants facts and jumps to a line.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueChoice {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Choice id (also a loc key suffix).
    pub id: Name,
    /// Localization key for the prompt.
    pub key: Name,
    /// Extra condition on this choice.
    pub condition: DialogueCond,
    /// Knows facts granted when taken.
    pub grants: Vec<Name>,
    /// Next line key.
    pub next: Name,
}

impl DialogueChoice {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("choice", &self.id)?;
        check_name("choice.key", &self.key)?;
        check_name("choice.next", &self.next)?;
        self.condition.check()?;
        for g in &self.grants {
            check_name("choice.grant", g)?;
        }
        Ok(())
    }
}

/// VO / subtitle timing in ticks.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineTiming {
    /// Subtitle and VO start.
    pub start: Tick,
    /// Inclusive duration. Must cover reading speed.
    pub duration: Tick,
}

/// Closed-caption / SDH payload. Required for release.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosedCaption {
    /// Speaker tag.
    pub speaker: Name,
    /// Caption body, including SDH non-speech cues.
    pub body: String,
    /// True when the body includes non-speech SDH.
    pub sdh: bool,
}

impl ClosedCaption {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("cc.speaker", &self.speaker)?;
        if self.body.is_empty() {
            return Err(DialogueError::Dialogue {
                token: self.speaker.as_str().to_owned(),
                reason: "empty closed caption".into(),
            });
        }
        Ok(())
    }
}

/// Approved VO binding. Bytes live in CAS; rights must be complete for release.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoBinding {
    /// Grain blob.
    pub blob: BlobId,
    /// Performer / casting record.
    pub rights: ReleaseRights,
}

/// One compiled line. The key is the localization identity.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueLine {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Stable localization key. Never derived from list position.
    pub key: Name,
    /// Speaker character.
    pub speaker: Name,
    /// Timeline beat this line belongs to (narrative order).
    pub beat: Name,
    /// Availability condition.
    pub condition: DialogueCond,
    /// Knows facts granted when the line plays.
    pub grants: Vec<Name>,
    /// Player choices. Empty is a bark / linear line.
    pub choices: Vec<DialogueChoice>,
    /// VO / subtitle timing.
    pub timing: LineTiming,
    /// Performance notes. Data, never executed.
    pub performance: String,
    /// Closed caption.
    pub cc: ClosedCaption,
    /// Source-locale body used for reading-speed checks. Loc catalogs override.
    pub source_text: String,
    /// Optional approved VO. Required at release when `vo_required`.
    pub vo: Option<VoBinding>,
    /// Release requires a VO binding.
    pub vo_required: bool,
    /// Next line when there are no choices. `None` ends the conversation.
    pub next: Option<Name>,
}

impl DialogueLine {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("line", &self.key)?;
        check_name("line.speaker", &self.speaker)?;
        check_name("line.beat", &self.beat)?;
        self.condition.check()?;
        for g in &self.grants {
            check_name("line.grant", g)?;
        }
        for c in &self.choices {
            c.check()?;
        }
        self.cc.check()?;
        if self.source_text.is_empty() {
            return Err(DialogueError::Dialogue {
                token: self.key.as_str().to_owned(),
                reason: "empty source text".into(),
            });
        }
        if self.timing.duration.0 == 0 {
            return Err(DialogueError::Dialogue {
                token: self.key.as_str().to_owned(),
                reason: "zero duration".into(),
            });
        }
        Ok(())
    }
}

/// One conversation / bark set. Compiles to Beats, Rites, Knows, and Manifest cues.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueModule {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Module id.
    pub id: Name,
    /// Entry line key.
    pub entry: Name,
    /// Lines. Review sorts by bible beat, then speaker, then key.
    pub lines: Vec<DialogueLine>,
}

impl DialogueModule {
    /// Structural checks plus bible/quest continuity.
    pub fn validate(&self, bible: &StoryBible, quests: &QuestGraph) -> Result<(), DialogueError> {
        check_name("dialogue", &self.id)?;
        check_name("dialogue.entry", &self.entry)?;
        for line in &self.lines {
            line.check()?;
        }
        let mut keys = Vec::new();
        for line in &self.lines {
            if keys.iter().any(|k: &Name| k == &line.key) {
                return Err(DialogueError::Dialogue {
                    token: line.key.as_str().to_owned(),
                    reason: "duplicate line key".into(),
                });
            }
            keys.push(line.key.clone());
            if bible.character(&line.speaker).is_none() {
                return Err(DialogueError::Dialogue {
                    token: line.key.as_str().to_owned(),
                    reason: format!("unknown speaker {}", line.speaker),
                });
            }
            if bible.beat(&line.beat).is_none() {
                return Err(DialogueError::Dialogue {
                    token: line.key.as_str().to_owned(),
                    reason: format!("unknown beat {}", line.beat),
                });
            }
            if line.cc.speaker != line.speaker {
                return Err(DialogueError::Dialogue {
                    token: line.key.as_str().to_owned(),
                    reason: "caption speaker mismatch".into(),
                });
            }
            self.check_condition(bible, quests, &line.condition, &line.key)?;
            for g in &line.grants {
                self.check_grant(bible, line, g)?;
            }
            for choice in &line.choices {
                self.check_condition(bible, quests, &choice.condition, &choice.key)?;
            }
        }
        if !keys.iter().any(|k| k == &self.entry) {
            return Err(DialogueError::Dialogue {
                token: self.entry.as_str().to_owned(),
                reason: "entry line missing".into(),
            });
        }
        for line in &self.lines {
            if let Some(next) = &line.next {
                if !keys.iter().any(|k| k == next) {
                    return Err(DialogueError::Dialogue {
                        token: line.key.as_str().to_owned(),
                        reason: format!("unknown next {}", next),
                    });
                }
            }
            for choice in &line.choices {
                if !keys.iter().any(|k| k == &choice.next) {
                    return Err(DialogueError::Dialogue {
                        token: choice.id.as_str().to_owned(),
                        reason: format!("unknown next {}", choice.next),
                    });
                }
            }
        }
        Ok(())
    }

    fn check_condition(
        &self,
        bible: &StoryBible,
        quests: &QuestGraph,
        cond: &DialogueCond,
        key: &Name,
    ) -> Result<(), DialogueError> {
        for read in cond.reads() {
            let known_secret = bible.secret(&read).is_some();
            let known_quest = quests.get(&read).is_some();
            let known_grant = quests
                .quests
                .iter()
                .any(|q| q.grants.iter().any(|g| g == &read));
            if !known_secret && !known_quest && !known_grant {
                return Err(DialogueError::Dialogue {
                    token: key.as_str().to_owned(),
                    reason: format!("condition cites unknown {}", read),
                });
            }
        }
        Ok(())
    }

    fn check_grant(
        &self,
        bible: &StoryBible,
        line: &DialogueLine,
        grant: &Name,
    ) -> Result<(), DialogueError> {
        if let Some(secret) = bible.secret(grant) {
            let Some(reveal) = bible.beat(&secret.reveal_at) else {
                return Ok(());
            };
            let Some(line_beat) = bible.beat(&line.beat) else {
                return Ok(());
            };
            if line_beat.order < reveal.order {
                return Err(DialogueError::Continuity {
                    token: line.key.as_str().to_owned(),
                    reason: format!("premature knowledge {}", grant),
                });
            }
        }
        Ok(())
    }

    /// Line by key.
    #[must_use]
    pub fn line(&self, key: &Name) -> Option<&DialogueLine> {
        self.lines.iter().find(|l| &l.key == key)
    }
}
