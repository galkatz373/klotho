//! Versioned story bible. Authoring truth; never a runtime category.

use serde::{Deserialize, Serialize};

use klotho_ir::{AnchorId, Name};

use crate::error::DialogueError;

fn check_name(field: &str, name: &Name) -> Result<(), DialogueError> {
    if name.as_str().is_empty() {
        Err(DialogueError::Name {
            field: field.to_owned(),
        })
    } else {
        Ok(())
    }
}

/// Character voice used by writers and loc context, not a runtime model.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VoiceProfile {
    /// Register (`dry`, `warm`, `clipped`).
    pub register: Name,
    /// Formality (`thou`, `you`, `honorific`).
    pub formality: Name,
    /// Forbidden phrasing notes. Data, never executed.
    pub forbid: String,
}

impl VoiceProfile {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("voice.register", &self.register)?;
        check_name("voice.formality", &self.formality)
    }
}

/// Character facts and voice. AI suggestions must cite [`Self::anchor`].
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterFact {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Current name. Rename does not change [`Self::anchor`].
    pub name: Name,
    /// Spoken voice.
    pub voice: VoiceProfile,
    /// Public facts this character owns.
    pub facts: Vec<Name>,
}

impl CharacterFact {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("character", &self.name)?;
        self.voice.check()?;
        for fact in &self.facts {
            check_name("character.fact", fact)?;
        }
        Ok(())
    }
}

/// Ordered timeline beat. Locations are exclusive per character at a beat.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineBeat {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Beat id (`arrival`).
    pub id: Name,
    /// Narrative order. Lower sorts first.
    pub order: u32,
    /// Location this beat occupies.
    pub location: Name,
}

impl TimelineBeat {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("timeline", &self.id)?;
        check_name("timeline.location", &self.location)
    }
}

/// Place named by the bible.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationFact {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Place name.
    pub name: Name,
}

impl LocationFact {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("location", &self.name)
    }
}

/// Term that loc catalogs and AI suggestions must cite.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlossaryEntry {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Term key.
    pub term: Name,
    /// Source-locale gloss. Not executable.
    pub gloss: String,
}

impl GlossaryEntry {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("glossary", &self.term)?;
        if self.gloss.is_empty() {
            return Err(DialogueError::Name {
                field: "glossary.gloss".into(),
            });
        }
        Ok(())
    }
}

/// Secret gated by a Knows fact. Revealing it without the grant is premature.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretFact {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Knows fact name.
    pub knows: Name,
    /// Owner character.
    pub owner: Name,
    /// Earliest timeline beat that may reveal it.
    pub reveal_at: Name,
}

impl SecretFact {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("secret.knows", &self.knows)?;
        check_name("secret.owner", &self.owner)?;
        check_name("secret.reveal_at", &self.reveal_at)
    }
}

/// Theme the writing room tracks.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFact {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Theme id.
    pub id: Name,
}

impl ThemeFact {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("theme", &self.id)
    }
}

/// Content and rating limits. Enforcement is authoring-time.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RatingLimits {
    /// Rating board label (`esrb_t`).
    pub board: Name,
    /// Forbidden content tags.
    pub forbid: Vec<Name>,
}

impl RatingLimits {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("rating.board", &self.board)?;
        for tag in &self.forbid {
            check_name("rating.forbid", tag)?;
        }
        Ok(())
    }
}

/// Open question the bible does not yet resolve.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnresolvedQuestion {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Question id.
    pub id: Name,
}

impl UnresolvedQuestion {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("unresolved", &self.id)
    }
}

/// Named, approved exception to a bible rule.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedException {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Exception id.
    pub id: Name,
    /// Named human owner.
    pub owner: Name,
}

impl ApprovedException {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("exception", &self.id)?;
        check_name("exception.owner", &self.owner)
    }
}

/// Where a character stands during a timeline beat.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presence {
    /// Character.
    pub character: Name,
    /// Timeline beat.
    pub beat: Name,
    /// Location.
    pub location: Name,
}

impl Presence {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("presence.character", &self.character)?;
        check_name("presence.beat", &self.beat)?;
        check_name("presence.location", &self.location)
    }
}

/// Versioned writers' room constitution. Never executes in the game.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoryBible {
    /// Immutable bible identity.
    pub anchor: AnchorId,
    /// Explicit version. A bump invalidates dependent evidence.
    pub version: u32,
    /// Characters, sorted by name at validate.
    pub characters: Vec<CharacterFact>,
    /// Timeline, sorted by `order` at validate.
    pub timeline: Vec<TimelineBeat>,
    /// Locations.
    pub locations: Vec<LocationFact>,
    /// Glossary terms.
    pub glossary: Vec<GlossaryEntry>,
    /// Knows-gated secrets.
    pub secrets: Vec<SecretFact>,
    /// Themes.
    pub themes: Vec<ThemeFact>,
    /// Rating / content limits.
    pub rating: RatingLimits,
    /// Open questions.
    pub unresolved: Vec<UnresolvedQuestion>,
    /// Approved exceptions.
    pub exceptions: Vec<ApprovedException>,
    /// Character presence per beat. Contradictory rows fail closed.
    pub presence: Vec<Presence>,
}

impl StoryBible {
    /// Structural and continuity validation.
    pub fn validate(&self) -> Result<(), DialogueError> {
        if self.version == 0 {
            return Err(DialogueError::Name {
                field: "bible.version".into(),
            });
        }
        self.rating.check()?;
        for c in &self.characters {
            c.check()?;
        }
        for t in &self.timeline {
            t.check()?;
        }
        for l in &self.locations {
            l.check()?;
        }
        for g in &self.glossary {
            g.check()?;
        }
        for s in &self.secrets {
            s.check()?;
        }
        for t in &self.themes {
            t.check()?;
        }
        for q in &self.unresolved {
            q.check()?;
        }
        for e in &self.exceptions {
            e.check()?;
        }
        for p in &self.presence {
            p.check()?;
        }
        unique_names(self.characters.iter().map(|c| c.name.as_str()), "character")?;
        unique_names(self.timeline.iter().map(|t| t.id.as_str()), "timeline")?;
        unique_names(self.locations.iter().map(|l| l.name.as_str()), "location")?;
        unique_names(self.glossary.iter().map(|g| g.term.as_str()), "glossary")?;
        unique_names(self.secrets.iter().map(|s| s.knows.as_str()), "secret")?;
        self.check_refs()?;
        self.check_presence()?;
        Ok(())
    }

    fn check_refs(&self) -> Result<(), DialogueError> {
        let chars: Vec<&str> = self.characters.iter().map(|c| c.name.as_str()).collect();
        let beats: Vec<&str> = self.timeline.iter().map(|t| t.id.as_str()).collect();
        let locs: Vec<&str> = self.locations.iter().map(|l| l.name.as_str()).collect();
        for t in &self.timeline {
            if !locs.iter().any(|l| *l == t.location.as_str()) {
                return Err(DialogueError::Continuity {
                    token: t.id.as_str().to_owned(),
                    reason: format!("unknown location {}", t.location),
                });
            }
        }
        for s in &self.secrets {
            if !chars.iter().any(|c| *c == s.owner.as_str()) {
                return Err(DialogueError::Continuity {
                    token: s.knows.as_str().to_owned(),
                    reason: format!("unknown owner {}", s.owner),
                });
            }
            if !beats.iter().any(|b| *b == s.reveal_at.as_str()) {
                return Err(DialogueError::Continuity {
                    token: s.knows.as_str().to_owned(),
                    reason: format!("unknown reveal beat {}", s.reveal_at),
                });
            }
        }
        Ok(())
    }

    fn check_presence(&self) -> Result<(), DialogueError> {
        let chars: Vec<&str> = self.characters.iter().map(|c| c.name.as_str()).collect();
        let beats: Vec<&str> = self.timeline.iter().map(|t| t.id.as_str()).collect();
        let locs: Vec<&str> = self.locations.iter().map(|l| l.name.as_str()).collect();
        let mut seen: Vec<(String, String, String)> = Vec::new();
        for p in &self.presence {
            if !chars.iter().any(|c| *c == p.character.as_str()) {
                return Err(DialogueError::Continuity {
                    token: p.character.as_str().to_owned(),
                    reason: "presence names unknown character".into(),
                });
            }
            if !beats.iter().any(|b| *b == p.beat.as_str()) {
                return Err(DialogueError::Continuity {
                    token: p.beat.as_str().to_owned(),
                    reason: "presence names unknown beat".into(),
                });
            }
            if !locs.iter().any(|l| *l == p.location.as_str()) {
                return Err(DialogueError::Continuity {
                    token: p.location.as_str().to_owned(),
                    reason: "presence names unknown location".into(),
                });
            }
            if let Some(prev) = seen
                .iter()
                .find(|(c, b, _)| c == p.character.as_str() && b == p.beat.as_str())
            {
                if prev.2 != p.location.as_str() {
                    return Err(DialogueError::Continuity {
                        token: p.character.as_str().to_owned(),
                        reason: format!(
                            "at {} cannot be both {} and {}",
                            p.beat, prev.2, p.location
                        ),
                    });
                }
            } else {
                seen.push((
                    p.character.as_str().to_owned(),
                    p.beat.as_str().to_owned(),
                    p.location.as_str().to_owned(),
                ));
            }
        }
        Ok(())
    }

    /// Character by name.
    #[must_use]
    pub fn character(&self, name: &Name) -> Option<&CharacterFact> {
        self.characters.iter().find(|c| &c.name == name)
    }

    /// Secret by Knows fact.
    #[must_use]
    pub fn secret(&self, knows: &Name) -> Option<&SecretFact> {
        self.secrets.iter().find(|s| &s.knows == knows)
    }

    /// Timeline beat by id.
    #[must_use]
    pub fn beat(&self, id: &Name) -> Option<&TimelineBeat> {
        self.timeline.iter().find(|t| &t.id == id)
    }
}

fn unique_names<'a>(names: impl Iterator<Item = &'a str>, kind: &str) -> Result<(), DialogueError> {
    let mut seen = Vec::new();
    for n in names {
        if seen.iter().any(|s: &String| s == n) {
            return Err(DialogueError::Continuity {
                token: n.to_owned(),
                reason: format!("duplicate {kind}"),
            });
        }
        seen.push(n.to_owned());
    }
    Ok(())
}
