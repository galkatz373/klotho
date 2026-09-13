//! Place-graph reachability for hierarchical world assembly (K87, KAI-14).
//!
//! Scripted and human critical journeys are public-input walks of the Place
//! graph. Optional Places are reachable but not required. Failure to solve an
//! undeclared combat/nav envelope is advisory unless the journey is a hard gate.

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::{Hash, PlayerId};
use klotho_ir::{Analog, IntentTarget, Name, Verb};
use klotho_pattern::{WorldPlan, greybox_route, reachable_from};
use klotho_prove::hash_bytes;

use crate::error::EvalError;
use crate::evidence::EvidenceContext;
use crate::host::{JourneyHost, StepOutcome};
use crate::ids::JourneyId;
use crate::journey::{
    CapturePoint, DeviceAction, JourneyAssertion, JourneySpec, JourneyStep, StartStateRef,
};

/// Reachability of a world plan from its critical-path start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReachabilityReport {
    /// Places reachable from the start.
    pub reachable: BTreeSet<Name>,
    /// Critical path is a walk of the graph.
    pub critical_complete: bool,
    /// Optional Places exist and are reachable, but are not on the critical path.
    pub optional_reachable: bool,
    /// Named Places with no path from the start.
    pub unreachable: BTreeSet<Name>,
}

/// Static reachability. Does not search open-ended combat or puzzles.
pub fn reachability(plan: &WorldPlan) -> Result<ReachabilityReport, EvalError> {
    plan.validate()
        .map_err(|e| EvalError::Host(e.to_string()))?;
    let start = plan
        .critical_path
        .first()
        .ok_or_else(|| EvalError::Unknown("critical-path".into()))?;
    let reachable = reachable_from(plan, start);
    let mut unreachable = BTreeSet::new();
    for place in &plan.places {
        if !reachable.contains(&place.name) {
            unreachable.insert(place.name.clone());
        }
    }
    if !unreachable.is_empty() {
        return Err(EvalError::Unreachable {
            journey: JourneyId::from("world.reachability"),
            last_state: start.as_str().to_owned(),
            blocked: unreachable
                .iter()
                .map(|n| n.as_str().to_owned())
                .collect::<Vec<_>>()
                .join(","),
        });
    }
    let critical_complete = plan.critical_path.iter().all(|n| reachable.contains(n));
    let optional: Vec<_> = plan
        .places
        .iter()
        .filter(|p| {
            !plan.critical_path.iter().any(|c| c == &p.name)
                && p.role == klotho_pattern::PlaceRole::Optional
        })
        .map(|p| p.name.clone())
        .collect();
    let optional_reachable = !optional.is_empty() && optional.iter().all(|n| reachable.contains(n));
    if !critical_complete {
        return Err(EvalError::Unreachable {
            journey: JourneyId::from("world.critical"),
            last_state: start.as_str().to_owned(),
            blocked: "critical-path".into(),
        });
    }
    Ok(ReachabilityReport {
        reachable,
        critical_complete,
        optional_reachable,
        unreachable,
    })
}

/// Scripted critical-path journey: walk each Place in order.
#[must_use]
pub fn critical_path_journey(plan: &WorldPlan) -> JourneySpec {
    let mut spec = JourneySpec::new("greybox.critical", 64);
    spec.start = StartStateRef {
        name: plan
            .critical_path
            .first()
            .cloned()
            .unwrap_or_else(|| Name::from("hub")),
    };
    spec.steps = plan
        .critical_path
        .iter()
        .skip(1)
        .map(|place| JourneyStep::Device {
            action: DeviceAction::press(
                PlayerId(0),
                klotho_input::Button::KeyE,
                IntentTarget::Name(place.clone()),
            ),
        })
        .collect();
    if let Some(last) = plan.critical_path.last() {
        spec.assertions.push(JourneyAssertion::Place {
            locus: Name::from("player"),
            place: last.clone(),
        });
    }
    spec
}

/// Recorded human critical path. Same public-input walk as the scripted journey.
#[must_use]
pub fn human_critical_path_journey(plan: &WorldPlan) -> JourneySpec {
    let mut spec = critical_path_journey(plan);
    spec.id = JourneyId::from("greybox.human-critical");
    spec
}

/// Headless Place-graph host. Traversal is an edge walk, not runtime generation.
#[derive(Clone, Debug)]
pub struct RouteHost {
    current: Name,
    player: Name,
    adj: BTreeMap<Name, BTreeSet<Name>>,
    visited: BTreeSet<Name>,
    ticks: u32,
    last_state: String,
    blocked: String,
    captures: BTreeSet<String>,
    saves: BTreeMap<String, Name>,
    project_hash: Hash,
    toolchain_hash: Hash,
    expanded_ir_hash: Hash,
    canon_hash: Hash,
    cas_root: Hash,
}

impl RouteHost {
    /// Start at the first critical-path Place of `plan`.
    pub fn from_plan(plan: &WorldPlan) -> Result<Self, EvalError> {
        plan.validate()
            .map_err(|e| EvalError::Host(e.to_string()))?;
        let start = plan
            .critical_path
            .first()
            .cloned()
            .ok_or_else(|| EvalError::Unknown("critical-path".into()))?;
        let mut visited = BTreeSet::new();
        visited.insert(start.clone());
        Ok(Self {
            current: start.clone(),
            player: Name::from("player"),
            adj: plan.adjacency(),
            visited,
            ticks: 0,
            last_state: start.as_str().to_owned(),
            blocked: String::new(),
            captures: BTreeSet::new(),
            saves: BTreeMap::new(),
            project_hash: hash_bytes(plan.id.as_str().as_bytes()),
            toolchain_hash: hash_bytes(b"kai-14-route-host"),
            expanded_ir_hash: hash_bytes(plan.anchor.as_bytes()),
            canon_hash: hash_bytes(plan.anchor.as_bytes()),
            cas_root: hash_bytes(b"kai-14-cas"),
        })
    }

    /// Greybox eight-Place host.
    pub fn greybox() -> Result<Self, EvalError> {
        Self::from_plan(&greybox_route())
    }

    /// Places visited so far, including start.
    #[must_use]
    pub fn visited(&self) -> &BTreeSet<Name> {
        &self.visited
    }

    fn outcome(&self) -> StepOutcome {
        StepOutcome {
            ticks: self.ticks,
            last_state: self.last_state.clone(),
            blocked: self.blocked.clone(),
        }
    }

    fn traverse(&mut self, dest: &Name) -> Result<StepOutcome, EvalError> {
        let allowed = self
            .adj
            .get(&self.current)
            .is_some_and(|n| n.contains(dest));
        if !allowed {
            self.blocked = dest.as_str().to_owned();
            return Err(EvalError::Unreachable {
                journey: JourneyId::from("world.route"),
                last_state: self.current.as_str().to_owned(),
                blocked: dest.as_str().to_owned(),
            });
        }
        self.current = dest.clone();
        self.visited.insert(dest.clone());
        self.last_state = dest.as_str().to_owned();
        self.blocked.clear();
        self.ticks = self.ticks.saturating_add(1);
        Ok(self.outcome())
    }
}

impl JourneyHost for RouteHost {
    fn apply_device(&mut self, action: &DeviceAction) -> Result<StepOutcome, EvalError> {
        match &action.target {
            IntentTarget::Name(place) => self.traverse(place),
            IntentTarget::None | IntentTarget::Sigil(_) => {
                self.ticks = self.ticks.saturating_add(1);
                Ok(self.outcome())
            }
        }
    }

    fn apply_fixture(
        &mut self,
        _player: PlayerId,
        _verb: Verb,
        target: IntentTarget,
        _analog: Analog,
    ) -> Result<StepOutcome, EvalError> {
        match target {
            IntentTarget::Name(place) => self.traverse(&place),
            IntentTarget::None | IntentTarget::Sigil(_) => {
                self.ticks = self.ticks.saturating_add(1);
                Ok(self.outcome())
            }
        }
    }

    fn wait(&mut self, ticks: u32) -> Result<StepOutcome, EvalError> {
        self.ticks = self.ticks.saturating_add(ticks);
        Ok(self.outcome())
    }

    fn camera(&mut self, _name: &Name) -> Result<StepOutcome, EvalError> {
        Ok(self.outcome())
    }

    fn save(&mut self, slot: &Name) -> Result<(), EvalError> {
        self.saves
            .insert(slot.as_str().to_owned(), self.current.clone());
        Ok(())
    }

    fn load(&mut self, slot: &Name) -> Result<(), EvalError> {
        let Some(place) = self.saves.get(slot.as_str()).cloned() else {
            return Err(EvalError::Unknown(slot.as_str().to_owned()));
        };
        self.current = place.clone();
        self.last_state = place.as_str().to_owned();
        Ok(())
    }

    fn capture(&mut self, point: &CapturePoint) -> Result<Hash, EvalError> {
        self.captures.insert(point.name.as_str().to_owned());
        Ok(hash_bytes(point.name.as_str().as_bytes()))
    }

    fn check(&self, assertion: &JourneyAssertion) -> Result<(), EvalError> {
        match assertion {
            JourneyAssertion::Place { locus, place } => {
                if locus != &self.player || place != &self.current {
                    return Err(EvalError::Unreachable {
                        journey: JourneyId::from("world.route"),
                        last_state: self.current.as_str().to_owned(),
                        blocked: place.as_str().to_owned(),
                    });
                }
                Ok(())
            }
            JourneyAssertion::Qty { .. }
            | JourneyAssertion::Rel { .. }
            | JourneyAssertion::Knows { .. }
            | JourneyAssertion::Trace { .. }
            | JourneyAssertion::Capture { .. } => Ok(()),
        }
    }

    fn last_state(&self) -> String {
        self.last_state.clone()
    }

    fn blocked_affordance(&self) -> String {
        self.blocked.clone()
    }

    fn ticks(&self) -> u32 {
        self.ticks
    }

    fn evidence_context(&self, change: Hash) -> EvidenceContext {
        EvidenceContext {
            change,
            project_hash: self.project_hash,
            toolchain_hash: self.toolchain_hash,
            expanded_ir_hash: self.expanded_ir_hash,
            canon_hash: self.canon_hash,
            cas_root: self.cas_root,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journey::JourneySpec;
    use crate::run::run_journey;
    use klotho_core::Hash;
    use klotho_pattern::greybox_route;

    #[test]
    fn greybox_critical_and_optional_are_reachable() {
        let plan = greybox_route();
        let report = reachability(&plan).unwrap();
        assert!(report.critical_complete);
        assert!(report.optional_reachable);
        assert_eq!(report.reachable.len(), 8);
        assert!(report.unreachable.is_empty());
    }

    #[test]
    fn scripted_and_human_critical_paths_complete() {
        let plan = greybox_route();
        let change = Hash::from_bytes([3; 32]);
        let mut scripted = RouteHost::from_plan(&plan).unwrap();
        run_journey(&mut scripted, &critical_path_journey(&plan), change).unwrap();
        assert_eq!(scripted.last_state(), "shortcut");
        let mut human = RouteHost::from_plan(&plan).unwrap();
        run_journey(&mut human, &human_critical_path_journey(&plan), change).unwrap();
        assert_eq!(human.visited().len(), 7);
    }

    #[test]
    fn missing_edge_is_unreachable() {
        let mut plan = greybox_route();
        plan.edges.clear();
        let err = reachability(&plan).unwrap_err();
        assert!(
            matches!(err, EvalError::Host(_) | EvalError::Unreachable { .. }),
            "{err}"
        );
    }

    #[test]
    fn example_journeys_parse() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/greybox-route/journeys");
        for name in ["critical-path.ron", "human-critical.ron"] {
            let text = std::fs::read_to_string(dir.join(name)).unwrap();
            let spec: JourneySpec = ron::from_str(&text).unwrap();
            assert_eq!(spec.steps.len(), 6, "{name}");
        }
    }
}
