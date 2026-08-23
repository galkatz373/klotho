//! Rite CFG cook checks (PR 04a).

use std::collections::{BTreeMap, BTreeSet};

use klotho_ir::{RiteGraph, RiteNode, RiteOp};

use crate::error::CookError;

/// Assign pcs, check jumps, reachability, and acyclicity.
pub fn check_rite_cfg(graph: &RiteGraph) -> Result<BTreeMap<u16, RiteOp>, CookError> {
    let (order, ops) = assign_pcs(&graph.nodes)?;
    if !ops.contains_key(&graph.entry) {
        return Err(CookError::MissingEntry(graph.entry));
    }
    let mut succ: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for &pc in &order {
        let op = &ops[&pc];
        let next = next_in_order(&order, pc);
        let edges = successors(pc, op, next, &ops)?;
        succ.insert(pc, edges);
    }
    let reachable = bfs(graph.entry, &succ);
    for &pc in &order {
        if !reachable.contains(&pc) {
            return Err(CookError::Unreachable(pc));
        }
    }
    if has_cycle(&order, &succ) {
        return Err(CookError::Cycle);
    }
    Ok(ops)
}

fn assign_pcs(nodes: &[RiteNode]) -> Result<(Vec<u16>, BTreeMap<u16, RiteOp>), CookError> {
    let any_labeled = nodes.iter().any(|n| matches!(n, RiteNode::Labeled { .. }));
    let any_op = nodes.iter().any(|n| matches!(n, RiteNode::Op(_)));
    if any_labeled && any_op {
        return Err(CookError::MixedLabeling);
    }
    let mut order = Vec::with_capacity(nodes.len());
    let mut ops = BTreeMap::new();
    if any_labeled {
        for n in nodes {
            let RiteNode::Labeled { pc, op } = n else {
                unreachable!();
            };
            if ops.insert(*pc, op.clone()).is_some() {
                return Err(CookError::DuplicatePc(*pc));
            }
            order.push(*pc);
        }
    } else {
        for (i, n) in nodes.iter().enumerate() {
            let RiteNode::Op(op) = n else {
                unreachable!();
            };
            let pc = i as u16;
            ops.insert(pc, op.clone());
            order.push(pc);
        }
    }
    Ok((order, ops))
}

fn next_in_order(order: &[u16], pc: u16) -> Option<u16> {
    let i = order.iter().position(|&p| p == pc)?;
    order.get(i + 1).copied()
}

fn successors(
    pc: u16,
    op: &RiteOp,
    next: Option<u16>,
    ops: &BTreeMap<u16, RiteOp>,
) -> Result<Vec<u16>, CookError> {
    let require = |to: u16| {
        if ops.contains_key(&to) {
            Ok(to)
        } else {
            Err(CookError::MissingTarget { from: pc, to })
        }
    };
    match op {
        RiteOp::Halt(_) | RiteOp::Complete(_) => Ok(Vec::new()),
        RiteOp::Guard(_, fail) | RiteOp::Spend(_, _, fail) => {
            let fail = require(*fail)?;
            let next = next.ok_or(CookError::FallOff(pc))?;
            Ok(vec![next, fail])
        }
        RiteOp::Branch(_, yes, no) => Ok(vec![require(*yes)?, require(*no)?]),
        RiteOp::Wait(_, _)
        | RiteOp::Emit(_)
        | RiteOp::Bind(_)
        | RiteOp::Setq(_, _, _)
        | RiteOp::RelAdd(_, _, _)
        | RiteOp::RelDel(_, _, _)
        | RiteOp::Awake(_) => {
            let next = next.ok_or(CookError::FallOff(pc))?;
            Ok(vec![next])
        }
    }
}

fn bfs(entry: u16, succ: &BTreeMap<u16, Vec<u16>>) -> BTreeSet<u16> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![entry];
    while let Some(pc) = stack.pop() {
        if !seen.insert(pc) {
            continue;
        }
        if let Some(edges) = succ.get(&pc) {
            stack.extend(edges.iter().copied());
        }
    }
    seen
}

fn has_cycle(order: &[u16], succ: &BTreeMap<u16, Vec<u16>>) -> bool {
    #[derive(Copy, Clone, Eq, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color = BTreeMap::new();
    for &pc in order {
        color.insert(pc, Color::White);
    }
    fn visit(pc: u16, color: &mut BTreeMap<u16, Color>, succ: &BTreeMap<u16, Vec<u16>>) -> bool {
        color.insert(pc, Color::Gray);
        if let Some(edges) = succ.get(&pc) {
            for &q in edges {
                match color.get(&q).copied().unwrap_or(Color::White) {
                    Color::Gray => return true,
                    Color::White => {
                        if visit(q, color, succ) {
                            return true;
                        }
                    }
                    Color::Black => {}
                }
            }
        }
        color.insert(pc, Color::Black);
        false
    }
    for &pc in order {
        if color[&pc] == Color::White && visit(pc, &mut color, succ) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use klotho_ir::{RiteGraph, from_ron};

    use super::*;
    use crate::CookError;

    fn graph(src: &str) -> RiteGraph {
        from_ron(src).unwrap()
    }

    #[test]
    fn mixed_labeling_fails() {
        let g = graph(
            r#"
RiteGraph(id: "x", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
    Bind(Target),
    Labeled(pc: 1, op: Complete(Success)),
])
"#,
        );
        assert_eq!(check_rite_cfg(&g), Err(CookError::MixedLabeling));
    }

    #[test]
    fn missing_entry_fails() {
        let g = graph(
            r#"
RiteGraph(id: "x", cap_steps: 8, cap_ticks: 8, entry: 3, nodes: [
    { pc: 0, op: Complete(Success) },
])
"#,
        );
        assert_eq!(check_rite_cfg(&g), Err(CookError::MissingEntry(3)));
    }

    #[test]
    fn backward_branch_is_a_cycle() {
        let g = graph(
            r#"
RiteGraph(id: "x", cap_steps: 8, cap_ticks: 8, entry: 0, nodes: [
    { pc: 0, op: Branch(EqVerb(Use), 1, 0) },
    { pc: 1, op: Complete(Success) },
])
"#,
        );
        assert_eq!(check_rite_cfg(&g), Err(CookError::Cycle));
    }
}
