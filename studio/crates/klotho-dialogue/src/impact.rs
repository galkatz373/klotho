//! Impact graph: canon/bible edits invalidate dependent evidence.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use klotho_ir::{AnchorId, Name};

use crate::bible::StoryBible;
use crate::dialogue::DialogueModule;
use crate::error::DialogueError;
use crate::loc::LocaleCatalog;
use crate::quest::QuestGraph;

/// Kind of dependent artifact.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactKind {
    /// Story bible fact.
    Bible,
    /// Quest node.
    Quest,
    /// Dialogue line.
    Dialogue,
    /// VO binding.
    Vo,
    /// Localization key / catalog.
    Loc,
    /// Journey covering the artifact.
    Journey,
}

impl ImpactKind {
    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bible => "bible",
            Self::Quest => "quest",
            Self::Dialogue => "dialogue",
            Self::Vo => "vo",
            Self::Loc => "loc",
            Self::Journey => "journey",
        }
    }
}

/// Directed dependency. `from` is upstream canon; `to` is a dependent.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactEdge {
    /// Upstream anchor.
    pub from: AnchorId,
    /// Dependent anchor.
    pub to: AnchorId,
    /// Dependent class.
    pub kind: ImpactKind,
    /// Human token for diagnostics.
    pub token: Name,
}

/// Closed impact graph for a narrative project.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactGraph {
    /// Edges, sorted by (from, to, kind).
    pub edges: Vec<ImpactEdge>,
}

impl ImpactGraph {
    /// Build the graph from bible, quests, dialogue, and locale catalogs.
    #[must_use]
    pub fn build(
        bible: &StoryBible,
        quests: &QuestGraph,
        module: &DialogueModule,
        locales: &[LocaleCatalog],
    ) -> Self {
        let mut edges = Vec::new();
        push(
            &mut edges,
            bible.anchor,
            quests.anchor,
            ImpactKind::Quest,
            &quests_token(quests),
        );
        push(
            &mut edges,
            bible.anchor,
            module.anchor,
            ImpactKind::Dialogue,
            &module.id,
        );
        for ch in &bible.characters {
            for line in &module.lines {
                if line.speaker == ch.name {
                    push(
                        &mut edges,
                        ch.anchor,
                        line.anchor,
                        ImpactKind::Dialogue,
                        &line.key,
                    );
                }
            }
        }
        for secret in &bible.secrets {
            for q in &quests.quests {
                if q.prerequisites.iter().any(|p| p == &secret.knows)
                    || q.grants.iter().any(|g| g == &secret.knows)
                {
                    push(
                        &mut edges,
                        secret.anchor,
                        q.anchor,
                        ImpactKind::Quest,
                        &q.id,
                    );
                }
            }
            for line in &module.lines {
                let reads = line.condition.reads();
                if reads.iter().any(|r| r == &secret.knows)
                    || line.grants.iter().any(|g| g == &secret.knows)
                {
                    push(
                        &mut edges,
                        secret.anchor,
                        line.anchor,
                        ImpactKind::Dialogue,
                        &line.key,
                    );
                }
            }
        }
        for beat in &bible.timeline {
            for line in &module.lines {
                if line.beat == beat.id {
                    push(
                        &mut edges,
                        beat.anchor,
                        line.anchor,
                        ImpactKind::Dialogue,
                        &line.key,
                    );
                }
            }
        }
        for q in &quests.quests {
            for line in &module.lines {
                let reads = line.condition.reads();
                if reads.iter().any(|r| r == &q.id)
                    || q.grants.iter().any(|g| {
                        reads.iter().any(|r| r == g) || line.grants.iter().any(|lg| lg == g)
                    })
                {
                    push(
                        &mut edges,
                        q.anchor,
                        line.anchor,
                        ImpactKind::Dialogue,
                        &line.key,
                    );
                }
            }
        }
        for line in &module.lines {
            if line.vo.is_some() {
                push(
                    &mut edges,
                    line.anchor,
                    line.anchor.child(b"vo"),
                    ImpactKind::Vo,
                    &line.key,
                );
            }
            for loc in locales {
                if loc.strings.contains_key(&line.key) {
                    push(
                        &mut edges,
                        line.anchor,
                        loc_anchor(loc, &line.key),
                        ImpactKind::Loc,
                        &line.key,
                    );
                }
            }
            push(
                &mut edges,
                line.anchor,
                line.anchor.child(b"journey"),
                ImpactKind::Journey,
                &line.key,
            );
        }
        edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then(a.to.cmp(&b.to))
                .then(a.kind.as_str().cmp(b.kind.as_str()))
        });
        Self { edges }
    }

    /// Transitive dependents of `changed`, including the seeds.
    #[must_use]
    pub fn invalidate(&self, changed: &[AnchorId]) -> BTreeSet<AnchorId> {
        let mut adj: BTreeMap<AnchorId, Vec<AnchorId>> = BTreeMap::new();
        for e in &self.edges {
            adj.entry(e.from).or_default().push(e.to);
        }
        let mut out: BTreeSet<AnchorId> = changed.iter().copied().collect();
        let mut stack: Vec<AnchorId> = changed.to_vec();
        while let Some(cur) = stack.pop() {
            if let Some(nexts) = adj.get(&cur) {
                for n in nexts {
                    if out.insert(*n) {
                        stack.push(*n);
                    }
                }
            }
        }
        out
    }

    /// Evidence covering any invalidated dependent is stale.
    pub fn check_evidence(
        &self,
        changed: &[AnchorId],
        covered: &[AnchorId],
    ) -> Result<(), DialogueError> {
        let dirty = self.invalidate(changed);
        for c in covered {
            if dirty.contains(c) {
                let token = self
                    .edges
                    .iter()
                    .find(|e| e.to == *c)
                    .map(|e| e.token.as_str().to_owned())
                    .unwrap_or_else(|| "dependent".into());
                let kind = self
                    .edges
                    .iter()
                    .find(|e| e.to == *c)
                    .map(|e| e.kind.as_str().to_owned())
                    .unwrap_or_else(|| "evidence".into());
                return Err(DialogueError::Stale { kind, token });
            }
        }
        Ok(())
    }
}

fn push(edges: &mut Vec<ImpactEdge>, from: AnchorId, to: AnchorId, kind: ImpactKind, token: &Name) {
    edges.push(ImpactEdge {
        from,
        to,
        kind,
        token: token.clone(),
    });
}

fn quests_token(_quests: &QuestGraph) -> Name {
    Name::from("quests")
}

fn loc_anchor(loc: &LocaleCatalog, key: &Name) -> AnchorId {
    let mut token = Vec::new();
    token.extend_from_slice(loc.locale.as_str().as_bytes());
    token.push(b':');
    token.extend_from_slice(key.as_str().as_bytes());
    AnchorId::derive(b"klotho-loc-v1", &token)
}
