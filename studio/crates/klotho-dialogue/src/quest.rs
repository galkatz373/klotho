//! Quest prerequisite / grant / failure / re-entry graph.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use klotho_ir::{AnchorId, Name};

use crate::bible::StoryBible;
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

/// How a quest may be re-entered after save / failure.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReentryKind {
    /// Completing or failing ends the quest forever.
    Never,
    /// Re-enter from the last checkpoint fact.
    Checkpoint,
    /// Always available after cancel.
    Always,
}

impl ReentryKind {
    /// Catalog snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Checkpoint => "checkpoint",
            Self::Always => "always",
        }
    }
}

/// One authored quest node.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestNode {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Quest id.
    pub id: Name,
    /// Other quests or Knows facts that must hold.
    pub prerequisites: Vec<Name>,
    /// Knows facts granted on success.
    pub grants: Vec<Name>,
    /// Failure / cancel successor quest, if any.
    pub failure: Option<Name>,
    /// Explicit cancel successor. Distinct from failure.
    pub cancel: Option<Name>,
    /// Save / re-entry policy.
    pub reentry: ReentryKind,
    /// Critical-path membership.
    pub critical: bool,
    /// Offered with no prerequisites.
    pub available_at_start: bool,
    /// True when a cycle through this node is an authored escape.
    pub escape: bool,
    /// Ending / terminal node.
    pub ending: bool,
}

impl QuestNode {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("quest", &self.id)?;
        for p in &self.prerequisites {
            check_name("quest.prerequisite", p)?;
        }
        for g in &self.grants {
            check_name("quest.grant", g)?;
        }
        if let Some(f) = &self.failure {
            check_name("quest.failure", f)?;
        }
        if let Some(c) = &self.cancel {
            check_name("quest.cancel", c)?;
        }
        Ok(())
    }
}

/// Pairwise mutual exclusion. Both quests cannot be completed.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestExclusion {
    /// First quest.
    pub a: Name,
    /// Second quest.
    pub b: Name,
}

/// Quest graph for a title increment.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestGraph {
    /// Immutable identity.
    pub anchor: AnchorId,
    /// Nodes. Validate sorts by id.
    pub quests: Vec<QuestNode>,
    /// Mutual exclusions.
    pub exclusions: Vec<QuestExclusion>,
}

impl QuestGraph {
    /// Structural, cycle, orphan, and ending checks against `bible`.
    pub fn validate(&self, bible: &StoryBible) -> Result<(), DialogueError> {
        for q in &self.quests {
            q.check()?;
        }
        unique_ids(&self.quests)?;
        let ids: BTreeSet<&str> = self.quests.iter().map(|q| q.id.as_str()).collect();
        let secrets: BTreeSet<&str> = bible.secrets.iter().map(|s| s.knows.as_str()).collect();
        let mut grants: BTreeMap<&str, &str> = BTreeMap::new();
        for q in &self.quests {
            for p in &q.prerequisites {
                if !ids.contains(p.as_str()) && !secrets.contains(p.as_str()) {
                    return Err(DialogueError::Quest {
                        token: q.id.as_str().to_owned(),
                        reason: format!("unknown prerequisite {}", p.as_str()),
                    });
                }
            }
            if let Some(f) = &q.failure {
                if !ids.contains(f.as_str()) {
                    return Err(DialogueError::Quest {
                        token: q.id.as_str().to_owned(),
                        reason: format!("unknown failure {}", f.as_str()),
                    });
                }
            }
            if let Some(c) = &q.cancel {
                if !ids.contains(c.as_str()) {
                    return Err(DialogueError::Quest {
                        token: q.id.as_str().to_owned(),
                        reason: format!("unknown cancel {}", c.as_str()),
                    });
                }
            }
            for g in &q.grants {
                if let Some(prev) = grants.insert(g.as_str(), q.id.as_str()) {
                    return Err(DialogueError::Quest {
                        token: g.as_str().to_owned(),
                        reason: format!("grant duplicated by {prev} and {}", q.id),
                    });
                }
            }
        }
        for ex in &self.exclusions {
            check_name("exclusion.a", &ex.a)?;
            check_name("exclusion.b", &ex.b)?;
            if !ids.contains(ex.a.as_str()) || !ids.contains(ex.b.as_str()) {
                return Err(DialogueError::Quest {
                    token: ex.a.as_str().to_owned(),
                    reason: "exclusion names unknown quest".into(),
                });
            }
        }
        self.check_cycles()?;
        self.check_orphans()?;
        self.check_critical_reachability()?;
        self.check_endings()?;
        Ok(())
    }

    fn check_cycles(&self) -> Result<(), DialogueError> {
        let idx = index(self);
        for q in &self.quests {
            if let Some(cycle) = cycle_from(q.id.as_str(), &idx) {
                let escape = cycle
                    .iter()
                    .any(|id| self.quests.iter().any(|n| n.id.as_str() == *id && n.escape));
                if !escape {
                    return Err(DialogueError::Quest {
                        token: q.id.as_str().to_owned(),
                        reason: format!("cycle without escape ({})", cycle.join("->")),
                    });
                }
            }
        }
        Ok(())
    }

    fn check_orphans(&self) -> Result<(), DialogueError> {
        let referenced: BTreeSet<&str> = self
            .quests
            .iter()
            .flat_map(|q| {
                q.prerequisites
                    .iter()
                    .chain(q.failure.iter())
                    .chain(q.cancel.iter())
            })
            .map(|n| n.as_str())
            .collect();
        for q in &self.quests {
            let offered = q.available_at_start || q.critical || referenced.contains(q.id.as_str());
            if !offered && q.prerequisites.is_empty() && !q.ending {
                return Err(DialogueError::Quest {
                    token: q.id.as_str().to_owned(),
                    reason: "orphan objective".into(),
                });
            }
        }
        Ok(())
    }

    fn check_critical_reachability(&self) -> Result<(), DialogueError> {
        let reachable = reachable_start(self);
        for q in &self.quests {
            if q.critical && !reachable.contains(q.id.as_str()) {
                return Err(DialogueError::Quest {
                    token: q.id.as_str().to_owned(),
                    reason: "critical quest is unreachable".into(),
                });
            }
        }
        Ok(())
    }

    fn check_endings(&self) -> Result<(), DialogueError> {
        let endings: Vec<&QuestNode> = self.quests.iter().filter(|q| q.ending).collect();
        if endings.is_empty() {
            return Err(DialogueError::Quest {
                token: "ending".into(),
                reason: "no authored ending".into(),
            });
        }
        let reachable = reachable_start(self);
        for end in &endings {
            if !reachable.contains(end.id.as_str()) {
                return Err(DialogueError::Quest {
                    token: end.id.as_str().to_owned(),
                    reason: "unreachable ending".into(),
                });
            }
            if mutex_prereqs(self, end) {
                return Err(DialogueError::Quest {
                    token: end.id.as_str().to_owned(),
                    reason: "ending requires mutually exclusive quests".into(),
                });
            }
        }
        Ok(())
    }

    /// Lookup.
    #[must_use]
    pub fn get(&self, id: &Name) -> Option<&QuestNode> {
        self.quests.iter().find(|q| &q.id == id)
    }
}

fn unique_ids(quests: &[QuestNode]) -> Result<(), DialogueError> {
    let mut seen = BTreeSet::new();
    for q in quests {
        if !seen.insert(q.id.as_str()) {
            return Err(DialogueError::Quest {
                token: q.id.as_str().to_owned(),
                reason: "duplicate quest id".into(),
            });
        }
    }
    Ok(())
}

fn index(graph: &QuestGraph) -> BTreeMap<&str, Vec<&str>> {
    let mut m: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for q in &graph.quests {
        m.entry(q.id.as_str()).or_default();
        for p in &q.prerequisites {
            if graph.quests.iter().any(|n| n.id.as_str() == p.as_str()) {
                m.entry(q.id.as_str()).or_default().push(p.as_str());
            }
        }
    }
    m
}

fn cycle_from(start: &str, idx: &BTreeMap<&str, Vec<&str>>) -> Option<Vec<String>> {
    fn dfs<'a>(
        cur: &'a str,
        start: &str,
        idx: &BTreeMap<&str, Vec<&'a str>>,
        stack: &mut Vec<&'a str>,
        seen: &mut BTreeSet<&'a str>,
    ) -> Option<Vec<String>> {
        if !seen.insert(cur) {
            return None;
        }
        stack.push(cur);
        if let Some(nexts) = idx.get(cur) {
            for n in nexts {
                if *n == start && stack.len() > 1 {
                    let mut cyc: Vec<String> = stack.iter().map(|s| (*s).to_owned()).collect();
                    cyc.push((*n).to_owned());
                    return Some(cyc);
                }
                if let Some(c) = dfs(n, start, idx, stack, seen) {
                    return Some(c);
                }
            }
        }
        stack.pop();
        None
    }
    let mut stack = Vec::new();
    let mut seen = BTreeSet::new();
    dfs(start, start, idx, &mut stack, &mut seen)
}

fn reachable_start(graph: &QuestGraph) -> BTreeSet<String> {
    let mut done: BTreeSet<String> = BTreeSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for q in &graph.quests {
            if done.contains(q.id.as_str()) {
                continue;
            }
            let ok = q.available_at_start
                || q.prerequisites.iter().all(|p| {
                    done.contains(p.as_str())
                        || graph.quests.iter().all(|n| n.id.as_str() != p.as_str())
                });
            if ok {
                done.insert(q.id.as_str().to_owned());
                changed = true;
            }
        }
    }
    done
}

fn mutex_prereqs(graph: &QuestGraph, end: &QuestNode) -> bool {
    for ex in &graph.exclusions {
        let a = end.prerequisites.iter().any(|p| p == &ex.a);
        let b = end.prerequisites.iter().any(|p| p == &ex.b);
        if a && b {
            return true;
        }
    }
    false
}
