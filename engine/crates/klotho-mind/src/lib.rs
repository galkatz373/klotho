//! Compiled GOAP and Far-policy proposer (K22, K89, K90).
//!
//! Programs are normalized authoring data, hash-interned at bind time, and read
//! only visible Projection facts. Planning scratch is bounded and disposable.
//! Mind emits [`MindIntent`] proposals and never commits or stores tick state.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use klotho_commit::{AdmitBuf, IslandProposer, Proposal, SyncProposer};
use klotho_core::{LOD_PERIOD, LocusKind, Mm, ResourceId, Sigil, SimLod, Tick};
use klotho_ir::{
    FarRule, IntentTarget, IrError, MindIntent, MindProgram, MindQuery, MindRef, MindSpec,
    MindTarget, Name, Rel, Verb,
};
use klotho_world::WorldView;

/// Maximum search depth per Full planning call (K90).
pub const MAX_SEARCH_DEPTH: u8 = 8;
/// Maximum expanded nodes per Full planning call (K90).
pub const MAX_EXPANDED_NODES: u16 = 128;
/// Declared Full scratch ceiling. The fixed node queue is conservatively below it.
pub const MAX_FULL_SCRATCH_BYTES: usize = 64 * 1024;
/// Declared Far scratch ceiling; table evaluation allocates no actor-sized state.
pub const MAX_FAR_SCRATCH_BYTES: usize = 1024;

/// Stable content id for one normalized program.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct MindProgramId(pub [u8; 32]);

/// A bounded runtime planning failure. It produces no proposal.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum MindDiagnostic {
    /// A referenced pin or resource was not present in cooked Canon.
    Unbound {
        /// Actor whose program failed.
        locus: Sigil,
        /// Missing authoring name.
        name: Name,
    },
    /// Full GOAP exhausted K90's node cap.
    PlannerCap {
        /// Actor whose call exhausted the cap.
        locus: Sigil,
        /// Stable program id.
        program: MindProgramId,
        /// Nodes expanded before stopping.
        expanded: u16,
    },
}

/// Result of a stateless planning pass.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct PlanReport {
    /// Proposals selected in actor order.
    pub intents: Vec<MindIntent>,
    /// Structured failures in actor order.
    pub diagnostics: Vec<MindDiagnostic>,
    /// Total expanded Full nodes, for p50/p95/p99 measurement.
    pub expanded_nodes: u32,
}

/// Compile/residency metrics for the Chorus scale gate.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct MindMetrics {
    /// Number of distinct content hashes.
    pub unique_programs: usize,
    /// Conservative bytes held by compiled tables.
    pub resident_bytes: usize,
    /// Canonical source bytes across unique programs.
    pub compiled_source_bytes: usize,
    /// Non-authoritative wall-clock compile/bind telemetry.
    pub compile_us: u64,
}

/// Mind proposer. Holds immutable, cooked tables only (cache of F).
#[derive(Clone, Debug)]
pub struct Mind {
    agents: Vec<BoundAgent>,
    metrics: MindMetrics,
    resolution: Resolution,
}

#[derive(Clone, Debug)]
struct BoundAgent {
    locus: Sigil,
    program: Arc<CompiledProgram>,
    templates: Vec<String>,
}

#[derive(Clone, Debug)]
struct CompiledProgram {
    id: MindProgramId,
    facts: Vec<CompiledFact>,
    operators: Vec<CompiledOperator>,
    goals: Vec<CompiledGoal>,
    far: Vec<CompiledFar>,
    resident_bytes: usize,
    source_bytes: usize,
}

#[derive(Clone, Debug)]
struct CompiledFact {
    query: MindQuery,
}

#[derive(Clone, Debug)]
struct CompiledOperator {
    requires: u64,
    sets: u64,
    clears: u64,
    cost: u16,
    verb: Verb,
    target: MindTarget,
}

#[derive(Clone, Debug)]
struct CompiledGoal {
    desired: u64,
    utility: u16,
}

#[derive(Clone, Debug)]
struct CompiledFar {
    requires: u64,
    verb: Verb,
    target: MindTarget,
    utility: u16,
}

impl Mind {
    /// Empty proposer. No agents act.
    #[must_use]
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            metrics: MindMetrics {
                unique_programs: 0,
                resident_bytes: 0,
                compiled_source_bytes: 0,
                compile_us: 0,
            },
            resolution: Resolution::default(),
        }
    }

    /// Bind programs through pin and resource lookups and hash-intern duplicates.
    /// Missing actor pins and non-Actor loci are omitted; unresolved program
    /// operands are reported at planning time and emit no intent.
    #[must_use]
    pub fn bind_with(
        specs: impl IntoIterator<Item = MindSpec>,
        pin: impl FnMut(&str) -> Option<Sigil>,
        resource: impl FnMut(&str) -> Option<ResourceId>,
    ) -> Self {
        Self::try_bind_with(specs, pin, resource).unwrap_or_default()
    }

    /// Checked binder. Invalid programs fail closed before any table is built.
    pub fn try_bind_with(
        specs: impl IntoIterator<Item = MindSpec>,
        mut pin: impl FnMut(&str) -> Option<Sigil>,
        mut resource: impl FnMut(&str) -> Option<ResourceId>,
    ) -> Result<Self, Vec<IrError>> {
        let compile_started = std::time::Instant::now();
        let specs: Vec<MindSpec> = specs.into_iter().collect();
        let errors: Vec<IrError> = specs
            .iter()
            .filter_map(|spec| spec.validate().err())
            .collect();
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut pins = BTreeMap::new();
        let mut resources = BTreeMap::new();
        for spec in &specs {
            collect_names(
                &spec.program,
                &mut pins,
                &mut resources,
                &mut pin,
                &mut resource,
            );
        }
        let mut interned: BTreeMap<MindProgramId, Arc<CompiledProgram>> = BTreeMap::new();
        let mut agents = Vec::new();
        for spec in specs {
            let Some(locus) = pin(spec.locus.as_str()) else {
                continue;
            };
            if locus.kind() != Some(LocusKind::Actor) {
                continue;
            }
            pins.insert(spec.locus.clone(), locus);
            let id = program_id(&spec.program);
            let program = interned
                .entry(id)
                .or_insert_with(|| Arc::new(compile_program(id, &spec.program)))
                .clone();
            agents.push(BoundAgent {
                locus,
                program,
                templates: spec.templates,
            });
        }
        agents.sort_by_key(|a| a.locus);
        let metrics = MindMetrics {
            unique_programs: interned.len(),
            resident_bytes: interned.values().map(|p| p.resident_bytes).sum(),
            compiled_source_bytes: interned.values().map(|p| p.source_bytes).sum(),
            compile_us: compile_started.elapsed().as_micros() as u64,
        };
        Ok(Self {
            agents,
            metrics,
            resolution: Resolution { pins, resources },
        })
    }

    /// Compatibility binder for the original Hearth heat-only surface.
    #[must_use]
    pub fn bind(
        specs: impl IntoIterator<Item = MindSpec>,
        pin: impl FnMut(&str) -> Option<Sigil>,
        heat: Option<ResourceId>,
    ) -> Self {
        Self::bind_with(specs, pin, |name| {
            (name == "heat").then_some(heat).flatten()
        })
    }

    /// Program interning and resident table metrics.
    #[must_use]
    pub const fn metrics(&self) -> MindMetrics {
        self.metrics
    }

    /// Stable program id for an actor.
    #[must_use]
    pub fn program_id(&self, locus: Sigil) -> Option<MindProgramId> {
        self.agents
            .iter()
            .find(|a| a.locus == locus)
            .map(|a| a.program.id)
    }

    /// First canned template for `locus`, if the spec listed any.
    #[must_use]
    pub fn canned_line(&self, locus: Sigil) -> Option<&str> {
        self.agents
            .iter()
            .find(|a| a.locus == locus)?
            .templates
            .first()
            .map(String::as_str)
    }

    /// Plan from the view, returning structured cap/binding diagnostics.
    #[must_use]
    pub fn plan_report(&self, view: &WorldView<'_>) -> PlanReport {
        let mut report = PlanReport::default();
        for agent in &self.agents {
            if inactive(agent.locus, view) {
                continue;
            }
            let outcome = match view.sim_lod(agent.locus) {
                SimLod::Dormant => continue,
                SimLod::Far => plan_far(agent, view, &self.resolution),
                SimLod::Full => plan_full(agent, view, &self.resolution),
            };
            report.expanded_nodes += u32::from(outcome.expanded);
            if let Some(diagnostic) = outcome.diagnostic {
                report.diagnostics.push(diagnostic);
            } else if let Some(intent) = outcome.intent {
                report.intents.push(intent);
            }
        }
        report
    }

    /// Plan this tick. Scratch is discarded before returning.
    #[must_use]
    pub fn plan(&self, view: &WorldView<'_>) -> Vec<MindIntent> {
        self.plan_report(view).intents
    }
}

impl PartialEq for Mind {
    fn eq(&self, other: &Self) -> bool {
        self.agents
            .iter()
            .map(|a| (a.locus, a.program.id, &a.templates))
            .eq(other
                .agents
                .iter()
                .map(|a| (a.locus, a.program.id, &a.templates)))
            && self.resolution == other.resolution
    }
}

impl Eq for Mind {}

impl Default for Mind {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncProposer for Mind {
    fn name(&self) -> &'static str {
        "mind"
    }

    fn propose(&mut self, view: &WorldView, _dt: Tick, out: &mut AdmitBuf) {
        for intent in self.plan(view) {
            out.push(Proposal::Mind(intent));
        }
    }
}

impl IslandProposer for Mind {
    fn name(&self) -> &'static str {
        "mind"
    }

    fn propose_island(&self, island: u16, view: &WorldView, out: &mut AdmitBuf) {
        for intent in self.plan(view) {
            if view.island(intent.locus).map(|(id, _)| id) == Some(island) {
                out.push(Proposal::Mind(intent));
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Resolution {
    pins: BTreeMap<Name, Sigil>,
    resources: BTreeMap<Name, ResourceId>,
}

fn collect_names(
    program: &MindProgram,
    pins: &mut BTreeMap<Name, Sigil>,
    resources: &mut BTreeMap<Name, ResourceId>,
    pin: &mut impl FnMut(&str) -> Option<Sigil>,
    resource: &mut impl FnMut(&str) -> Option<ResourceId>,
) {
    for fact in &program.facts {
        match &fact.query {
            MindQuery::Related { a, b, .. } | MindQuery::Near { a, b, .. } => {
                collect_ref(a, pins, pin);
                collect_ref(b, pins, pin);
            }
            MindQuery::QtyAtLeast { of, res, .. } => {
                collect_ref(of, pins, pin);
                if let Some(id) = resource(res.as_str()) {
                    resources.insert(res.clone(), id);
                }
            }
            MindQuery::AnyQtyAtLeast { res, .. } => {
                if let Some(id) = resource(res.as_str()) {
                    resources.insert(res.clone(), id);
                }
            }
            MindQuery::Never | MindQuery::Always | MindQuery::TickModulo { .. } => {}
        }
    }
    for target in program
        .operators
        .iter()
        .map(|op| &op.target)
        .chain(program.far.iter().map(|row| &row.target))
    {
        if let MindTarget::Ref(r) = target {
            collect_ref(r, pins, pin);
        }
    }
}

fn collect_ref(
    r: &MindRef,
    pins: &mut BTreeMap<Name, Sigil>,
    pin: &mut impl FnMut(&str) -> Option<Sigil>,
) {
    if let MindRef::Pin(name) = r {
        if let Some(sigil) = pin(name.as_str()) {
            pins.insert(name.clone(), sigil);
        }
    }
}

fn program_id(program: &MindProgram) -> MindProgramId {
    let bytes = ron::ser::to_string(program).expect("MindProgram serialization is infallible");
    MindProgramId(*blake3::hash(bytes.as_bytes()).as_bytes())
}

fn compile_program(id: MindProgramId, program: &MindProgram) -> CompiledProgram {
    let source_bytes = ron::ser::to_string(program)
        .expect("MindProgram serialization is infallible")
        .len();
    let ix: BTreeMap<_, _> = program
        .facts
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();
    let mask = |names: &[Name]| names.iter().fold(0, |bits, n| bits | (1u64 << ix[n]));
    let operators = program
        .operators
        .iter()
        .map(|op| CompiledOperator {
            requires: mask(&op.requires),
            sets: mask(&op.sets),
            clears: mask(&op.clears),
            cost: op.cost,
            verb: op.verb,
            target: op.target.clone(),
        })
        .collect();
    let goals = program
        .goals
        .iter()
        .map(|goal| CompiledGoal {
            desired: mask(&goal.desired),
            utility: goal.utility,
        })
        .collect();
    let far = program
        .far
        .iter()
        .map(|row| compile_far(row, &mask))
        .collect();
    let resident_bytes = std::mem::size_of::<CompiledProgram>()
        + program.facts.len() * std::mem::size_of::<CompiledFact>()
        + program.operators.len() * std::mem::size_of::<CompiledOperator>()
        + program.goals.len() * std::mem::size_of::<CompiledGoal>()
        + program.far.len() * std::mem::size_of::<CompiledFar>();
    CompiledProgram {
        id,
        facts: program
            .facts
            .iter()
            .map(|f| CompiledFact {
                query: f.query.clone(),
            })
            .collect(),
        operators,
        goals,
        far,
        resident_bytes,
        source_bytes,
    }
}

fn compile_far(row: &FarRule, mask: &impl Fn(&[Name]) -> u64) -> CompiledFar {
    CompiledFar {
        requires: mask(&row.requires),
        verb: row.verb,
        target: row.target.clone(),
        utility: row.utility,
    }
}

fn inactive(locus: Sigil, view: &WorldView<'_>) -> bool {
    view.first_rite(locus).is_some()
        || view.has_rel(locus, Rel::Dead, locus)
        || (view.sim_lod(locus) == SimLod::Far && view.tick().0 % u64::from(LOD_PERIOD) != 0)
}

#[derive(Default)]
struct Outcome {
    intent: Option<MindIntent>,
    diagnostic: Option<MindDiagnostic>,
    expanded: u16,
}

fn plan_full(agent: &BoundAgent, view: &WorldView<'_>, r: &Resolution) -> Outcome {
    let initial = match fact_mask(agent, view, r) {
        Ok(bits) => bits,
        Err(name) => return unbound(agent.locus, name),
    };
    let mut best: Option<(u16, u16, usize)> = None;
    let mut total_expanded = 0;
    for (goal_ix, goal) in agent.program.goals.iter().enumerate() {
        if initial & goal.desired == goal.desired {
            continue;
        }
        match search(initial, goal.desired, &agent.program.operators) {
            Search::Found {
                first,
                cost,
                expanded,
            } => {
                total_expanded += expanded;
                let candidate = (goal.utility, cost, first);
                if best
                    .is_none_or(|b| candidate.0 > b.0 || (candidate.0 == b.0 && candidate.1 < b.1))
                {
                    best = Some(candidate);
                }
            }
            Search::Cap(expanded) => {
                return Outcome {
                    diagnostic: Some(MindDiagnostic::PlannerCap {
                        locus: agent.locus,
                        program: agent.program.id,
                        expanded,
                    }),
                    expanded,
                    ..Outcome::default()
                };
            }
            Search::None(expanded) => total_expanded += expanded,
        }
        let _ = goal_ix;
    }
    let Some((utility, _, op_ix)) = best else {
        return Outcome {
            expanded: total_expanded,
            ..Outcome::default()
        };
    };
    let op = &agent.program.operators[op_ix];
    match resolve_target(&op.target, agent.locus, view, r) {
        Ok(target) => Outcome {
            intent: Some(MindIntent {
                locus: agent.locus,
                verb: op.verb,
                target,
                utility,
            }),
            expanded: total_expanded,
            ..Outcome::default()
        },
        Err(name) => unbound(agent.locus, name),
    }
}

fn plan_far(agent: &BoundAgent, view: &WorldView<'_>, r: &Resolution) -> Outcome {
    let facts = match fact_mask(agent, view, r) {
        Ok(bits) => bits,
        Err(name) => return unbound(agent.locus, name),
    };
    let row = agent
        .program
        .far
        .iter()
        .enumerate()
        .filter(|(_, row)| facts & row.requires == row.requires)
        .max_by_key(|(i, row)| (row.utility, std::cmp::Reverse(*i)));
    let Some((_, row)) = row else {
        return Outcome::default();
    };
    match resolve_target(&row.target, agent.locus, view, r) {
        Ok(target) => Outcome {
            intent: Some(MindIntent {
                locus: agent.locus,
                verb: row.verb,
                target,
                utility: row.utility,
            }),
            ..Outcome::default()
        },
        Err(name) => unbound(agent.locus, name),
    }
}

fn unbound(locus: Sigil, name: Name) -> Outcome {
    Outcome {
        diagnostic: Some(MindDiagnostic::Unbound { locus, name }),
        ..Outcome::default()
    }
}

fn fact_mask(agent: &BoundAgent, view: &WorldView<'_>, r: &Resolution) -> Result<u64, Name> {
    let mut bits = 0;
    for (i, fact) in agent.program.facts.iter().enumerate() {
        if eval_query(&fact.query, agent.locus, view, r)? {
            bits |= 1u64 << i;
        }
    }
    Ok(bits)
}

fn eval_query(
    query: &MindQuery,
    actor: Sigil,
    view: &WorldView<'_>,
    r: &Resolution,
) -> Result<bool, Name> {
    Ok(match query {
        MindQuery::Never => false,
        MindQuery::Always => true,
        MindQuery::Related { a, rel, b } => view.has_rel(
            resolve_ref(a, actor, view, r)?,
            *rel,
            resolve_ref(b, actor, view, r)?,
        ),
        MindQuery::QtyAtLeast { of, res, min } => {
            let id = r.resources.get(res).copied().ok_or_else(|| res.clone())?;
            view.qty(resolve_ref(of, actor, view, r)?, id) >= *min
        }
        MindQuery::AnyQtyAtLeast { res, min } => {
            let id = r.resources.get(res).copied().ok_or_else(|| res.clone())?;
            view.loci().any(|s| view.qty(s, id) >= *min)
        }
        MindQuery::Near { a, b, within } => {
            let Some(pa) = view.pose(resolve_ref(a, actor, view, r)?) else {
                return Ok(false);
            };
            let Some(pb) = view.pose(resolve_ref(b, actor, view, r)?) else {
                return Ok(false);
            };
            xz_chebyshev(pa.x, pa.z, pb.x, pb.z) <= within.0
        }
        MindQuery::TickModulo { period, phase } => {
            view.tick().0 % u64::from(*period) == u64::from(*phase)
        }
    })
}

fn resolve_ref(
    reference: &MindRef,
    actor: Sigil,
    view: &WorldView<'_>,
    r: &Resolution,
) -> Result<Sigil, Name> {
    match reference {
        MindRef::This => Ok(actor),
        MindRef::Pin(name) => r.pins.get(name).copied().ok_or_else(|| name.clone()),
        MindRef::Related(rel) => view
            .related(actor, *rel)
            .next()
            .ok_or_else(|| Name::from(rel.as_str())),
    }
}

fn resolve_target(
    target: &MindTarget,
    actor: Sigil,
    view: &WorldView<'_>,
    r: &Resolution,
) -> Result<IntentTarget, Name> {
    match target {
        MindTarget::None => Ok(IntentTarget::None),
        MindTarget::Ref(reference) => {
            resolve_ref(reference, actor, view, r).map(IntentTarget::Sigil)
        }
    }
}

fn xz_chebyshev(ax: Mm, az: Mm, bx: Mm, bz: Mm) -> i32 {
    ax.wrapping_sub(bx)
        .0
        .unsigned_abs()
        .max(az.wrapping_sub(bz).0.unsigned_abs()) as i32
}

#[derive(Copy, Clone)]
struct Node {
    state: u64,
    cost: u16,
    depth: u8,
    first: usize,
}

enum Search {
    Found {
        first: usize,
        cost: u16,
        expanded: u16,
    },
    Cap(u16),
    None(u16),
}

fn search(initial: u64, desired: u64, operators: &[CompiledOperator]) -> Search {
    let mut queue = VecDeque::with_capacity(usize::from(MAX_EXPANDED_NODES));
    let mut visited = BTreeMap::new();
    for (i, op) in operators.iter().enumerate() {
        if initial & op.requires == op.requires {
            queue.push_back(apply(initial, op, i, 1));
        }
    }
    let mut expanded = 0;
    while let Some(node) = pop_cheapest(&mut queue) {
        if expanded == MAX_EXPANDED_NODES {
            return Search::Cap(expanded);
        }
        if visited
            .get(&node.state)
            .is_some_and(|cost| *cost <= node.cost)
        {
            continue;
        }
        visited.insert(node.state, node.cost);
        expanded += 1;
        if node.state & desired == desired {
            return Search::Found {
                first: node.first,
                cost: node.cost,
                expanded,
            };
        }
        if node.depth >= MAX_SEARCH_DEPTH {
            continue;
        }
        for op in operators {
            if node.state & op.requires == op.requires {
                if queue.len() == usize::from(MAX_EXPANDED_NODES) {
                    return Search::Cap(expanded);
                }
                queue.push_back(Node {
                    state: (node.state & !op.clears) | op.sets,
                    cost: node.cost.saturating_add(op.cost),
                    depth: node.depth + 1,
                    first: node.first,
                });
            }
        }
    }
    Search::None(expanded)
}

fn apply(state: u64, op: &CompiledOperator, first: usize, depth: u8) -> Node {
    Node {
        state: (state & !op.clears) | op.sets,
        cost: op.cost,
        depth,
        first,
    }
}

fn pop_cheapest(queue: &mut VecDeque<Node>) -> Option<Node> {
    let best = queue
        .iter()
        .enumerate()
        .min_by_key(|(i, node)| (node.cost, node.depth, *i))?
        .0;
    queue.remove(best)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::CommitKernel;
    use klotho_core::{Budget, Hash};
    use klotho_ir::{CanonDiff, MindFact, MindGoal, MindOperator, from_ron};
    use klotho_world::World;

    use super::*;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn action_spec(locus: &str, goal: &str, verb: Verb, utility: u16) -> MindSpec {
        let ready = Name::from("ready");
        let done = Name::from("done");
        MindSpec {
            locus: Name::from(locus),
            program: MindProgram {
                beat: Some(Name::from(goal)),
                facts: vec![
                    MindFact {
                        id: ready.clone(),
                        query: MindQuery::Always,
                        far_safe: true,
                    },
                    MindFact {
                        id: done.clone(),
                        query: MindQuery::Never,
                        far_safe: true,
                    },
                ],
                operators: vec![MindOperator {
                    id: Name::from("act"),
                    requires: vec![ready.clone()],
                    sets: vec![done.clone()],
                    clears: vec![],
                    cost: 1,
                    verb,
                    target: MindTarget::None,
                }],
                goals: vec![MindGoal {
                    id: Name::from(goal),
                    desired: vec![done.clone()],
                    utility,
                }],
                far: vec![FarRule {
                    requires: vec![ready],
                    effects: vec![done],
                    verb,
                    target: MindTarget::None,
                    utility,
                }],
            },
            templates: Vec::new(),
        }
    }

    fn kernel() -> CommitKernel {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        CommitKernel::new(World::new(
            Arc::new(cook_diffs(&diffs).unwrap()),
            Hash::ZERO,
        ))
    }

    #[test]
    fn twenty_authored_goal_labels_need_no_dispatch() {
        let mut k = kernel();
        let mut specs = Vec::new();
        for i in 0..20u128 {
            k.world_mut()
                .insert_locus(actor(i), LocusKind::Actor)
                .unwrap();
            specs.push(action_spec(
                &format!("actor_{i}"),
                &format!("goal_{i}"),
                Verb::Investigate,
                10,
            ));
        }
        let mind = Mind::bind_with(
            specs,
            |name| name.strip_prefix("actor_")?.parse().ok().map(actor),
            |_| None,
        );
        assert_eq!(mind.plan(&k.world().view()).len(), 20);
    }

    #[test]
    fn programs_are_hash_interned_and_stateless() {
        let mut k = kernel();
        k.world_mut()
            .insert_locus(actor(1), LocusKind::Actor)
            .unwrap();
        k.world_mut()
            .insert_locus(actor(2), LocusKind::Actor)
            .unwrap();
        let a = action_spec("a", "observe", Verb::Investigate, 10);
        let mut b = a.clone();
        b.locus = Name::from("b");
        let mut mind = Mind::bind_with(
            vec![a, b],
            |n| match n {
                "a" => Some(actor(1)),
                "b" => Some(actor(2)),
                _ => None,
            },
            |_| None,
        );
        assert_eq!(mind.metrics().unique_programs, 1);
        let before = mind.clone();
        let mut out = AdmitBuf::new();
        mind.propose(&k.world().view(), Tick(1), &mut out);
        assert_eq!(mind, before);
    }

    #[test]
    fn far_uses_table_and_worker_paths_are_equal() {
        let mut k = kernel();
        k.world_mut()
            .insert_locus(actor(1), LocusKind::Actor)
            .unwrap();
        k.world_mut().set_sim_lod(actor(1), SimLod::Far).unwrap();
        let mind = Mind::bind_with(
            vec![action_spec("a", "far", Verb::Move, 7)],
            |n| (n == "a").then(|| actor(1)),
            |_| None,
        );
        let report = mind.plan_report(&k.world().view());
        assert_eq!(report.expanded_nodes, 0);
        assert_eq!(report.intents[0].verb, Verb::Move);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn proposals_never_claim_time_agency() {
        let mut k = kernel();
        k.world_mut()
            .insert_locus(actor(1), LocusKind::Actor)
            .unwrap();
        let mut mind = Mind::bind_with(
            vec![action_spec("a", "act", Verb::Investigate, 10)],
            |n| (n == "a").then(|| actor(1)),
            |_| None,
        );
        let delta = k.step(Tick(1), Budget::HEARTH, &mut [&mut mind]).unwrap();
        assert!(
            delta
                .rejects
                .iter()
                .all(|(_, r)| *r != klotho_core::RejectReason::UnclaimedAgency)
        );
    }

    #[test]
    fn chorus_scale_is_interned_and_bounded() {
        let canon = Arc::new(cook_diffs(&[]).unwrap());
        let mut k = CommitKernel::new(World::with_locus_cap(canon, Hash::ZERO, 2_300));
        let template = action_spec("template", "crowd_beat", Verb::Investigate, 1);
        let mut specs = Vec::with_capacity(2_200);
        for i in 0..2_200u128 {
            k.world_mut()
                .insert_locus(actor(i), LocusKind::Actor)
                .unwrap();
            k.world_mut()
                .set_sim_lod(actor(i), if i < 200 { SimLod::Full } else { SimLod::Far })
                .unwrap();
            let mut spec = template.clone();
            spec.locus = Name(format!("crowd_{i}"));
            specs.push(spec);
        }
        let mind = Mind::bind_with(
            specs,
            |name| name.strip_prefix("crowd_")?.parse().ok().map(actor),
            |_| None,
        );
        let report = mind.plan_report(&k.world().view());
        assert_eq!(mind.metrics().unique_programs, 1);
        assert!(mind.metrics().resident_bytes < MAX_FULL_SCRATCH_BYTES);
        assert_eq!(report.intents.len(), 2_200);
        assert_eq!(report.expanded_nodes, 200);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn eight_worker_partition_matches_serial_proposals() {
        let mut k = kernel();
        let mut specs = Vec::new();
        for i in 0..8u128 {
            k.world_mut()
                .insert_locus(actor(i), LocusKind::Actor)
                .unwrap();
            k.world_mut().set_island(actor(i), i as u16, 0).unwrap();
            specs.push(action_spec(
                &format!("actor_{i}"),
                "coordinate",
                Verb::Investigate,
                1,
            ));
        }
        let mut mind = Mind::bind_with(
            specs,
            |name| name.strip_prefix("actor_")?.parse().ok().map(actor),
            |_| None,
        );
        let mut serial = AdmitBuf::new();
        SyncProposer::propose(&mut mind, &k.world().view(), Tick(1), &mut serial);
        let mut partitioned = AdmitBuf::new();
        for island in 0..8 {
            IslandProposer::propose_island(&mind, island, &k.world().view(), &mut partitioned);
        }
        assert_eq!(serial.drain(), partitioned.drain());
    }
}
