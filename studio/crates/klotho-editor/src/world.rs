//! Distaff world graph view: Places, traversal, protected anchors, budgets.

use std::fmt;

use klotho_ir::Name;
use klotho_pattern::{
    DressingInstance, PlaceRole, SolvedDressing, TraversalEdge, WorldPlan, solve_budgets,
};

/// One Place node in the graph view.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PlaceNode {
    /// Place name.
    pub name: Name,
    /// Route role.
    pub role: PlaceRole,
    /// Protected anchor count.
    pub protected: usize,
    /// Dressing instances currently assigned.
    pub dressing: u32,
    /// Whether every domain is under cap.
    pub budget_ok: bool,
}

/// One directed traversal edge as shown in Distaff.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct GraphEdge {
    /// Origin.
    pub from: Name,
    /// Destination.
    pub to: Name,
    /// Bidirectional in the plan.
    pub bidirectional: bool,
}

/// Headless Distaff world graph.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WorldGraphView {
    /// Places in seed/plan order.
    pub places: Vec<PlaceNode>,
    /// Traversal edges, directed as stored (bidirectional shown once).
    pub edges: Vec<GraphEdge>,
    /// Critical-path names.
    pub critical_path: Vec<Name>,
}

impl fmt::Display for WorldGraphView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "world:")?;
        for p in &self.places {
            let flag = if p.budget_ok { "ok" } else { "over" };
            writeln!(
                f,
                "  {} ({}) protected={} dressing={} {flag}",
                p.name,
                p.role.as_str(),
                p.protected,
                p.dressing
            )?;
        }
        writeln!(f, "path:")?;
        let path = self
            .critical_path
            .iter()
            .map(|n| n.as_str().to_owned())
            .collect::<Vec<_>>()
            .join(" -> ");
        writeln!(f, "  {path}")?;
        Ok(())
    }
}

/// Build a graph view from a plan and solved dressing.
#[must_use]
pub fn world_graph(plan: &WorldPlan, solved: &SolvedDressing) -> WorldGraphView {
    let mut counts = std::collections::BTreeMap::new();
    for inst in &solved.instances {
        *counts.entry(inst.place.clone()).or_insert(0u32) += 1;
    }
    let places = plan
        .places
        .iter()
        .map(|p| {
            let dressing = counts.get(&p.name).copied().unwrap_or(0);
            PlaceNode {
                name: p.name.clone(),
                role: p.role,
                protected: p.protected.len(),
                dressing,
                budget_ok: dressing <= p.budgets.density,
            }
        })
        .collect();
    let edges = plan
        .edges
        .iter()
        .map(|e: &TraversalEdge| GraphEdge {
            from: e.from.clone(),
            to: e.to.clone(),
            bidirectional: e.bidirectional,
        })
        .collect();
    WorldGraphView {
        places,
        edges,
        critical_path: plan.critical_path.clone(),
    }
}

/// Solve dressing and produce the Distaff graph in one step.
pub fn review_world(
    plan: &WorldPlan,
    proposed: &[DressingInstance],
) -> Result<(SolvedDressing, WorldGraphView), klotho_pattern::PatternError> {
    let solved = solve_budgets(plan, proposed)?;
    let view = world_graph(plan, &solved);
    Ok((solved, view))
}
