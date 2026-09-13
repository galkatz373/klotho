//! Pure capability-checked expansion. No RNG, no HashMap, no runtime types.

use std::collections::BTreeMap;

use klotho_core::LocusKind;
use klotho_ir::{
    Affordance, AnchorId, AnchorKind, Beat, BindSrc, CanonDiff, IntentModule, Law, LawBody,
    MindFact, MindGoal, MindOperator, MindProgram, MindQuery, MindSpec, MindTarget, Name,
    ObjectAnchor, ParameterType, ParameterValue, PatternInstance, Pred, ProjectBundle, Rel,
    RiteGraph, RiteNode, RiteOp, SeedFact, Slot, Status, Verb,
};

use crate::def::{
    ExpandKind, Expansion, ExpansionCost, HostCaps, JourneyHook, PatternSpan, PatternSpec,
};
use crate::error::PatternError;
use crate::stdlib::{lookup, specs};

struct Ctx<'a> {
    module: &'a IntentModule,
    instance: &'a PatternInstance,
    spec: &'static PatternSpec,
    args: BTreeMap<&'static str, ParameterValue>,
}

impl Ctx<'_> {
    fn qual(&self, local: &str) -> Name {
        Name::from(
            format!(
                "{}__{}__{}",
                self.module.id.as_str(),
                self.instance.instance.as_str(),
                local
            )
            .as_str(),
        )
    }

    fn child(&self, local: &str) -> AnchorId {
        let token = format!("v{}:{local}", self.instance.version);
        self.instance.anchor.child(token.as_bytes())
    }

    fn arg_name(&self, key: &str) -> Result<Name, PatternError> {
        match self.args.get(key) {
            Some(ParameterValue::Name(n)) => Ok(n.clone()),
            Some(_) => Err(arg_err(self.spec.id, key, "expected Name")),
            None => Err(arg_err(self.spec.id, key, "missing")),
        }
    }

    fn arg_bool(&self, key: &str) -> Result<bool, PatternError> {
        match self.args.get(key) {
            Some(ParameterValue::Bool(v)) => Ok(*v),
            Some(_) => Err(arg_err(self.spec.id, key, "expected Bool")),
            None => Err(arg_err(self.spec.id, key, "missing")),
        }
    }

    fn arg_i32(&self, key: &str) -> Result<i32, PatternError> {
        match self.args.get(key) {
            Some(ParameterValue::I32(v)) => Ok(*v),
            Some(_) => Err(arg_err(self.spec.id, key, "expected I32")),
            None => Err(arg_err(self.spec.id, key, "missing")),
        }
    }

    fn locus_exists(&self, name: &Name) -> bool {
        self.module.body.seed.iter().any(|f| match f {
            SeedFact::Locus { name: n, .. } => n == name,
            _ => false,
        })
    }
}

fn arg_err(id: &str, key: &str, reason: &str) -> PatternError {
    PatternError::Arg {
        id: id.to_owned(),
        key: key.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Bind arguments, filling defaults. Unknown keys fail closed.
pub fn bind_args(
    spec: &PatternSpec,
    instance: &PatternInstance,
) -> Result<BTreeMap<&'static str, ParameterValue>, PatternError> {
    let mut supplied: BTreeMap<&str, &ParameterValue> = BTreeMap::new();
    for arg in &instance.args {
        let Some(param) = spec.params.iter().find(|p| p.name == arg.key.as_str()) else {
            return Err(arg_err(spec.id, arg.key.as_str(), "unknown"));
        };
        if value_ty(&arg.value) != param.ty {
            return Err(arg_err(spec.id, arg.key.as_str(), "type mismatch"));
        }
        supplied.insert(param.name, &arg.value);
    }
    let mut out = BTreeMap::new();
    for param in spec.params {
        if let Some(v) = supplied.get(param.name) {
            out.insert(param.name, (*v).clone());
        } else if let Some(default) = param.default {
            out.insert(param.name, default.to_value());
        } else {
            return Err(arg_err(spec.id, param.name, "missing"));
        }
    }
    Ok(out)
}

fn value_ty(v: &ParameterValue) -> ParameterType {
    match v {
        ParameterValue::Name(_) => ParameterType::Name,
        ParameterValue::I32(_) => ParameterType::I32,
        ParameterValue::Bool(_) => ParameterType::Bool,
        ParameterValue::Anchor(_) => ParameterType::Anchor,
    }
}

fn check_caps(spec: &PatternSpec, ctx: &Ctx<'_>, caps: &HostCaps) -> Result<(), PatternError> {
    for (param, cap) in spec.requires {
        let locus = ctx.arg_name(param)?;
        if !ctx.locus_exists(&locus) {
            return Err(PatternError::Arg {
                id: spec.id.to_owned(),
                key: (*param).to_owned(),
                reason: format!("unknown locus {}", locus.as_str()),
            });
        }
        if !caps.has(locus.as_str(), cap) {
            return Err(PatternError::Capability {
                locus: locus.0,
                cap: (*cap).to_owned(),
            });
        }
    }
    for (param, cap) in spec.conflicts {
        let locus = ctx.arg_name(param)?;
        if caps.has(locus.as_str(), cap) {
            return Err(PatternError::Conflict {
                locus: locus.0,
                cap: (*cap).to_owned(),
            });
        }
    }
    Ok(())
}

/// Expand one instance against `module` and `caps`. Pure.
pub fn expand_instance(
    module: &IntentModule,
    instance: &PatternInstance,
    caps: &HostCaps,
) -> Result<Expansion, PatternError> {
    let spec = lookup(instance.pattern.as_str(), instance.version).ok_or_else(|| {
        if specs().iter().any(|s| s.id == instance.pattern.as_str()) {
            PatternError::Version {
                id: instance.pattern.0.clone(),
                requested: instance.version,
            }
        } else {
            PatternError::Unknown(instance.pattern.0.clone())
        }
    })?;
    let args = bind_args(spec, instance)?;
    let ctx = Ctx {
        module,
        instance,
        spec,
        args,
    };
    check_caps(spec, &ctx, caps)?;
    let mut expansion = emit(&ctx)?;
    expansion.cost = measure(&expansion);
    if expansion.cost.predicates > spec.budget.predicates {
        return Err(PatternError::Budget {
            id: spec.id.to_owned(),
            counter: "predicates".into(),
            used: expansion.cost.predicates,
            cap: spec.budget.predicates,
        });
    }
    if expansion.cost.rite_steps > spec.budget.rite_steps {
        return Err(PatternError::Budget {
            id: spec.id.to_owned(),
            counter: "rite_steps".into(),
            used: expansion.cost.rite_steps,
            cap: spec.budget.rite_steps,
        });
    }
    if expansion.cost.per_tick > spec.budget.per_tick {
        return Err(PatternError::Budget {
            id: spec.id.to_owned(),
            counter: "per_tick".into(),
            used: expansion.cost.per_tick,
            cap: spec.budget.per_tick,
        });
    }
    expansion.journeys.sort_by(|a, b| a.name.cmp(&b.name));
    expansion
        .anchors
        .sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    expansion.spans.sort_by(|a, b| a.local_id.cmp(&b.local_id));
    Ok(expansion)
}

/// Expand every instance in canonical `(instance name, anchor)` order.
pub fn expand_module(
    module: &IntentModule,
    caps: &mut HostCaps,
) -> Result<IntentModule, PatternError> {
    if module.patterns.is_empty() {
        return Ok(module.clone());
    }
    let mut instances = module.patterns.clone();
    instances.sort_by(|a, b| a.instance.cmp(&b.instance).then(a.anchor.cmp(&b.anchor)));
    let mut out = module.clone();
    for instance in &instances {
        let expansion = expand_instance(&out, instance, caps)?;
        merge_expansion(&mut out, &expansion);
        for (locus, cap) in &expansion.grants {
            caps.grant(locus.as_str(), cap.as_str());
        }
    }
    out.patterns.clear();
    out.object_anchors.retain(|o| o.kind != AnchorKind::Pattern);
    out.validate_local()?;
    Ok(out)
}

/// Expand a project. Source bytes are not mutated; the result has empty `patterns`
/// and a lock that matches the expanded module bytes so flatten can proceed.
pub fn expand_bundle(bundle: &ProjectBundle) -> Result<ProjectBundle, PatternError> {
    let mut expanded = ProjectBundle {
        project: bundle.project.clone(),
        modules: Vec::new(),
    };
    let mut sorted = bundle.modules.clone();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for module in sorted {
        let mut caps = caps_from_module(&module);
        expanded.modules.push(expand_module(&module, &mut caps)?);
    }
    for module in &expanded.modules {
        let hash = module.content_hash()?;
        if let Some(r) = expanded
            .project
            .modules
            .iter_mut()
            .find(|r| r.id == module.id)
        {
            r.hash = hash;
        }
        if let Some(e) = expanded
            .project
            .lock
            .entries
            .iter_mut()
            .find(|e| e.id == module.id)
        {
            e.hash = hash;
        }
    }
    Ok(expanded)
}

/// `Knows` seed rows are the authoring encoding of granted capabilities.
pub fn caps_from_module(module: &IntentModule) -> HostCaps {
    let mut caps = HostCaps::new();
    for fact in &module.body.seed {
        if let SeedFact::Rel {
            a,
            rel: Rel::Knows,
            b,
        } = fact
        {
            caps.grant(a.as_str(), b.as_str());
        }
    }
    caps
}

fn merge_expansion(module: &mut IntentModule, expansion: &Expansion) {
    for fact in &expansion.seed {
        if !module.body.seed.iter().any(|s| s == fact) {
            module.body.seed.push(fact.clone());
        }
    }
    module
        .body
        .canon_diffs
        .extend(expansion.canon_diffs.iter().cloned());
    module.body.minds.extend(expansion.minds.iter().cloned());
    for object in &expansion.anchors {
        if !module
            .object_anchors
            .iter()
            .any(|o| o.anchor == object.anchor || (o.kind == object.kind && o.name == object.name))
        {
            module.object_anchors.push(object.clone());
        }
    }
}

fn emit(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    match ctx.spec.kind {
        ExpandKind::LockablePassage => emit_lockable(ctx),
        ExpandKind::Marker => emit_marker(ctx),
        ExpandKind::TraversalContract => emit_traversal(ctx),
        ExpandKind::CombatExchange => emit_combat(ctx),
        ExpandKind::Destructible => emit_destructible(ctx),
        ExpandKind::EncounterBoundary => emit_boundary(ctx),
        ExpandKind::MindPolicy => emit_mind(ctx),
        ExpandKind::QuestStep => emit_quest(ctx),
        ExpandKind::Narrative => emit_narrative(ctx),
        ExpandKind::PlaceShell => emit_place(ctx),
        ExpandKind::Zone => emit_zone(ctx),
        ExpandKind::UiCue => emit_ui(ctx),
        ExpandKind::ProductionEncounter => emit_perf(ctx),
        ExpandKind::FeelContract => emit_feel(ctx),
    }
}

struct Builder {
    seed: Vec<SeedFact>,
    diffs: Vec<CanonDiff>,
    minds: Vec<MindSpec>,
    journeys: Vec<JourneyHook>,
    anchors: Vec<ObjectAnchor>,
    spans: Vec<PatternSpan>,
    grants: Vec<(Name, Name)>,
}

impl Builder {
    fn new(ctx: &Ctx<'_>) -> Self {
        let mut journeys = Vec::new();
        for hook in ctx.spec.journeys {
            journeys.push(JourneyHook {
                name: Name::from(*hook),
                pattern: Name::from(ctx.spec.id),
                instance: ctx.instance.instance.clone(),
            });
        }
        let mut grants = Vec::new();
        for (param, cap) in ctx.spec.grants {
            if let Some(ParameterValue::Name(n)) = ctx.args.get(param) {
                grants.push((n.clone(), Name::from(*cap)));
            }
        }
        Self {
            seed: Vec::new(),
            diffs: Vec::new(),
            minds: Vec::new(),
            journeys,
            anchors: Vec::new(),
            spans: Vec::new(),
            grants,
        }
    }

    fn span(&mut self, ctx: &Ctx<'_>, local: &str) {
        self.spans.push(PatternSpan {
            instance: ctx.instance.anchor,
            pattern: Name::from(ctx.spec.id),
            version: ctx.instance.version,
            local_id: Name::from(local),
        });
    }

    fn push_anchor(&mut self, ctx: &Ctx<'_>, kind: AnchorKind, name: Name, local: &str) {
        self.anchors.push(ObjectAnchor {
            kind,
            name,
            anchor: ctx.child(local),
        });
        self.span(ctx, local);
    }

    fn affordance(&mut self, ctx: &Ctx<'_>, local: &str, grants: &[&str], conflicts: &[&str]) {
        let id = ctx.qual(local);
        self.push_anchor(ctx, AnchorKind::Affordance, id.clone(), local);
        self.diffs.push(CanonDiff::AddAffordance(Affordance {
            id,
            requires: Vec::new(),
            grants: grants.iter().map(|g| Name::from(*g)).collect(),
            conflicts: conflicts.iter().map(|c| Name::from(*c)).collect(),
        }));
    }

    fn law(&mut self, ctx: &Ctx<'_>, local: &str, when: Pred, must: Pred) {
        let id = ctx.qual(local);
        self.push_anchor(ctx, AnchorKind::Law, id.clone(), local);
        self.diffs.push(CanonDiff::AddLaw(Law {
            id,
            when,
            body: LawBody::Pred { must, ought: None },
        }));
    }

    fn cap_law(&mut self, ctx: &Ctx<'_>, local: &str, mark: Pred, n: u16) {
        let id = ctx.qual(local);
        self.push_anchor(ctx, AnchorKind::Law, id.clone(), local);
        self.diffs.push(CanonDiff::AddLaw(Law {
            id,
            when: Pred::SelfIs(Slot::This),
            body: LawBody::Cap {
                mark,
                n,
                require_rel: None,
            },
        }));
    }

    fn rite(&mut self, ctx: &Ctx<'_>, local: &str, emit: &str) {
        let id = ctx.qual(local);
        self.push_anchor(ctx, AnchorKind::Rite, id.clone(), local);
        self.diffs.push(CanonDiff::AddRite(RiteGraph {
            id,
            cap_steps: 8,
            cap_ticks: 32,
            entry: 0,
            nodes: vec![
                RiteNode::Op(RiteOp::Bind(BindSrc::Target)),
                RiteNode::Op(RiteOp::Guard(Pred::SelfIs(Slot::This), 4)),
                RiteNode::Op(RiteOp::Emit(Name::from(emit))),
                RiteNode::Op(RiteOp::Halt(Status::Success)),
                RiteNode::Op(RiteOp::Halt(Status::Fail)),
            ],
        }));
    }

    fn beat(&mut self, ctx: &Ctx<'_>, local: &str, notes: String) {
        let id = ctx.qual(local);
        self.push_anchor(ctx, AnchorKind::Beat, id.clone(), local);
        self.diffs.push(CanonDiff::AddBeat(Beat { id, notes }));
    }

    fn finish(self) -> Expansion {
        Expansion {
            seed: self.seed,
            canon_diffs: self.diffs,
            minds: self.minds,
            journeys: self.journeys,
            anchors: self.anchors,
            spans: self.spans,
            cost: ExpansionCost {
                predicates: 0,
                rite_steps: 0,
                per_tick: 0,
            },
            grants: self.grants,
        }
    }
}

fn emit_lockable(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let passage = ctx.arg_name("passage")?;
    let key = if ctx.args.contains_key("key") {
        ctx.arg_name("key")?
    } else {
        ctx.arg_name("lever")?
    };
    let locked = ctx.arg_bool("locked_at_start").unwrap_or(true);
    let mut b = Builder::new(ctx);
    b.affordance(ctx, "lockable", &["Use", "Open"], &["Driveable"]);
    let aff = ctx.qual("lockable");
    let rite_id = ctx.qual("unlock");
    let when = Pred::And(
        Box::new(Pred::EqVerb(Verb::Use)),
        Box::new(Pred::Affordance(Slot::Target, aff)),
    );
    let must = Pred::Or(
        Box::new(Pred::ExistsRelated {
            of: Slot::Target,
            rel: Rel::KeyedBy,
            pred: Box::new(Pred::Rel(Slot::Other, Rel::WieldedBy, Slot::This)),
        }),
        Box::new(Pred::RiteActive(rite_id)),
    );
    b.law(ctx, "lock_use", when, must);
    b.rite(ctx, "unlock", "Unlocked");
    b.seed.push(SeedFact::Rel {
        a: passage.clone(),
        rel: Rel::KeyedBy,
        b: key,
    });
    if locked {
        b.seed.push(SeedFact::Rel {
            a: passage.clone(),
            rel: Rel::LockedBy,
            b: passage,
        });
    }
    if ctx.spec.version >= 2 && ctx.arg_bool("consume_key").unwrap_or(false) {
        let consumed = ctx.arg_name("key")?;
        b.seed.push(SeedFact::Qty {
            of: consumed,
            res: Name::from("consume"),
            value: 1,
        });
    }
    Ok(b.finish())
}

fn emit_marker(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let target = ctx.arg_name("target")?;
    let label = ctx.arg_name("label")?;
    let mut b = Builder::new(ctx);
    b.beat(
        ctx,
        "mark",
        format!("{}:{}", label.as_str(), target.as_str()),
    );
    b.rite(ctx, "mark_rite", label.as_str());
    Ok(b.finish())
}

fn emit_traversal(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let _actor = ctx.arg_name("actor")?;
    let fixture = ctx.arg_name("fixture")?;
    let ticks = ctx.arg_i32("ticks")?;
    let mut b = Builder::new(ctx);
    b.law(
        ctx,
        "contract",
        Pred::EqVerb(Verb::Move),
        Pred::AabbNear(
            Slot::This,
            Slot::Name(fixture),
            klotho_core::Mm(ticks.max(1)),
        ),
    );
    b.rite(ctx, "traverse", "Traversed");
    Ok(b.finish())
}

fn emit_combat(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let _actor = ctx.arg_name("actor")?;
    let _target = ctx.arg_name("target")?;
    let window = ctx.arg_i32("window_ticks")?.clamp(1, 30) as u16;
    let mut b = Builder::new(ctx);
    b.affordance(ctx, "exchange", &["Use"], &[]);
    let id = ctx.qual("strike");
    b.push_anchor(ctx, AnchorKind::Rite, id.clone(), "strike");
    b.diffs.push(CanonDiff::AddRite(RiteGraph {
        id,
        cap_steps: 8,
        cap_ticks: window.max(8),
        entry: 0,
        nodes: vec![
            RiteNode::Op(RiteOp::Bind(BindSrc::Target)),
            RiteNode::Op(RiteOp::Wait(window, None)),
            RiteNode::Op(RiteOp::Emit(Name::from("Hit"))),
            RiteNode::Op(RiteOp::Halt(Status::Success)),
        ],
    }));
    Ok(b.finish())
}

fn emit_destructible(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let assembly = ctx.arg_name("assembly")?;
    let hits = ctx.arg_i32("hits")?;
    let mut b = Builder::new(ctx);
    b.affordance(ctx, "destructible", &["Use"], &[]);
    b.seed.push(SeedFact::Qty {
        of: assembly,
        res: Name::from("hits"),
        value: hits,
    });
    b.rite(ctx, "collapse", "Collapsed");
    Ok(b.finish())
}

fn emit_boundary(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let place = ctx.arg_name("place")?;
    let envelope = ctx.arg_name("envelope")?;
    let mut b = Builder::new(ctx);
    if !ctx.locus_exists(&envelope) {
        b.seed.push(SeedFact::Locus {
            name: envelope.clone(),
            kind: LocusKind::Place,
        });
        b.push_anchor(ctx, AnchorKind::Locus, envelope.clone(), "envelope");
    }
    b.seed.push(SeedFact::Rel {
        a: envelope,
        rel: Rel::In,
        b: place,
    });
    b.beat(ctx, "encounter", ctx.spec.id.to_owned());
    b.rite(ctx, "boundary", "Entered");
    Ok(b.finish())
}

fn emit_mind(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let actor = ctx.arg_name("actor")?;
    let mut b = Builder::new(ctx);
    let facts: Vec<MindFact> = ctx
        .spec
        .journeys
        .iter()
        .map(|id| MindFact {
            id: Name(format!("{id}_done")),
            query: MindQuery::Never,
            far_safe: false,
        })
        .collect();
    let operators: Vec<MindOperator> = ctx
        .spec
        .journeys
        .iter()
        .map(|id| MindOperator {
            id: Name::from(*id),
            requires: Vec::new(),
            sets: vec![Name(format!("{id}_done"))],
            clears: Vec::new(),
            cost: 1,
            verb: Verb::Investigate,
            target: MindTarget::None,
        })
        .collect();
    let goals: Vec<MindGoal> = ctx
        .spec
        .journeys
        .iter()
        .enumerate()
        .map(|(i, id)| MindGoal {
            id: Name::from(*id),
            desired: vec![Name(format!("{id}_done"))],
            utility: u16::try_from(ctx.spec.journeys.len() - i).unwrap_or(1),
        })
        .collect();
    b.push_anchor(ctx, AnchorKind::Mind, actor.clone(), "mind");
    b.minds.push(MindSpec {
        locus: actor,
        program: MindProgram {
            beat: Some(Name::from(ctx.spec.id)),
            facts,
            operators,
            goals,
            far: Vec::new(),
        },
        templates: Vec::new(),
    });
    b.rite(ctx, "policy", "Acted");
    Ok(b.finish())
}

fn emit_quest(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let actor = ctx.arg_name("actor")?;
    let item = ctx.arg_name("item")?;
    let mut b = Builder::new(ctx);
    b.seed.push(SeedFact::Rel {
        a: actor.clone(),
        rel: Rel::Knows,
        b: item.clone(),
    });
    b.beat(
        ctx,
        "quest",
        format!("{}:{}", actor.as_str(), item.as_str()),
    );
    b.rite(ctx, "advance", "Advanced");
    Ok(b.finish())
}

fn emit_narrative(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let actor = ctx.arg_name("actor")?;
    let topic = ctx.arg_name("topic")?;
    let mut b = Builder::new(ctx);
    b.beat(ctx, "beat", topic.0.clone());
    if ctx.locus_exists(&actor)
        && ctx.module.body.seed.iter().any(
            |f| matches!(f, SeedFact::Locus { name, kind: LocusKind::Actor } if name == &actor),
        )
        && !ctx.module.body.minds.iter().any(|m| m.locus == actor)
    {
        b.push_anchor(ctx, AnchorKind::Mind, actor.clone(), "voice");
        b.minds.push(MindSpec {
            locus: actor,
            program: MindProgram {
                beat: Some(Name::from("conversation")),
                facts: vec![MindFact {
                    id: Name::from("talked"),
                    query: MindQuery::Never,
                    far_safe: false,
                }],
                operators: vec![MindOperator {
                    id: Name::from("talk"),
                    requires: Vec::new(),
                    sets: vec![Name::from("talked")],
                    clears: Vec::new(),
                    cost: 1,
                    verb: Verb::Talk,
                    target: MindTarget::None,
                }],
                goals: vec![MindGoal {
                    id: Name::from("talk"),
                    desired: vec![Name::from("talked")],
                    utility: 1,
                }],
                far: Vec::new(),
            },
            templates: vec![format!("{{name}} : {}", topic.as_str())],
        });
    }
    b.rite(ctx, "play", "Played");
    Ok(b.finish())
}

fn emit_place(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let place = ctx.arg_name("place")?;
    let mut b = Builder::new(ctx);
    if !ctx.locus_exists(&place) {
        b.seed.push(SeedFact::Locus {
            name: place.clone(),
            kind: LocusKind::Place,
        });
        b.push_anchor(ctx, AnchorKind::Locus, place.clone(), "place");
    }
    b.beat(ctx, "shell", place.0.clone());
    b.rite(ctx, "enter", "Entered");
    Ok(b.finish())
}

fn emit_zone(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let place = ctx.arg_name("place")?;
    let tag = ctx.arg_name("tag").unwrap_or_else(|_| Name::from("zone"));
    let mut b = Builder::new(ctx);
    b.beat(ctx, "zone", format!("{}:{}", place.as_str(), tag.as_str()));
    b.rite(ctx, "zone_rite", tag.as_str());
    Ok(b.finish())
}

fn emit_ui(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let actor = ctx.arg_name("actor")?;
    let action = ctx.arg_name("action")?;
    let mut b = Builder::new(ctx);
    b.seed.push(SeedFact::Rel {
        a: actor,
        rel: Rel::Knows,
        b: action.clone(),
    });
    let (beat, emit) = match ctx.spec.id {
        "ui.remappable_action" => ("remap", "Rebound"),
        "ui.subtitle_cue" => ("subtitle", "Captioned"),
        "ui.hold_toggle" => ("hold", "Toggled"),
        "ui.contrast_variant" => ("contrast", "Contrasted"),
        "ui.text_scale" => ("scale", "Scaled"),
        "ui.screen_reader" => ("reader", "Spoken"),
        "ui.motion_reduction" => ("motion", "Reduced"),
        "ui.menu_focus" => ("focus", "Focused"),
        _ => ("cue", "Shown"),
    };
    b.beat(ctx, beat, action.0.clone());
    b.rite(ctx, "prompt", emit);
    Ok(b.finish())
}

fn emit_feel(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let actor = ctx.arg_name("actor")?;
    let mut b = Builder::new(ctx);
    match ctx.spec.id {
        "feel.action_contract" => {
            let action = ctx.arg_name("action")?;
            let recovery = ctx.arg_i32("recovery_wait")?.clamp(1, 32) as u16;
            qty(&mut b, &actor, "buffer_ticks", ctx.arg_i32("buffer_ticks")?);
            qty(&mut b, &actor, "coyote_ticks", ctx.arg_i32("coyote_ticks")?);
            qty(&mut b, &actor, "recovery_wait", i32::from(recovery));
            qty(
                &mut b,
                &actor,
                "hit_stop_present",
                ctx.arg_i32("hit_stop_present")?,
            );
            b.beat(
                ctx,
                "feel",
                format!("{}:{}", actor.as_str(), action.as_str()),
            );
            let id = ctx.qual("recover");
            b.push_anchor(ctx, AnchorKind::Rite, id.clone(), "recover");
            b.diffs.push(CanonDiff::AddRite(RiteGraph {
                id,
                cap_steps: 8,
                cap_ticks: recovery.max(8),
                entry: 0,
                nodes: vec![
                    RiteNode::Op(RiteOp::Bind(BindSrc::Target)),
                    RiteNode::Op(RiteOp::Wait(recovery, None)),
                    RiteNode::Op(RiteOp::Emit(Name::from("Recovered"))),
                    RiteNode::Op(RiteOp::Halt(Status::Success)),
                ],
            }));
        }
        "feel.camera_response" => {
            qty(
                &mut b,
                &actor,
                "smoothing_ticks",
                ctx.arg_i32("smoothing_ticks")?,
            );
            qty(
                &mut b,
                &actor,
                "follow_stiffness",
                ctx.arg_i32("follow_stiffness")?,
            );
            qty(&mut b, &actor, "shake_amp_mm", ctx.arg_i32("shake_amp_mm")?);
            qty(&mut b, &actor, "shake_cap_mm", ctx.arg_i32("shake_cap_mm")?);
            qty(
                &mut b,
                &actor,
                "hull_radius_mm",
                ctx.arg_i32("hull_radius_mm")?,
            );
            b.beat(ctx, "camera", actor.0.clone());
            b.rite(ctx, "follow", "Followed");
        }
        "feel.aim_assist" => {
            qty(
                &mut b,
                &actor,
                "magnet_permille",
                ctx.arg_i32("magnet_permille")?,
            );
            qty(&mut b, &actor, "cone_md", ctx.arg_i32("cone_md")?);
            qty(
                &mut b,
                &actor,
                "max_correction_md",
                ctx.arg_i32("max_correction_md")?,
            );
            b.beat(ctx, "aim", actor.0.clone());
            b.rite(ctx, "magnet", "Magnet");
        }
        "feel.haptic_cue" => {
            let haptic = ctx.arg_name("haptic")?;
            b.beat(
                ctx,
                "haptic",
                format!("{}:{}", actor.as_str(), haptic.as_str()),
            );
            b.rite(ctx, "rumble", "Played");
        }
        _ => {
            let action = ctx.arg_name("action")?;
            qty(
                &mut b,
                &actor,
                "reduce_shake",
                i32::from(ctx.arg_bool("reduce_shake").unwrap_or(false)),
            );
            qty(
                &mut b,
                &actor,
                "reduce_haptics",
                i32::from(ctx.arg_bool("reduce_haptics").unwrap_or(false)),
            );
            qty(
                &mut b,
                &actor,
                "hold_to_toggle",
                i32::from(ctx.arg_bool("hold_to_toggle").unwrap_or(false)),
            );
            qty(
                &mut b,
                &actor,
                "aim_assist_required",
                i32::from(ctx.arg_bool("aim_assist_required").unwrap_or(false)),
            );
            b.beat(
                ctx,
                "access",
                format!("{}:{}", actor.as_str(), action.as_str()),
            );
            b.rite(ctx, "access_rite", "Accessible");
        }
    }
    Ok(b.finish())
}

fn qty(b: &mut Builder, of: &Name, res: &str, value: i32) {
    b.seed.push(SeedFact::Qty {
        of: of.clone(),
        res: Name::from(res),
        value,
    });
}

fn emit_perf(ctx: &Ctx<'_>) -> Result<Expansion, PatternError> {
    let place = ctx.arg_name("place")?;
    let budget = ctx.arg_i32("budget_ms")?.clamp(1, 64) as u16;
    let mut b = Builder::new(ctx);
    b.cap_law(
        ctx,
        "perf_cap",
        Pred::InPlace(Slot::This, Slot::Name(place.clone())),
        budget,
    );
    b.beat(ctx, "perf", place.0);
    b.rite(ctx, "sample", "Sampled");
    Ok(b.finish())
}

fn measure(expansion: &Expansion) -> ExpansionCost {
    let mut predicates = 0u32;
    let mut rite_steps = 0u32;
    let mut per_tick = 0u32;
    for diff in &expansion.canon_diffs {
        match diff {
            CanonDiff::AddLaw(law) => {
                predicates = predicates.saturating_add(count_pred(&law.when));
                match &law.body {
                    LawBody::Pred { must, .. } => {
                        predicates = predicates.saturating_add(count_pred(must));
                    }
                    LawBody::Cap { mark, .. } => {
                        predicates = predicates.saturating_add(count_pred(mark));
                    }
                    LawBody::Ramp { .. } | LawBody::Spread { .. } => per_tick += 1,
                    LawBody::Conserve { .. } => {}
                }
            }
            CanonDiff::AddRite(rite) => {
                rite_steps = rite_steps.saturating_add(rite.nodes.len() as u32);
                for node in &rite.nodes {
                    let op = match node {
                        RiteNode::Op(op) | RiteNode::Labeled { op, .. } => op,
                    };
                    if let RiteOp::Guard(p, _) | RiteOp::Branch(p, _, _) = op {
                        predicates = predicates.saturating_add(count_pred(p));
                    }
                }
            }
            _ => {}
        }
    }
    ExpansionCost {
        predicates,
        rite_steps,
        per_tick,
    }
}

fn count_pred(p: &Pred) -> u32 {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => 1 + count_pred(a) + count_pred(b),
        Pred::Not(a) => 1 + count_pred(a),
        Pred::ExistsRelated { pred, .. } | Pred::CountRelated { pred, .. } => 1 + count_pred(pred),
        _ => 1,
    }
}
