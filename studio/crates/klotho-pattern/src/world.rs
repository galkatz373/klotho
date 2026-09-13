//! Hierarchical world assembly (K75). Coarse-to-fine Place graphs, protected
//! anchors, and a deterministic Place budget solver. Chosen dressing is
//! content-addressed later; this module never emits runtime generation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use klotho_core::{AabbMm, BlobId, IVec3, YawMd};
use klotho_ir::{AnchorId, Name, ParameterValue, PatternArg, PatternInstance};

use crate::error::PatternError;
use crate::stdlib::latest;

fn latest_pattern_version(id: &str) -> u32 {
    latest(id).map(|s| s.version).unwrap_or(1)
}

/// Gameplay role of a Place in a first-title route.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceRole {
    /// Safe hub.
    Hub,
    /// Combat pocket.
    CombatPocket,
    /// Traversal beat.
    Traversal,
    /// Conversation beat.
    Conversation,
    /// Cinematic beat.
    Cinematic,
    /// Checkpoint / rest.
    Checkpoint,
    /// Return shortcut.
    Shortcut,
    /// Optional objective.
    Optional,
}

impl PlaceRole {
    /// Catalog snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hub => "hub",
            Self::CombatPocket => "combat_pocket",
            Self::Traversal => "traversal",
            Self::Conversation => "conversation",
            Self::Cinematic => "cinematic",
            Self::Checkpoint => "checkpoint",
            Self::Shortcut => "shortcut",
            Self::Optional => "optional",
        }
    }

    /// World-family pattern this role instantiates after the Place shell.
    #[must_use]
    pub const fn pattern_id(self) -> &'static str {
        match self {
            Self::Hub => "world.safe_hub",
            Self::CombatPocket => "world.encounter_pocket",
            Self::Traversal | Self::Shortcut => "world.traversal_graph",
            Self::Conversation | Self::Cinematic | Self::Checkpoint | Self::Optional => {
                "world.place_shell"
            }
        }
    }
}

/// Why a directed Place edge exists.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Required critical-path connection.
    Critical,
    /// Optional branch.
    Optional,
    /// Return / skip shortcut.
    Shortcut,
}

/// Directed (or bidirectional) traversal between Places.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraversalEdge {
    /// Origin Place.
    pub from: Name,
    /// Destination Place.
    pub to: Name,
    /// When true, the reverse edge is implied.
    pub bidirectional: bool,
    /// Edge class.
    pub kind: EdgeKind,
}

/// Per-Place density, streaming, nav, Phys, audio, and GPU caps.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceBudgets {
    /// Maximum dressing instances.
    pub density: u32,
    /// Maximum unique bindings visible in the Place.
    pub visibility: u32,
    /// Maximum unique dressing bytes streamed with the Place.
    pub streaming_bytes: u64,
    /// Maximum nav-cell cost.
    pub nav_cells: u32,
    /// Maximum Phys-body cost.
    pub phys_bodies: u32,
    /// Maximum concurrent audio voices.
    pub audio_voices: u32,
    /// Maximum GPU instances.
    pub gpu_instances: u32,
}

impl PlaceBudgets {
    /// Greybox defaults that still fail closed on a protected over-cap.
    #[must_use]
    pub const fn greybox() -> Self {
        Self {
            density: 64,
            visibility: 16,
            streaming_bytes: 64 * 1024,
            nav_cells: 64,
            phys_bodies: 64,
            audio_voices: 8,
            gpu_instances: 64,
        }
    }
}

/// One Place in a world plan.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacePlan {
    /// Authoring name. Not identity.
    pub name: Name,
    /// Route role.
    pub role: PlaceRole,
    /// Streaming envelope, millimetres.
    pub envelope: AabbMm,
    /// Multidomain caps.
    pub budgets: PlaceBudgets,
    /// Semantic anchors that regeneration must preserve.
    pub protected: Vec<AnchorId>,
    /// Dressing zone tags owned by this Place.
    pub zones: Vec<Name>,
}

/// Coarse-to-fine world graph. Dressing is solved separately.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldPlan {
    /// Immutable plan identity.
    pub anchor: AnchorId,
    /// Authoring id.
    pub id: Name,
    /// Places. [`WorldPlan::validate`] sorts by name.
    pub places: Vec<PlacePlan>,
    /// Traversal edges.
    pub edges: Vec<TraversalEdge>,
    /// Ordered critical-path Place names. Order is the journey, not lexical.
    pub critical_path: Vec<Name>,
    /// Plan-level protected anchors (Places, traversal, checkpoints).
    pub protected: Vec<AnchorId>,
}

/// Content-addressed dressing instance proposed for a Place/zone.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DressingInstance {
    /// Owning Place.
    pub place: Name,
    /// Owning dressing zone.
    pub zone: Name,
    /// Shared mesh blob.
    pub mesh: BlobId,
    /// Shared material blob.
    pub material: BlobId,
    /// Optional clip/animation blob.
    pub clip: Option<BlobId>,
    /// Variant index.
    pub variant: u16,
    /// Translation, millimetres.
    pub pose: IVec3,
    /// Yaw, millidegrees.
    pub yaw: YawMd,
    /// Uniform scale in permille (`1000` = 1.0).
    pub scale_permille: u16,
    /// Unique bytes of `mesh`/`material`/`clip` for streaming budgets.
    pub blob_bytes: u32,
    /// Nav-cell cost.
    pub nav_cost: u32,
    /// Phys-body cost.
    pub phys_cost: u32,
    /// Audio-voice cost.
    pub audio_cost: u32,
    /// When true the solver may not drop this instance.
    pub protected: bool,
}

/// Result of a budget solve. Accepted instances are sorted canonically.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SolvedDressing {
    /// Instances that fit every Place budget.
    pub instances: Vec<DressingInstance>,
    /// Dropped non-protected overflow, sorted.
    pub culled: Vec<DressingInstance>,
}

impl WorldPlan {
    /// Sort Places, zones, edges, and protected lists. Critical-path order is
    /// kept.
    pub fn normalize(&mut self) {
        self.places.sort_by(|a, b| a.name.cmp(&b.name));
        for place in &mut self.places {
            place.zones.sort();
            place.zones.dedup();
            place.protected.sort();
            place.protected.dedup();
        }
        self.edges
            .sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
        self.protected.sort();
        self.protected.dedup();
    }

    /// Fail closed on missing names, duplicate Places, or a critical path that
    /// is not a walk of the graph.
    pub fn validate(&self) -> Result<(), PatternError> {
        if self.id.as_str().is_empty() || self.anchor == AnchorId::ZERO {
            return Err(graph_err("world plan id or anchor is empty"));
        }
        let mut names = BTreeSet::new();
        for place in &self.places {
            if place.name.as_str().is_empty() {
                return Err(graph_err("empty place name"));
            }
            if place.envelope.is_empty() {
                return Err(graph_err(format!("empty envelope {}", place.name.as_str())));
            }
            if !names.insert(place.name.clone()) {
                return Err(graph_err(format!(
                    "duplicate place {}",
                    place.name.as_str()
                )));
            }
            if place.protected.contains(&AnchorId::ZERO) {
                return Err(PatternError::ProtectedAnchor {
                    token: place.name.as_str().to_owned(),
                });
            }
        }
        if self.places.is_empty() {
            return Err(graph_err("no places"));
        }
        for edge in &self.edges {
            if !names.contains(&edge.from) {
                return Err(graph_err(format!(
                    "edge from unknown {}",
                    edge.from.as_str()
                )));
            }
            if !names.contains(&edge.to) {
                return Err(graph_err(format!("edge to unknown {}", edge.to.as_str())));
            }
        }
        if self.critical_path.is_empty() {
            return Err(graph_err("empty critical path"));
        }
        for name in &self.critical_path {
            if !names.contains(name) {
                return Err(graph_err(format!(
                    "critical path names unknown {}",
                    name.as_str()
                )));
            }
        }
        let adj = directed_adj(self);
        for window in self.critical_path.windows(2) {
            if !adj
                .get(&window[0])
                .is_some_and(|next| next.contains(&window[1]))
            {
                return Err(graph_err(format!(
                    "critical path missing {} -> {}",
                    window[0].as_str(),
                    window[1].as_str()
                )));
            }
        }
        if self.protected.contains(&AnchorId::ZERO) {
            return Err(PatternError::ProtectedAnchor {
                token: "plan".into(),
            });
        }
        Ok(())
    }

    /// Place by name.
    #[must_use]
    pub fn place(&self, name: &Name) -> Option<&PlacePlan> {
        self.places.iter().find(|p| p.name == *name)
    }

    /// Directed adjacency including implied reverse edges.
    #[must_use]
    pub fn adjacency(&self) -> BTreeMap<Name, BTreeSet<Name>> {
        directed_adj(self)
    }
}

fn graph_err(reason: impl Into<String>) -> PatternError {
    PatternError::WorldGraph {
        reason: reason.into(),
    }
}

fn directed_adj(plan: &WorldPlan) -> BTreeMap<Name, BTreeSet<Name>> {
    let mut adj: BTreeMap<Name, BTreeSet<Name>> = BTreeMap::new();
    for place in &plan.places {
        adj.entry(place.name.clone()).or_default();
    }
    for edge in &plan.edges {
        adj.entry(edge.from.clone())
            .or_default()
            .insert(edge.to.clone());
        if edge.bidirectional {
            adj.entry(edge.to.clone())
                .or_default()
                .insert(edge.from.clone());
        }
    }
    adj
}

/// BFS from the first critical-path Place.
#[must_use]
pub fn reachable_from(plan: &WorldPlan, start: &Name) -> BTreeSet<Name> {
    let adj = directed_adj(plan);
    let mut seen = BTreeSet::new();
    let mut stack = vec![start.clone()];
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        if let Some(next) = adj.get(&cur) {
            stack.extend(next.iter().cloned());
        }
    }
    seen
}

/// Keep every protected instance. Fill remaining budget in canonical order.
/// Never deletes a Place or a critical-path / protected anchor.
pub fn solve_budgets(
    plan: &WorldPlan,
    proposed: &[DressingInstance],
) -> Result<SolvedDressing, PatternError> {
    plan.validate()?;
    let mut by_place: BTreeMap<Name, Vec<DressingInstance>> = BTreeMap::new();
    for inst in proposed {
        if plan.place(&inst.place).is_none() {
            return Err(graph_err(format!(
                "dressing for unknown {}",
                inst.place.as_str()
            )));
        }
        by_place
            .entry(inst.place.clone())
            .or_default()
            .push(inst.clone());
    }
    let mut kept = Vec::new();
    let mut culled = Vec::new();
    for place in &plan.places {
        let mut rows = by_place.remove(&place.name).unwrap_or_default();
        rows.sort_by(cmp_dressing);
        let mut protected = Vec::new();
        let mut rest = Vec::new();
        for row in rows {
            if row.protected {
                protected.push(row);
            } else {
                rest.push(row);
            }
        }
        charge(place, &protected)?;
        let mut accepted = protected;
        for row in rest {
            let mut trial = accepted.clone();
            trial.push(row.clone());
            if charge(place, &trial).is_ok() {
                accepted = trial;
            } else {
                culled.push(row);
            }
        }
        kept.extend(accepted);
    }
    kept.sort_by(cmp_dressing);
    culled.sort_by(cmp_dressing);
    Ok(SolvedDressing {
        instances: kept,
        culled,
    })
}

fn cmp_dressing(a: &DressingInstance, b: &DressingInstance) -> std::cmp::Ordering {
    a.place
        .cmp(&b.place)
        .then(a.zone.cmp(&b.zone))
        .then(a.mesh.0.cmp(&b.mesh.0))
        .then(a.material.0.cmp(&b.material.0))
        .then(a.clip.map(|c| c.0).cmp(&b.clip.map(|c| c.0)))
        .then(a.variant.cmp(&b.variant))
        .then(a.pose.x.cmp(&b.pose.x))
        .then(a.pose.y.cmp(&b.pose.y))
        .then(a.pose.z.cmp(&b.pose.z))
        .then(a.yaw.0.cmp(&b.yaw.0))
        .then(a.scale_permille.cmp(&b.scale_permille))
}

fn charge(place: &PlacePlan, rows: &[DressingInstance]) -> Result<(), PatternError> {
    let mut density = 0u32;
    let mut gpu = 0u32;
    let mut nav = 0u32;
    let mut phys = 0u32;
    let mut audio = 0u32;
    let mut groups = BTreeSet::new();
    let mut blobs = BTreeMap::new();
    for row in rows {
        density = density.saturating_add(1);
        gpu = gpu.saturating_add(1);
        nav = nav.saturating_add(row.nav_cost);
        phys = phys.saturating_add(row.phys_cost);
        audio = audio.saturating_add(row.audio_cost);
        groups.insert((row.mesh, row.material, row.clip, row.variant));
        blobs.entry(row.mesh).or_insert(row.blob_bytes);
        blobs.entry(row.material).or_insert(0);
        if let Some(clip) = row.clip {
            blobs.entry(clip).or_insert(0);
        }
    }
    let streaming = u64::from(blobs.values().copied().sum::<u32>());
    let vis = u32::try_from(groups.len()).unwrap_or(u32::MAX);
    miss(
        place,
        "density",
        u64::from(density),
        u64::from(place.budgets.density),
    )?;
    miss(
        place,
        "visibility",
        u64::from(vis),
        u64::from(place.budgets.visibility),
    )?;
    miss(place, "streaming", streaming, place.budgets.streaming_bytes)?;
    miss(
        place,
        "nav",
        u64::from(nav),
        u64::from(place.budgets.nav_cells),
    )?;
    miss(
        place,
        "phys",
        u64::from(phys),
        u64::from(place.budgets.phys_bodies),
    )?;
    miss(
        place,
        "audio",
        u64::from(audio),
        u64::from(place.budgets.audio_voices),
    )?;
    miss(
        place,
        "gpu",
        u64::from(gpu),
        u64::from(place.budgets.gpu_instances),
    )?;
    Ok(())
}

fn miss(place: &PlacePlan, domain: &str, used: u64, cap: u64) -> Result<(), PatternError> {
    if used > cap {
        Err(PatternError::WorldBudget {
            place: place.name.as_str().to_owned(),
            domain: domain.to_owned(),
            used,
            cap,
        })
    } else {
        Ok(())
    }
}

/// Re-solve dressing against an unchanged plan. Protected plan anchors are
/// compared byte-for-byte with `base`.
pub fn regenerate_dressing(
    base: &WorldPlan,
    proposed: &[DressingInstance],
) -> Result<(WorldPlan, SolvedDressing), PatternError> {
    let mut plan = base.clone();
    plan.normalize();
    plan.validate()?;
    if plan.anchor != base.anchor || plan.critical_path != base.critical_path {
        return Err(PatternError::ProtectedAnchor {
            token: "plan".into(),
        });
    }
    let mut base_protected = base.protected.clone();
    base_protected.sort();
    base_protected.dedup();
    if plan.protected != base_protected || plan.places.len() != base.places.len() {
        return Err(PatternError::ProtectedAnchor {
            token: "plan".into(),
        });
    }
    for place in &plan.places {
        let Some(orig) = base.place(&place.name) else {
            return Err(PatternError::ProtectedAnchor {
                token: place.name.as_str().to_owned(),
            });
        };
        let mut orig_protected = orig.protected.clone();
        orig_protected.sort();
        orig_protected.dedup();
        if place.protected != orig_protected || place.role != orig.role {
            return Err(PatternError::ProtectedAnchor {
                token: place.name.as_str().to_owned(),
            });
        }
    }
    let solved = solve_budgets(&plan, proposed)?;
    Ok((plan, solved))
}

/// Expand a validated plan to ordinary world-family pattern instances.
pub fn expand_world_plan(
    plan: &WorldPlan,
    module: AnchorId,
) -> Result<Vec<PatternInstance>, PatternError> {
    plan.validate()?;
    let mut out = Vec::new();
    for place in &plan.places {
        out.push(instance(
            plan,
            module,
            place,
            "shell",
            "world.place_shell",
            vec![arg_name("place", place.name.clone())],
        ));
        let role_id = place.role.pattern_id();
        if role_id != "world.place_shell" {
            let mut args = vec![arg_name("place", place.name.clone())];
            if role_id == "world.encounter_pocket" {
                args.push(arg_name("envelope", place.name.clone()));
            } else {
                args.push(arg_name("tag", Name::from(place.role.as_str())));
            }
            out.push(instance(plan, module, place, "role", role_id, args));
        }
        if place.role == PlaceRole::Checkpoint {
            out.push(instance(
                plan,
                module,
                place,
                "checkpoint",
                "traversal.checkpoint",
                vec![
                    arg_name("target", place.name.clone()),
                    arg_name("label", Name::from("rest")),
                ],
            ));
        }
        for zone in &place.zones {
            out.push(instance(
                plan,
                module,
                place,
                &format!("zone:{}", zone.as_str()),
                "world.dressing_zone",
                vec![
                    arg_name("place", place.name.clone()),
                    arg_name("tag", zone.clone()),
                ],
            ));
        }
    }
    out.sort_by(|a, b| {
        a.instance
            .cmp(&b.instance)
            .then(a.pattern.cmp(&b.pattern))
            .then(a.version.cmp(&b.version))
    });
    Ok(out)
}

fn arg_name(key: &str, value: Name) -> PatternArg {
    PatternArg {
        key: Name::from(key),
        value: ParameterValue::Name(value),
    }
}

fn instance(
    plan: &WorldPlan,
    module: AnchorId,
    place: &PlacePlan,
    local: &str,
    pattern: &str,
    args: Vec<PatternArg>,
) -> PatternInstance {
    let token = format!("{}:{local}", place.name.as_str());
    let instance = Name::from(format!("{}__{}", place.name.as_str(), local).as_str());
    PatternInstance {
        anchor: plan.anchor.child(token.as_bytes()),
        module,
        instance,
        pattern: Name::from(pattern),
        version: latest_pattern_version(pattern),
        args,
    }
}

/// Eight-Place greybox route: hub, combat, traversal, conversation, cinematic,
/// checkpoint, return shortcut, optional objective.
#[must_use]
pub fn greybox_route() -> WorldPlan {
    let anchor = AnchorId::derive(b"kai-14", b"greybox-route");
    let names = [
        ("hub", PlaceRole::Hub),
        ("combat", PlaceRole::CombatPocket),
        ("traverse", PlaceRole::Traversal),
        ("talk", PlaceRole::Conversation),
        ("cinema", PlaceRole::Cinematic),
        ("rest", PlaceRole::Checkpoint),
        ("shortcut", PlaceRole::Shortcut),
        ("optional", PlaceRole::Optional),
    ];
    let mut places = Vec::new();
    let mut protected = Vec::new();
    for (i, (name, role)) in names.iter().enumerate() {
        let origin = (i as i32) * 40_000;
        let place_anchor = anchor.child(name.as_bytes());
        protected.push(place_anchor);
        places.push(PlacePlan {
            name: Name::from(*name),
            role: *role,
            envelope: AabbMm::sorted(
                IVec3 {
                    x: 0,
                    y: 0,
                    z: origin,
                },
                IVec3 {
                    x: 20_000,
                    y: 8_000,
                    z: origin + 20_000,
                },
            ),
            budgets: PlaceBudgets::greybox(),
            protected: vec![place_anchor],
            zones: vec![Name::from("dress")],
        });
    }
    let mut plan = WorldPlan {
        anchor,
        id: Name::from("greybox"),
        places,
        edges: vec![
            edge("hub", "combat", false, EdgeKind::Critical),
            edge("combat", "traverse", false, EdgeKind::Critical),
            edge("traverse", "talk", false, EdgeKind::Critical),
            edge("talk", "cinema", false, EdgeKind::Critical),
            edge("cinema", "rest", false, EdgeKind::Critical),
            edge("rest", "shortcut", false, EdgeKind::Critical),
            edge("shortcut", "hub", true, EdgeKind::Shortcut),
            edge("hub", "optional", true, EdgeKind::Optional),
        ],
        critical_path: [
            "hub", "combat", "traverse", "talk", "cinema", "rest", "shortcut",
        ]
        .into_iter()
        .map(Name::from)
        .collect(),
        protected,
    };
    plan.normalize();
    plan
}

fn edge(from: &str, to: &str, bidirectional: bool, kind: EdgeKind) -> TraversalEdge {
    TraversalEdge {
        from: Name::from(from),
        to: Name::from(to),
        bidirectional,
        kind,
    }
}

/// Shared greybox kit: two meshes reused across Places.
#[must_use]
pub fn greybox_dressing(plan: &WorldPlan) -> Vec<DressingInstance> {
    let tree = blob(b"greybox-tree");
    let rock = blob(b"greybox-rock");
    let mat = blob(b"greybox-mat");
    let mut out = Vec::new();
    for place in &plan.places {
        for k in 0..4u16 {
            let mesh = if k % 2 == 0 { tree } else { rock };
            out.push(DressingInstance {
                place: place.name.clone(),
                zone: Name::from("dress"),
                mesh,
                material: mat,
                clip: None,
                variant: k % 2,
                pose: IVec3 {
                    x: 1_000 + i32::from(k) * 2_000,
                    y: 0,
                    z: place.envelope.min.z + 2_000,
                },
                yaw: YawMd(i32::from(k) * 45_000),
                scale_permille: 1000,
                blob_bytes: 256,
                nav_cost: 1,
                phys_cost: 1,
                audio_cost: 0,
                protected: k == 0,
            });
        }
    }
    out
}

fn blob(bytes: &[u8]) -> BlobId {
    let hash = klotho_prove::hash_bytes(bytes);
    BlobId(hash.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greybox_has_eight_places_and_a_walkable_critical_path() {
        let plan = greybox_route();
        plan.validate().unwrap();
        assert_eq!(plan.places.len(), 8);
        assert_eq!(plan.critical_path.len(), 7);
        let start = &plan.critical_path[0];
        let seen = reachable_from(&plan, start);
        assert_eq!(seen.len(), 8, "{seen:?}");
        assert!(seen.contains(&Name::from("optional")));
    }

    #[test]
    fn regeneration_preserves_protected_anchors() {
        let base = greybox_route();
        let mut proposed = greybox_dressing(&base);
        proposed.push(DressingInstance {
            place: Name::from("hub"),
            zone: Name::from("dress"),
            mesh: blob(b"extra"),
            material: blob(b"greybox-mat"),
            clip: None,
            variant: 9,
            pose: IVec3 { x: 50, y: 0, z: 50 },
            yaw: YawMd::ZERO,
            scale_permille: 1000,
            blob_bytes: 16,
            nav_cost: 0,
            phys_cost: 0,
            audio_cost: 0,
            protected: false,
        });
        let (plan, solved) = regenerate_dressing(&base, &proposed).unwrap();
        assert_eq!(plan.anchor, base.anchor);
        assert_eq!(plan.protected, base.protected);
        assert_eq!(plan.critical_path, base.critical_path);
        for (a, b) in plan.places.iter().zip(base.places.iter()) {
            assert_eq!(a.protected, b.protected);
        }
        assert!(solved.instances.iter().any(|d| d.variant == 9));
    }

    #[test]
    fn budget_solve_cannot_drop_protected_or_critical_places() {
        let mut plan = greybox_route();
        plan.places
            .iter_mut()
            .find(|p| p.name.as_str() == "hub")
            .unwrap()
            .budgets
            .density = 0;
        let proposed = greybox_dressing(&plan);
        let err = solve_budgets(&plan, &proposed).unwrap_err();
        assert!(
            matches!(err, PatternError::WorldBudget { ref place, .. } if place == "hub"),
            "{err}"
        );
        assert_eq!(plan.places.len(), 8);
        assert_eq!(plan.critical_path.len(), 7);
    }

    #[test]
    fn budget_solve_culls_unprotected_and_keeps_protected() {
        let mut plan = greybox_route();
        plan.places
            .iter_mut()
            .find(|p| p.name.as_str() == "hub")
            .unwrap()
            .budgets
            .density = 1;
        let proposed = greybox_dressing(&plan);
        let solved = solve_budgets(&plan, &proposed).unwrap();
        let hub: Vec<_> = solved
            .instances
            .iter()
            .filter(|d| d.place.as_str() == "hub")
            .collect();
        assert_eq!(hub.len(), 1);
        assert!(hub[0].protected);
        assert_eq!(solved.culled.len(), 3);
    }

    #[test]
    fn reorder_does_not_change_expansion() {
        let mut plan = greybox_route();
        let module = plan.anchor.child(b"module");
        let forward = expand_world_plan(&plan, module).unwrap();
        plan.places.reverse();
        plan.edges.reverse();
        let reverse = expand_world_plan(&plan, module).unwrap();
        assert_eq!(forward, reverse);
        assert!(
            forward
                .iter()
                .any(|p| p.pattern.as_str() == "world.place_shell")
        );
        assert!(
            forward
                .iter()
                .any(|p| p.pattern.as_str() == "world.safe_hub")
        );
        assert!(
            forward
                .iter()
                .any(|p| p.pattern.as_str() == "traversal.checkpoint")
        );
    }

    #[test]
    fn missing_critical_edge_fails_closed() {
        let mut plan = greybox_route();
        plan.edges
            .retain(|e| e.from.as_str() != "hub" || e.to.as_str() != "combat");
        let err = plan.validate().unwrap_err();
        assert!(matches!(err, PatternError::WorldGraph { .. }), "{err}");
    }
}
