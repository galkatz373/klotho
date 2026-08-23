//! Canonical RON (de)serialization.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::IrError;

/// Parse canonical RON into `T`.
///
/// Also accepts Appendix A rite sugar: bare ops in `nodes` lists and
/// `{ pc: N, op: ... }` maps. Those are rewritten to `Op(...)` / `Labeled(...)`
/// before serde.
pub fn from_ron<T: DeserializeOwned>(s: &str) -> Result<T, IrError> {
    let normalized = normalize_rite_ron(s);
    match ron::from_str(&normalized) {
        Ok(v) => Ok(v),
        Err(first) => ron::from_str(s).map_err(|_| IrError::Parse(first.to_string())),
    }
}

/// Rewrite Appendix A rite-node sugar into tagged [`crate::RiteNode`] RON.
pub(crate) fn normalize_rite_ron(s: &str) -> String {
    let labeled = rewrite_pc_maps(s);
    // `Guard(pred, fail: 10)` → `Guard(pred, 10)`.
    let labeled = labeled.replace(", fail: ", ", ");
    let labeled = unquote_rel_names(&labeled);
    wrap_bare_node_items(&labeled)
}

fn unquote_rel_names(s: &str) -> String {
    let mut out = s.to_string();
    for name in [
        "In",
        "OwnedBy",
        "WieldedBy",
        "KeyedBy",
        "Knows",
        "Owes",
        "Fears",
        "PartOf",
        "DerivedFrom",
        "LockedBy",
        "Dead",
    ] {
        out = out.replace(&format!("\"{name}\""), name);
    }
    out
}

fn rewrite_pc_maps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        if let Some((rewritten, consumed)) = rewrite_one_pc_map(&rest[start..]) {
            out.push_str(&rewritten);
            rest = &rest[start + consumed..];
        } else {
            out.push('{');
            rest = &rest[start + 1..];
        }
    }
    out.push_str(rest);
    out
}

fn rewrite_one_pc_map(s: &str) -> Option<(String, usize)> {
    let after = s.strip_prefix('{')?.trim_start();
    let after = after.strip_prefix("pc:")?.trim_start();
    let digits = after
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, c)| i + c.len_utf8())?;
    let pc = &after[..digits];
    let after = after[digits..].trim_start().strip_prefix(',')?.trim_start();
    let after = after.strip_prefix("op:")?.trim_start();
    let op_len = scan_until(after, '}')?;
    let op = after[..op_len].trim();
    let after_op = after[op_len..].trim_start();
    let after_brace = after_op.strip_prefix('}')?;
    let consumed = s.len() - after_brace.len();
    Some((format!("Labeled(pc: {pc}, op: {op})"), consumed))
}

fn scan_until(s: &str, until: char) -> Option<usize> {
    let mut round = 0i32;
    let mut square = 0i32;
    for (i, c) in s.char_indices() {
        if c == until && round == 0 && square == 0 {
            return Some(i);
        }
        match c {
            '(' => round += 1,
            ')' => round -= 1,
            '[' => square += 1,
            ']' => square -= 1,
            _ => {}
        }
        if round < 0 || square < 0 {
            return None;
        }
    }
    None
}

fn wrap_bare_node_items(s: &str) -> String {
    let key = "nodes:";
    let mut out = String::with_capacity(s.len() + 32);
    let mut rest = s;
    while let Some(pos) = rest.find(key) {
        out.push_str(&rest[..pos + key.len()]);
        rest = &rest[pos + key.len()..];
        let pad_len = rest.len() - rest.trim_start().len();
        out.push_str(&rest[..pad_len]);
        rest = &rest[pad_len..];
        if !rest.starts_with('[') {
            continue;
        }
        let Some(inner_len) = scan_until(&rest[1..], ']') else {
            continue;
        };
        let inner = &rest[1..1 + inner_len];
        out.push('[');
        out.push_str(&wrap_node_list(inner));
        out.push(']');
        rest = &rest[1 + inner_len + 1..];
    }
    out.push_str(rest);
    out
}

fn wrap_node_list(inner: &str) -> String {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&inner[start..]);
    items
        .into_iter()
        .map(wrap_one_node)
        .collect::<Vec<_>>()
        .join(",")
}

fn wrap_one_node(item: &str) -> String {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return item.to_string();
    }
    if trimmed.starts_with("Op(") || trimmed.starts_with("Labeled(") {
        return item.to_string();
    }
    let leading = &item[..item.len() - item.trim_start().len()];
    let trailing = &item[item.trim_end().len()..];
    format!("{leading}Op({trimmed}){trailing}")
}

/// Pretty-print `T` as canonical RON with struct names.
pub fn to_ron<T: Serialize>(value: &T) -> Result<String, IrError> {
    let cfg = ron::ser::PrettyConfig::new().struct_names(true);
    ron::ser::to_string_pretty(value, cfg).map_err(|e| IrError::Ser(e.to_string()))
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, PlayerId, Sigil, Tick};
    use klotho_prove::{Hash, ProvenanceId};

    use super::*;
    use crate::agency::{Agency, AssistLevel, Channel};
    use crate::analog::Analog;
    use crate::decl::{Affordance, CanonDiff, Law, LawBody, RiteGraph, RiteNode, RiteOp, Status};
    use crate::doc::IntentDoc;
    use crate::infer::{FactId, InferIntent, ModelId};
    use crate::mind::{MindIntent, MindSpec};
    use crate::name::Name;
    use crate::player::PlayerIntent;
    use crate::pred::{Cmp, Pred};
    use crate::rel::Rel;
    use crate::seed::SeedFact;
    use crate::style::StyleIntent;
    use crate::target::{IntentTarget, Slot};
    use crate::validate::validate_doc;
    use crate::verb::Verb;

    fn name(s: &str) -> Name {
        Name::new(s).unwrap()
    }

    fn player_use_door() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(0),
            at: Tick(12),
            verb: Verb::Use,
            target: IntentTarget::Name(name("oak_door")),
            analog: Analog {
                phase: 400,
                ..Analog::default()
            },
            agency: Agency {
                claimed: vec![Channel::Timing],
                assist: AssistLevel::None,
            },
        }
    }

    #[test]
    fn player_intent_ron_round_trip() {
        let p = player_use_door();
        p.validate().unwrap();
        let text = to_ron(&p).unwrap();
        let q: PlayerIntent = from_ron(&text).unwrap();
        assert_eq!(p, q);
        assert!(text.contains("Use"));
        assert!(text.contains("Timing"));
        assert!(text.contains("oak_door"));
    }

    #[test]
    fn player_intent_parses_handwritten_ron() {
        let src = r#"
PlayerIntent(
    player: PlayerId(0),
    at: Tick(12),
    verb: Use,
    target: Name("oak_door"),
    analog: Analog(
        phase: 400,
        stick_x: 0,
        stick_z: 0,
        look_yaw: YawMd(0),
        look_pitch: 0,
    ),
    agency: Agency(
        claimed: [Timing],
        assist: None,
    ),
)
"#;
        let q: PlayerIntent = from_ron(src).unwrap();
        assert_eq!(q, player_use_door());
    }

    #[test]
    fn infer_rejects_agency_field() {
        let src = r#"
InferIntent(
    model: "dialogue-fill",
    locus: None,
    verb: Talk,
    target: None,
    claimed_facts: [],
    agency: Agency(claimed: [Timing], assist: None),
)
"#;
        let err = from_ron::<InferIntent>(src).unwrap_err();
        match err {
            crate::IrError::Parse(_) => {}
            other => panic!("expected parse error, got {other}"),
        }
    }

    #[test]
    fn infer_intent_round_trip() {
        let i = InferIntent {
            model: ModelId(name("dialogue-fill")),
            locus: None,
            verb: Verb::Talk,
            target: IntentTarget::None,
            claimed_facts: vec![FactId(name("kel_is_here"))],
        };
        i.validate().unwrap();
        let q: InferIntent = from_ron(&to_ron(&i).unwrap()).unwrap();
        assert_eq!(i, q);
    }

    #[test]
    fn mind_intent_round_trip() {
        let m = MindIntent {
            locus: Sigil::pack(LocusKind::Actor, 0, 2).unwrap(),
            verb: Verb::Investigate,
            target: IntentTarget::Name(name("oak_door")),
            utility: 10,
        };
        m.validate().unwrap();
        let q: MindIntent = from_ron(&to_ron(&m).unwrap()).unwrap();
        assert_eq!(m, q);
    }

    #[test]
    fn nested_quantifier_is_rejected() {
        let inner = Pred::ExistsRelated {
            of: Slot::Target,
            rel: Rel::KeyedBy,
            pred: Box::new(Pred::Rel(Slot::Other, Rel::WieldedBy, Slot::This)),
        };
        let outer = Pred::ExistsRelated {
            of: Slot::This,
            rel: Rel::In,
            pred: Box::new(inner),
        };
        assert_eq!(outer.check(), Err(crate::IrError::NestedQuantifier));
    }

    #[test]
    fn empty_name_is_rejected() {
        let a = Affordance {
            id: Name(String::new()),
            requires: vec![],
            grants: vec![],
            conflicts: vec![],
        };
        assert_eq!(a.check(), Err(crate::IrError::EmptyName));
    }

    #[test]
    fn zero_rite_cap_is_rejected() {
        let r = RiteGraph {
            id: name("lockpick"),
            cap_steps: 0,
            cap_ticks: 180,
            entry: 0,
            nodes: vec![RiteNode::Op(RiteOp::Complete(Status::Fail))],
        };
        assert_eq!(r.check(), Err(crate::IrError::InvalidRiteCap));
    }

    #[test]
    fn duplicate_channel_is_rejected() {
        let p = PlayerIntent {
            agency: Agency {
                claimed: vec![Channel::Timing, Channel::Timing],
                assist: AssistLevel::None,
            },
            ..player_use_door()
        };
        assert_eq!(p.validate(), Err(crate::IrError::DuplicateChannel));
    }

    fn portable_affordance() -> Affordance {
        Affordance {
            id: name("Portable"),
            requires: vec![Pred::Qty(Slot::This, name("mass_g"), Cmp::Lt, 40000)],
            grants: vec![name("Carry")],
            conflicts: vec![],
        }
    }

    fn carry_mass_law() -> Law {
        Law {
            id: name("carry.mass"),
            when: Pred::Or(
                Box::new(Pred::EqVerb(Verb::Carry)),
                Box::new(Pred::EqVerb(Verb::Drop)),
            ),
            body: LawBody::Conserve {
                res: name("mass_g"),
                over: Rel::WieldedBy,
            },
        }
    }

    #[test]
    fn intent_doc_hearth_slice_round_trip() {
        let doc = IntentDoc {
            style: StyleIntent {
                notes: "chunky readable silhouettes".into(),
                palettes: vec![name("stone"), name("metal"), name("organic")],
                kitbash_tags: vec![
                    name("door.oak.lockable"),
                    name("barrel.oak.portable.flammable"),
                ],
            },
            canon_diffs: vec![
                CanonDiff::AddAffordance(portable_affordance()),
                CanonDiff::AddLaw(carry_mass_law()),
                CanonDiff::AddRite(RiteGraph {
                    id: name("ignite"),
                    cap_steps: 8,
                    cap_ticks: 10,
                    entry: 0,
                    nodes: vec![
                        RiteNode::Op(RiteOp::Bind(crate::decl::BindSrc::Target)),
                        RiteNode::Labeled {
                            pc: 5,
                            op: RiteOp::Complete(Status::Success),
                        },
                    ],
                }),
            ],
            seed: vec![
                SeedFact::Locus {
                    name: name("oak_door"),
                    kind: LocusKind::Relic,
                },
                SeedFact::Rel {
                    a: name("oak_door"),
                    rel: Rel::LockedBy,
                    b: name("oak_door"),
                },
                SeedFact::Qty {
                    of: name("barrel_oak"),
                    res: name("mass_g"),
                    value: 12_000,
                },
            ],
            minds: vec![MindSpec {
                locus: name("bran"),
                goals: vec![name("stay_near_forge")],
                templates: vec!["{name} won't sell that.".into()],
            }],
            provenance: ProvenanceId(Hash::ZERO),
        };
        validate_doc(&doc).unwrap();
        let text = to_ron(&doc).unwrap();
        let q: IntentDoc = from_ron(&text).unwrap();
        assert_eq!(doc, q);
        assert!(text.contains("Portable"));
        assert!(text.contains("LockedBy"));
        assert!(text.contains("stay_near_forge"));
    }

    #[test]
    fn handwritten_portable_parses() {
        let src = r#"
AddAffordance(Affordance(
    id: "Portable",
    requires: [Qty(Self, "mass_g", Lt, 40000)],
    grants: ["Carry"],
    conflicts: [],
))
"#;
        let d: CanonDiff = from_ron(src).unwrap();
        d.check().unwrap();
        match d {
            CanonDiff::AddAffordance(a) => {
                assert_eq!(a.id.as_str(), "Portable");
                assert_eq!(a.grants[0].as_str(), "Carry");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn appendix_guard_fail_named_and_string_rel() {
        let src = r#"
AddRite(RiteGraph(
    id: "trade.offer",
    cap_steps: 24,
    cap_ticks: 600,
    entry: 0,
    nodes: [
        { pc: 0, op: Bind(Target) },
        { pc: 1, op: Guard(Not(TargetIs(Name("fathers_hammer"))), fail: 10) },
        { pc: 8, op: RelDel(Target, "LockedBy", Target) },
        { pc: 10, op: Complete(Fail) },
    ],
))
"#;
        let d: CanonDiff = from_ron(src).unwrap();
        match d {
            CanonDiff::AddRite(g) => {
                assert_eq!(g.nodes.len(), 4);
                match &g.nodes[1] {
                    RiteNode::Labeled {
                        pc: 1,
                        op: RiteOp::Guard(_, 10),
                    } => {}
                    other => panic!("{other:?}"),
                }
                match &g.nodes[2] {
                    RiteNode::Labeled {
                        op: RiteOp::RelDel(_, Rel::LockedBy, _),
                        ..
                    } => {}
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unlabeled_bind_parses_as_op() {
        let src = r#"
AddRite(RiteGraph(
    id: "ignite",
    cap_steps: 8,
    cap_ticks: 10,
    entry: 0,
    nodes: [Bind(Target), Complete(Success), Complete(Fail)],
))
"#;
        let d: CanonDiff = from_ron(src).unwrap();
        match d {
            CanonDiff::AddRite(g) => {
                assert!(matches!(g.nodes[0], RiteNode::Op(RiteOp::Bind(_))));
                assert!(matches!(
                    g.nodes[1],
                    RiteNode::Op(RiteOp::Complete(Status::Success))
                ));
            }
            other => panic!("{other:?}"),
        }
    }
}
