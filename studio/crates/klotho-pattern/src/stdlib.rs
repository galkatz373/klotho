//! First-title standard library. Capability-composed, not genre inheritance.

use klotho_ir::ParameterType;

use crate::def::{ExpandKind, ParamSpec, PatternBudget, PatternFamily, PatternSpec, StaticValue};

const NAME: ParameterType = ParameterType::Name;
const I32: ParameterType = ParameterType::I32;
const BOOL: ParameterType = ParameterType::Bool;

const fn n(name: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        ty: NAME,
        default: None,
    }
}

const fn n_d(name: &'static str, default: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        ty: NAME,
        default: Some(StaticValue::Name(default)),
    }
}

const fn i_d(name: &'static str, default: i32) -> ParamSpec {
    ParamSpec {
        name,
        ty: I32,
        default: Some(StaticValue::I32(default)),
    }
}

const fn b_d(name: &'static str, default: bool) -> ParamSpec {
    ParamSpec {
        name,
        ty: BOOL,
        default: Some(StaticValue::Bool(default)),
    }
}

const fn budget(predicates: u32, rite_steps: u32) -> PatternBudget {
    PatternBudget {
        predicates,
        rite_steps,
        per_tick: 0,
    }
}

macro_rules! spec_from {
    () => {
        None
    };
    ($v:expr) => {
        Some($v)
    };
}

macro_rules! spec {
    (
        $id:expr, $ver:expr, $fam:expr, $kind:expr,
        params: $params:expr,
        req: $req:expr,
        grants: $grants:expr,
        conflicts: $conflicts:expr,
        budget: $budget:expr,
        journeys: $journeys:expr
        $(, from: $from:expr)?
    ) => {
        PatternSpec {
            id: $id,
            version: $ver,
            family: $fam,
            params: $params,
            requires: $req,
            grants: $grants,
            conflicts: $conflicts,
            budget: $budget,
            journeys: $journeys,
            kind: $kind,
            from_version: spec_from!($($from)?),
        }
    };
}

/// Every published `(id, version)` including migrations.
#[must_use]
pub fn specs() -> &'static [PatternSpec] {
    const DOOR_KEY_V1_PARAMS: &[ParamSpec] = &[
        n("passage"),
        n("key"),
        b_d("locked_at_start", true),
        n_d("denied_bark", "locked"),
    ];
    const DOOR_KEY_V2_PARAMS: &[ParamSpec] = &[
        n("passage"),
        n("key"),
        b_d("locked_at_start", true),
        b_d("consume_key", true),
        n_d("denied_bark", "locked"),
    ];
    const LEVER_PARAMS: &[ParamSpec] = &[
        n("passage"),
        n("lever"),
        b_d("locked_at_start", true),
        n_d("denied_bark", "stuck"),
    ];
    const MARKER_PARAMS: &[ParamSpec] = &[n("target"), n_d("label", "mark")];
    const TRAVERSAL_PARAMS: &[ParamSpec] = &[n("actor"), n("fixture"), i_d("ticks", 4)];
    const COMBAT_PARAMS: &[ParamSpec] = &[n("actor"), n("target"), i_d("window_ticks", 6)];
    const DESTRUCT_PARAMS: &[ParamSpec] = &[n("assembly"), i_d("hits", 3)];
    const BOUNDARY_PARAMS: &[ParamSpec] = &[n("place"), n("envelope")];
    const MIND_PARAMS: &[ParamSpec] = &[n("actor")];
    const QUEST_PARAMS: &[ParamSpec] = &[n("actor"), n("item")];
    const NARRATIVE_PARAMS: &[ParamSpec] = &[n("actor"), n("topic")];
    const PLACE_PARAMS: &[ParamSpec] = &[n("place")];
    const ZONE_PARAMS: &[ParamSpec] = &[n("place"), n_d("tag", "zone")];
    const UI_PARAMS: &[ParamSpec] = &[n("actor"), n("action")];
    const PERF_PARAMS: &[ParamSpec] = &[n("place"), i_d("budget_ms", 8)];

    const S: &[PatternSpec] = &[
        spec!(
            "traversal.door_key", 1, PatternFamily::Traversal, ExpandKind::LockablePassage,
            params: DOOR_KEY_V1_PARAMS,
            req: &[("passage", "Openable"), ("key", "Carryable")],
            grants: &[("passage", "Lockable")],
            conflicts: &[("passage", "Driveable")],
            budget: budget(16, 8),
            journeys: &["unlocked", "opened", "denied"]
        ),
        spec!(
            "traversal.door_key", 2, PatternFamily::Traversal, ExpandKind::LockablePassage,
            params: DOOR_KEY_V2_PARAMS,
            req: &[("passage", "Openable"), ("key", "Carryable")],
            grants: &[("passage", "Lockable")],
            conflicts: &[("passage", "Driveable")],
            budget: budget(16, 8),
            journeys: &["unlocked", "opened", "denied", "consumed"],
            from: 1
        ),
        spec!(
            "traversal.lever_gate", 1, PatternFamily::Traversal, ExpandKind::LockablePassage,
            params: LEVER_PARAMS,
            req: &[("passage", "Openable"), ("lever", "Usable")],
            grants: &[("passage", "Lockable")],
            conflicts: &[("passage", "Driveable")],
            budget: budget(16, 8),
            journeys: &["unlocked", "opened", "denied"]
        ),
        spec!(
            "traversal.checkpoint", 1, PatternFamily::Traversal, ExpandKind::Marker,
            params: MARKER_PARAMS,
            req: &[("target", "Placeable")],
            grants: &[("target", "Checkpoint")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["reached", "restored"]
        ),
        spec!(
            "traversal.ladder_ledge", 1, PatternFamily::Traversal, ExpandKind::TraversalContract,
            params: TRAVERSAL_PARAMS,
            req: &[("actor", "Mobile"), ("fixture", "Climbable")],
            grants: &[("fixture", "Ledge")],
            conflicts: &[("fixture", "Driveable")],
            budget: budget(8, 6),
            journeys: &["mount", "dismount", "timeout"]
        ),
        spec!(
            "traversal.streaming_threshold", 1, PatternFamily::Traversal, ExpandKind::TraversalContract,
            params: TRAVERSAL_PARAMS,
            req: &[("actor", "Mobile"), ("fixture", "Placeable")],
            grants: &[("fixture", "StreamGate")],
            conflicts: &[],
            budget: budget(8, 6),
            journeys: &["cross", "hold", "rollback"]
        ),
        spec!(
            "combat.light_heavy", 1, PatternFamily::Combat, ExpandKind::CombatExchange,
            params: COMBAT_PARAMS,
            req: &[("actor", "Armed"), ("target", "Hittable")],
            grants: &[("actor", "Melee")],
            conflicts: &[],
            budget: budget(8, 8),
            journeys: &["hit", "miss", "recover"]
        ),
        spec!(
            "combat.parry_window", 1, PatternFamily::Combat, ExpandKind::CombatExchange,
            params: COMBAT_PARAMS,
            req: &[("actor", "Armed"), ("target", "Hittable")],
            grants: &[("actor", "Parry")],
            conflicts: &[],
            budget: budget(8, 8),
            journeys: &["parry", "whiff", "punish"]
        ),
        spec!(
            "combat.ranged_hit", 1, PatternFamily::Combat, ExpandKind::CombatExchange,
            params: COMBAT_PARAMS,
            req: &[("actor", "Armed"), ("target", "Hittable")],
            grants: &[("actor", "Ranged")],
            conflicts: &[],
            budget: budget(8, 8),
            journeys: &["hit", "miss", "reload"]
        ),
        spec!(
            "combat.destructible", 1, PatternFamily::Combat, ExpandKind::Destructible,
            params: DESTRUCT_PARAMS,
            req: &[("assembly", "Hittable")],
            grants: &[("assembly", "Destructible")],
            conflicts: &[],
            budget: budget(8, 6),
            journeys: &["damaged", "collapsed"]
        ),
        spec!(
            "combat.encounter_boundary", 1, PatternFamily::Combat, ExpandKind::EncounterBoundary,
            params: BOUNDARY_PARAMS,
            req: &[("place", "Placeable")],
            grants: &[("place", "Encounter")],
            conflicts: &[("place", "SafeHub")],
            budget: budget(6, 8),
            journeys: &["enter", "exit", "complete"]
        ),
        spec!(
            "ai.patrol_investigate", 1, PatternFamily::Ai, ExpandKind::MindPolicy,
            params: MIND_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("actor", "Patrol")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["patrol", "investigate", "return"]
        ),
        spec!(
            "ai.guard_chase_return", 1, PatternFamily::Ai, ExpandKind::MindPolicy,
            params: MIND_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("actor", "Guard")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["guard", "chase", "return"]
        ),
        spec!(
            "ai.assist_ally", 1, PatternFamily::Ai, ExpandKind::MindPolicy,
            params: MIND_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("actor", "Assist")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["assist", "idle", "abort"]
        ),
        spec!(
            "ai.flee_hazard", 1, PatternFamily::Ai, ExpandKind::MindPolicy,
            params: MIND_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("actor", "Flee")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["flee", "hide", "resume"]
        ),
        spec!(
            "ai.conversation_availability", 1, PatternFamily::Ai, ExpandKind::MindPolicy,
            params: MIND_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("actor", "Talkable")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["available", "busy", "done"]
        ),
        spec!(
            "quest.acquire_use", 1, PatternFamily::Quest, ExpandKind::QuestStep,
            params: QUEST_PARAMS,
            req: &[("actor", "Minded"), ("item", "Carryable")],
            grants: &[("item", "Objective")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["offered", "acquired", "used"]
        ),
        spec!(
            "quest.escort_checkpoints", 1, PatternFamily::Quest, ExpandKind::QuestStep,
            params: QUEST_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("actor", "Escort")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["depart", "checkpoint", "arrive"]
        ),
        spec!(
            "quest.investigate_clues", 1, PatternFamily::Quest, ExpandKind::QuestStep,
            params: QUEST_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("item", "Clue")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["found", "linked", "concluded"]
        ),
        spec!(
            "quest.handoff", 1, PatternFamily::Quest, ExpandKind::QuestStep,
            params: QUEST_PARAMS,
            req: &[("actor", "Minded"), ("item", "Carryable")],
            grants: &[("item", "Handoff")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["offered", "accepted", "completed"]
        ),
        spec!(
            "quest.optional_objective", 1, PatternFamily::Quest, ExpandKind::QuestStep,
            params: QUEST_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("item", "Optional")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["noticed", "completed", "skipped"]
        ),
        spec!(
            "narrative.conditional_conversation", 1, PatternFamily::Narrative, ExpandKind::Narrative,
            params: NARRATIVE_PARAMS,
            req: &[("actor", "Talkable")],
            grants: &[("actor", "Dialogue")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["available", "played", "skipped"]
        ),
        spec!(
            "narrative.bark_set", 1, PatternFamily::Narrative, ExpandKind::Narrative,
            params: NARRATIVE_PARAMS,
            req: &[("actor", "Talkable")],
            grants: &[("actor", "Bark")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["idle", "combat", "done"]
        ),
        spec!(
            "narrative.cinematic_beat", 1, PatternFamily::Narrative, ExpandKind::Narrative,
            params: NARRATIVE_PARAMS,
            req: &[("actor", "Placeable")],
            grants: &[("topic", "Cinematic")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["queued", "played", "skipped"]
        ),
        spec!(
            "narrative.knowledge_reveal", 1, PatternFamily::Narrative, ExpandKind::Narrative,
            params: NARRATIVE_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("topic", "Known")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["hidden", "revealed", "recalled"]
        ),
        spec!(
            "narrative.lore_entry", 1, PatternFamily::Narrative, ExpandKind::Narrative,
            params: NARRATIVE_PARAMS,
            req: &[("topic", "Placeable")],
            grants: &[("topic", "Lore")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["unread", "read", "complete"]
        ),
        spec!(
            "world.place_shell", 1, PatternFamily::World, ExpandKind::PlaceShell,
            params: PLACE_PARAMS,
            req: &[],
            grants: &[("place", "Placeable")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["loaded", "entered", "streamed_out"]
        ),
        spec!(
            "world.traversal_graph", 1, PatternFamily::World, ExpandKind::Zone,
            params: ZONE_PARAMS,
            req: &[("place", "Placeable")],
            grants: &[("place", "Graph")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["linked", "reachable", "blocked"]
        ),
        spec!(
            "world.encounter_pocket", 1, PatternFamily::World, ExpandKind::EncounterBoundary,
            params: BOUNDARY_PARAMS,
            req: &[("place", "Placeable")],
            grants: &[("place", "Pocket")],
            conflicts: &[("place", "SafeHub")],
            budget: budget(6, 8),
            journeys: &["enter", "exit", "complete"]
        ),
        spec!(
            "world.safe_hub", 1, PatternFamily::World, ExpandKind::Zone,
            params: ZONE_PARAMS,
            req: &[("place", "Placeable")],
            grants: &[("place", "SafeHub")],
            conflicts: &[("place", "Encounter")],
            budget: budget(4, 8),
            journeys: &["enter", "rest", "leave"]
        ),
        spec!(
            "world.dressing_zone", 1, PatternFamily::World, ExpandKind::Zone,
            params: ZONE_PARAMS,
            req: &[("place", "Placeable")],
            grants: &[("place", "Dressing")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["placed", "culled"]
        ),
        spec!(
            "world.audio_zone", 1, PatternFamily::World, ExpandKind::Zone,
            params: ZONE_PARAMS,
            req: &[("place", "Placeable")],
            grants: &[("place", "AudioZone")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["enter", "leave"]
        ),
        spec!(
            "ui.knows_gated_prompt", 1, PatternFamily::Ui, ExpandKind::UiCue,
            params: UI_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("action", "Prompt")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["shown", "accepted", "dismissed"]
        ),
        spec!(
            "ui.remappable_action", 1, PatternFamily::Ui, ExpandKind::UiCue,
            params: UI_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("action", "Remap")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["bound", "rebound", "cleared"]
        ),
        spec!(
            "ui.subtitle_cue", 1, PatternFamily::Ui, ExpandKind::UiCue,
            params: UI_PARAMS,
            req: &[("actor", "Talkable")],
            grants: &[("action", "Subtitle")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["shown", "hidden"]
        ),
        spec!(
            "ui.hold_toggle", 1, PatternFamily::Ui, ExpandKind::UiCue,
            params: UI_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("action", "HoldToggle")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["hold", "toggle"]
        ),
        spec!(
            "ui.contrast_variant", 1, PatternFamily::Ui, ExpandKind::UiCue,
            params: UI_PARAMS,
            req: &[("actor", "Minded")],
            grants: &[("action", "Contrast")],
            conflicts: &[],
            budget: budget(6, 8),
            journeys: &["default", "high"]
        ),
        spec!(
            "production.save_checkpoint", 1, PatternFamily::Production, ExpandKind::Marker,
            params: MARKER_PARAMS,
            req: &[("target", "Placeable")],
            grants: &[("target", "Save")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["saved", "loaded"]
        ),
        spec!(
            "production.analytics_marker", 1, PatternFamily::Production, ExpandKind::Marker,
            params: MARKER_PARAMS,
            req: &[("target", "Placeable")],
            grants: &[("target", "Analytics")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["fired"]
        ),
        spec!(
            "production.screenshot_marker", 1, PatternFamily::Production, ExpandKind::Marker,
            params: MARKER_PARAMS,
            req: &[("target", "Placeable")],
            grants: &[("target", "Capture")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["captured"]
        ),
        spec!(
            "production.journey_fixture", 1, PatternFamily::Production, ExpandKind::Marker,
            params: MARKER_PARAMS,
            req: &[("target", "Placeable")],
            grants: &[("target", "Journey")],
            conflicts: &[],
            budget: budget(4, 8),
            journeys: &["start", "assert", "end"]
        ),
        spec!(
            "production.performance_encounter", 1, PatternFamily::Production, ExpandKind::ProductionEncounter,
            params: PERF_PARAMS,
            req: &[("place", "Placeable")],
            grants: &[("place", "PerfProbe")],
            conflicts: &[],
            budget: budget(8, 8),
            journeys: &["start", "budget_ok", "over_budget"]
        ),
    ];
    S
}

/// Latest published version of `id`, if any.
#[must_use]
pub fn latest(id: &str) -> Option<&'static PatternSpec> {
    specs()
        .iter()
        .filter(|s| s.id == id)
        .max_by_key(|s| s.version)
}

/// Exact `(id, version)` row.
#[must_use]
pub fn lookup(id: &str, version: u32) -> Option<&'static PatternSpec> {
    specs().iter().find(|s| s.id == id && s.version == version)
}

/// First-title pattern ids, unique, sorted. `traversal.door_key` is one id.
#[must_use]
pub fn first_pattern_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = specs().iter().map(|s| s.id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}
