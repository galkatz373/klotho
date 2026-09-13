//! Deterministic conditional-conversation replay across locales.

use std::collections::BTreeMap;

use klotho_core::Hash;
use klotho_ir::Name;

use crate::dialogue::DialogueModule;
use crate::error::DialogueError;
use crate::loc::{LocaleCatalog, SHIPPING_LOCALES};
use crate::lower::{LoweredNarrative, presentation_hash};

/// Walk result shared by every locale.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ConversationReplay {
    /// Locale-independent branch hash (lowered IR).
    pub branch_hash: Hash,
    /// Knows facts after the walk, canonical order.
    pub knows: Vec<Name>,
    /// Played line keys in order.
    pub played: Vec<Name>,
    /// Per-locale presentation hashes. Branch must not depend on these.
    pub presentation: BTreeMap<String, Hash>,
}

/// Play `module` from `entry` under `initial` Knows, consuming `choices` in order.
pub fn walk(
    module: &DialogueModule,
    initial: &[Name],
    choices: &[Name],
) -> Result<(Vec<Name>, Vec<Name>), DialogueError> {
    let mut knows: Vec<Name> = initial.to_vec();
    let mut played = Vec::new();
    let mut choice_i = 0usize;
    let mut current = module.entry.clone();
    let mut guard = 0u32;
    loop {
        guard += 1;
        if guard > 64 {
            return Err(DialogueError::Replay {
                locale: "walk".into(),
                reason: "exceeded line cap".into(),
            });
        }
        let Some(line) = module.line(&current) else {
            return Err(DialogueError::Dialogue {
                token: current.as_str().to_owned(),
                reason: "missing line during replay".into(),
            });
        };
        if !line.condition.holds(&knows) {
            return Err(DialogueError::Replay {
                locale: "walk".into(),
                reason: format!("line {} blocked", line.key),
            });
        }
        played.push(line.key.clone());
        for g in &line.grants {
            if !knows.iter().any(|k| k == g) {
                knows.push(g.clone());
            }
        }
        if !line.choices.is_empty() {
            if choice_i >= choices.len() {
                return Err(DialogueError::Replay {
                    locale: "walk".into(),
                    reason: format!("no choice at {}", line.key),
                });
            }
            let want = &choices[choice_i];
            choice_i += 1;
            let Some(choice) = line.choices.iter().find(|c| &c.id == want) else {
                return Err(DialogueError::Replay {
                    locale: "walk".into(),
                    reason: format!("unknown choice {} at {}", want, line.key),
                });
            };
            if !choice.condition.holds(&knows) {
                return Err(DialogueError::Replay {
                    locale: "walk".into(),
                    reason: format!("choice {} blocked", choice.id),
                });
            }
            for g in &choice.grants {
                if !knows.iter().any(|k| k == g) {
                    knows.push(g.clone());
                }
            }
            current = choice.next.clone();
            continue;
        }
        match &line.next {
            Some(next) => current = next.clone(),
            None => break,
        }
    }
    if choice_i != choices.len() {
        return Err(DialogueError::Replay {
            locale: "walk".into(),
            reason: "unused choices".into(),
        });
    }
    knows.sort();
    Ok((played, knows))
}

/// Replay the same choice sequence in every shipping locale.
///
/// Branch (`played`, `knows`, lowered IR hash) must be identical. Presentation
/// hashes may differ. Locale catalogs do not change conditions.
pub fn replay_locales(
    module: &DialogueModule,
    lowered: &LoweredNarrative,
    catalogs: &[LocaleCatalog],
    initial: &[Name],
    choices: &[Name],
) -> Result<ConversationReplay, DialogueError> {
    let (played, knows) = walk(module, initial, choices)?;
    let branch_hash = lowered.branch_hash();
    let mut presentation = BTreeMap::new();
    for id in SHIPPING_LOCALES {
        let Some(cat) = catalogs.iter().find(|c| c.locale.as_str() == *id) else {
            return Err(DialogueError::Replay {
                locale: (*id).to_owned(),
                reason: "missing shipping locale".into(),
            });
        };
        let (p2, k2) = walk(module, initial, choices)?;
        if p2 != played || k2 != knows {
            return Err(DialogueError::Replay {
                locale: (*id).to_owned(),
                reason: "branch diverged".into(),
            });
        }
        if lowered.with_locale(cat, module).branch_hash() != branch_hash {
            return Err(DialogueError::Replay {
                locale: (*id).to_owned(),
                reason: "locale mutated lowered IR".into(),
            });
        }
        presentation.insert((*id).to_owned(), presentation_hash(lowered, cat, module));
    }
    Ok(ConversationReplay {
        branch_hash,
        knows,
        played,
        presentation,
    })
}
