//! Distaff writers' room: narrative-order review, not file order.

use std::fmt;

use klotho_dialogue::NarrativeProject;
use klotho_ir::Name;

/// One line as shown to writers.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LineRow {
    /// Localization key.
    pub key: Name,
    /// Speaker.
    pub speaker: Name,
    /// Timeline beat.
    pub beat: Name,
    /// Whether the line has approved VO.
    pub vo: bool,
}

/// One quest as shown to writers.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct QuestRow {
    /// Quest id.
    pub id: Name,
    /// Critical-path membership.
    pub critical: bool,
    /// Ending node.
    pub ending: bool,
}

/// Headless writers' room.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WriterRoomView {
    /// Bible version under review.
    pub bible_version: u32,
    /// Quests in id order with critical first.
    pub quests: Vec<QuestRow>,
    /// Lines in narrative order.
    pub lines: Vec<LineRow>,
    /// Impact edge count.
    pub impact_edges: usize,
    /// Distinct dependent kinds.
    pub impact_kinds: Vec<&'static str>,
}

impl fmt::Display for WriterRoomView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "bible v{}", self.bible_version)?;
        writeln!(f, "quests:")?;
        for q in &self.quests {
            let flag = if q.critical { "critical" } else { "side" };
            writeln!(f, "  {} {flag}", q.id)?;
        }
        writeln!(f, "lines:")?;
        for l in &self.lines {
            writeln!(f, "  {} ({})", l.key, l.speaker)?;
        }
        Ok(())
    }
}

/// Build the writers' room view. Line order is bible beat, speaker, key.
#[must_use]
pub fn review_narrative(project: &NarrativeProject) -> WriterRoomView {
    let order = project.narrative_lines();
    let mut lines = Vec::new();
    for key in &order {
        if let Some(line) = project.dialogue.line(key) {
            lines.push(LineRow {
                key: line.key.clone(),
                speaker: line.speaker.clone(),
                beat: line.beat.clone(),
                vo: line.vo.is_some(),
            });
        }
    }
    let mut quests: Vec<QuestRow> = project
        .quests
        .quests
        .iter()
        .map(|q| QuestRow {
            id: q.id.clone(),
            critical: q.critical,
            ending: q.ending,
        })
        .collect();
    quests.sort_by(|a, b| b.critical.cmp(&a.critical).then(a.id.cmp(&b.id)));
    let graph = project.impact();
    let kinds: Vec<&str> = graph
        .edges
        .iter()
        .map(|e| e.kind.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    WriterRoomView {
        bible_version: project.bible.version,
        quests,
        lines,
        impact_edges: graph.edges.len(),
        impact_kinds: kinds,
    }
}
