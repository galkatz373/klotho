//! GOAP mind proposer (K22). Not a behaviour tree.
//!
//! NPCs emit [`MindIntent`] on the same admission path as players, minus
//! Agency. Planner working memory is the view (Knows, rels, poses, heat)
//! plus a per-tick scratch that is wiped. Cooked [`MindSpec`]s and the pin
//! table are a cache of F, not tick state.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;

use klotho_commit::{AdmitBuf, IslandProposer, Proposal, SyncProposer};
use klotho_core::{LOD_PERIOD, LocusKind, PoseMm, ResourceId, Sigil, SimLod, Tick};
use klotho_ir::{IntentTarget, MindIntent, MindSpec, Name, Rel, Verb};
use klotho_world::WorldView;

/// Heat threshold matching Canon `Burning` sugar (`Qty(heat) Ge 400`).
const IGNITE_HEAT: i32 = 400;
/// Stay-near radius for `stay_near_forge`, millimetres.
const STAY_NEAR_MM: i32 = 2_500;

/// GOAP proposer. Holds cooked specs + pins only (cache of F).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mind {
    agents: Vec<BoundAgent>,
    pins: BTreeMap<Name, Sigil>,
    heat: Option<ResourceId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BoundAgent {
    locus: Sigil,
    goals: Vec<Name>,
    templates: Vec<String>,
}

impl Mind {
    /// Empty proposer. No agents act.
    #[must_use]
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            pins: BTreeMap::new(),
            heat: None,
        }
    }

    /// Bind authoring specs through a pin lookup. Missing pins are skipped.
    #[must_use]
    pub fn bind(
        specs: impl IntoIterator<Item = MindSpec>,
        mut pin: impl FnMut(&str) -> Option<Sigil>,
        heat: Option<ResourceId>,
    ) -> Self {
        let specs: Vec<MindSpec> = specs.into_iter().collect();
        let mut pins = BTreeMap::new();
        for extra in ["hearth", "bucket", "ingot"] {
            if let Some(s) = pin(extra) {
                pins.insert(Name::from(extra), s);
            }
        }
        let mut agents = Vec::new();
        for spec in specs {
            if let Some(locus) = pin(spec.locus.as_str()) {
                pins.insert(spec.locus.clone(), locus);
                if locus.kind() == Some(LocusKind::Actor) {
                    agents.push(BoundAgent {
                        locus,
                        goals: spec.goals,
                        templates: spec.templates,
                    });
                }
            }
        }
        agents.sort_by_key(|a| a.locus);
        Self { agents, pins, heat }
    }

    /// Optional heat resource for `fetch_bucket` / Burning.
    #[must_use]
    pub fn with_heat(mut self, heat: ResourceId) -> Self {
        self.heat = Some(heat);
        self
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

    /// Plan this tick from the view. Scratch lives on the stack (K22).
    #[must_use]
    pub fn plan(&self, view: &WorldView<'_>) -> Vec<MindIntent> {
        let mut out = Vec::new();
        for agent in &self.agents {
            if let Some(intent) = plan_one(agent, view, &self.pins, self.heat) {
                out.push(intent);
            }
        }
        out
    }
}

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
            if view.island(intent.locus).map(|(id, _)| id) != Some(island) {
                continue;
            }
            out.push(Proposal::Mind(intent));
        }
    }
}

fn skip_lod(view: &WorldView<'_>, s: Sigil) -> bool {
    match view.sim_lod(s) {
        SimLod::Dormant => true,
        SimLod::Far => view.tick().0 % u64::from(LOD_PERIOD) != 0,
        SimLod::Full => false,
    }
}

fn plan_one(
    agent: &BoundAgent,
    view: &WorldView<'_>,
    pins: &BTreeMap<Name, Sigil>,
    heat: Option<ResourceId>,
) -> Option<MindIntent> {
    if view.first_rite(agent.locus).is_some() {
        return None;
    }
    if view.has_rel(agent.locus, Rel::Dead, agent.locus) {
        return None;
    }
    if skip_lod(view, agent.locus) {
        return None;
    }
    let mut best: Option<MindIntent> = None;
    for goal in &agent.goals {
        let Some(intent) = apply_goal(goal.as_str(), agent.locus, view, pins, heat) else {
            continue;
        };
        if intent.verb == Verb::Time {
            continue;
        }
        if best.as_ref().is_none_or(|b| intent.utility > b.utility) {
            best = Some(intent);
        }
    }
    best
}

fn apply_goal(
    goal: &str,
    locus: Sigil,
    view: &WorldView<'_>,
    pins: &BTreeMap<Name, Sigil>,
    heat: Option<ResourceId>,
) -> Option<MindIntent> {
    match goal {
        "stay_near_forge" => stay_near_forge(locus, view, pins),
        "investigate" => Some(intent(locus, Verb::Investigate, IntentTarget::None, 10)),
        "pump_bellows" => Some(intent(locus, Verb::Investigate, IntentTarget::None, 30)),
        "fetch_bucket" => fetch_bucket(locus, view, pins, heat),
        "evening_trade" => evening_trade(locus, pins),
        _ => None,
    }
}

fn stay_near_forge(
    locus: Sigil,
    view: &WorldView<'_>,
    pins: &BTreeMap<Name, Sigil>,
) -> Option<MindIntent> {
    let hearth = *pins.get(&Name::from("hearth"))?;
    if let (Some(here), Some(there)) = (view.pose(locus), view.pose(hearth)) {
        if xz_chebyshev(here, there) <= STAY_NEAR_MM {
            return None;
        }
    }
    Some(intent(locus, Verb::Move, IntentTarget::Sigil(hearth), 40))
}

fn fetch_bucket(
    locus: Sigil,
    view: &WorldView<'_>,
    pins: &BTreeMap<Name, Sigil>,
    heat: Option<ResourceId>,
) -> Option<MindIntent> {
    let heat = heat?;
    let bucket = *pins.get(&Name::from("bucket"))?;
    if !any_burning(view, heat) {
        return None;
    }
    if view.has_rel(bucket, Rel::WieldedBy, locus) {
        return None;
    }
    Some(intent(locus, Verb::Carry, IntentTarget::Sigil(bucket), 80))
}

fn evening_trade(locus: Sigil, pins: &BTreeMap<Name, Sigil>) -> Option<MindIntent> {
    let target = pins
        .get(&Name::from("ingot"))
        .copied()
        .map(IntentTarget::Sigil)
        .unwrap_or(IntentTarget::None);
    Some(intent(locus, Verb::Talk, target, 70))
}

fn any_burning(view: &WorldView<'_>, heat: ResourceId) -> bool {
    view.loci().any(|s| view.qty(s, heat) >= IGNITE_HEAT)
}

fn xz_chebyshev(a: PoseMm, b: PoseMm) -> i32 {
    let dx = a.x.wrapping_sub(b.x);
    let dz = a.z.wrapping_sub(b.z);
    dx.0.unsigned_abs().max(dz.0.unsigned_abs()) as i32
}

fn intent(locus: Sigil, verb: Verb, target: IntentTarget, utility: u16) -> MindIntent {
    MindIntent {
        locus,
        verb,
        target,
        utility,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::CommitKernel;
    use klotho_core::{Budget, Hash, LocusKind, Sigil, Tick};
    use klotho_ir::{CanonDiff, Name, from_ron};
    use klotho_world::World;

    use super::*;

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn kernel_with(agents: &[(u128, &str)]) -> (CommitKernel, BTreeMap<Name, Sigil>) {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let mut pins = BTreeMap::new();
        for (id, name) in agents {
            let s = actor(*id);
            k.world_mut().insert_locus(s, LocusKind::Actor).unwrap();
            pins.insert(Name::from(*name), s);
        }
        let hearth = Sigil::pack(LocusKind::Place, 0, 99).unwrap();
        k.world_mut()
            .insert_locus(hearth, LocusKind::Place)
            .unwrap();
        pins.insert(Name::from("hearth"), hearth);
        (k, pins)
    }

    fn spec(locus: &str, goals: &[&str]) -> MindSpec {
        MindSpec {
            locus: Name::from(locus),
            goals: goals.iter().map(|g| Name::from(*g)).collect(),
            templates: vec!["{name} won't sell that.".into()],
        }
    }

    #[test]
    fn name_is_mind() {
        assert_eq!(SyncProposer::name(&Mind::new()), "mind");
        assert_eq!(IslandProposer::name(&Mind::new()), "mind");
    }

    #[test]
    fn propose_does_not_write_fields() {
        let (k, _pins) = kernel_with(&[(1, "bran")]);
        let mut mind = Mind::bind(
            vec![spec("bran", &["investigate"])],
            |n| {
                k.canon().pin(n).or_else(|| match n {
                    "bran" => Some(actor(1)),
                    "hearth" => Sigil::pack(LocusKind::Place, 0, 99),
                    _ => None,
                })
            },
            None,
        );
        let before = mind.clone();
        let mut buf = AdmitBuf::new();
        mind.propose(&k.world().view(), Tick(1), &mut buf);
        assert_eq!(mind, before);
    }

    #[test]
    fn infer_off_npcs_plan_actions() {
        let (mut k, _pins) = kernel_with(&[(1, "bran"), (2, "mira"), (3, "kel")]);
        let mut mind = Mind::bind(
            vec![
                spec("bran", &["stay_near_forge", "investigate"]),
                spec("mira", &["pump_bellows", "investigate", "fetch_bucket"]),
                spec("kel", &["evening_trade"]),
            ],
            |n| match n {
                "bran" => Some(actor(1)),
                "mira" => Some(actor(2)),
                "kel" => Some(actor(3)),
                "hearth" => Sigil::pack(LocusKind::Place, 0, 99),
                _ => None,
            },
            None,
        );
        let planned = mind.plan(&k.world().view());
        assert!(
            planned
                .iter()
                .any(|i| i.locus == actor(1) && i.verb != Verb::Time),
            "{planned:?}"
        );
        assert!(
            planned
                .iter()
                .any(|i| i.locus == actor(3) && i.verb == Verb::Talk),
            "{planned:?}"
        );
        let d = k.step(Tick(1), Budget::HEARTH, &mut [&mut mind]).unwrap();
        assert!(
            d.rejects
                .iter()
                .all(|(_, r)| *r != klotho_core::RejectReason::UnclaimedAgency),
            "{d:?}"
        );
    }

    #[test]
    fn never_emits_time() {
        let (k, _) = kernel_with(&[(1, "bran")]);
        let mind = Mind::bind(
            vec![spec("bran", &["investigate"])],
            |n| match n {
                "bran" => Some(actor(1)),
                _ => None,
            },
            None,
        );
        assert!(
            mind.plan(&k.world().view())
                .iter()
                .all(|i| i.verb != Verb::Time)
        );
    }
}
