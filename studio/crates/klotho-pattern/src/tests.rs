//! Positive, negative, migration, cost, and journey fixtures for every pattern.

use klotho_canon::cook;
use klotho_core::{Hash, LocusKind};
use klotho_ir::{
    AnchorKind, IntentDoc, Name, ParameterValue, PatternArg, PatternInstance, ProvenanceId,
    SeedFact, StyleIntent, migrate_doc, to_ron,
};
use klotho_prove::hash_bytes;

use crate::def::{HostCaps, PatternSpec};
use crate::error::PatternError;
use crate::expand::{expand_bundle, expand_instance, expand_module};
use crate::migrate::migrate_instance;
use crate::stdlib::{first_pattern_ids, lookup, specs};

fn name(s: &str) -> Name {
    Name::from(s)
}

fn empty_doc() -> IntentDoc {
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs: Vec::new(),
        seed: Vec::new(),
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

fn host_for(spec: &PatternSpec) -> klotho_ir::IntentModule {
    let mut seed = Vec::new();
    let mut seen = Vec::new();
    for param in spec.params {
        if param.ty != klotho_ir::ParameterType::Name {
            continue;
        }
        if seen.iter().any(|n: &Name| n.as_str() == param.name) {
            continue;
        }
        let n = name(param.name);
        seen.push(n.clone());
        let kind = if param.name == "actor" {
            LocusKind::Actor
        } else if param.name == "place" || param.name == "envelope" {
            LocusKind::Place
        } else {
            LocusKind::Relic
        };
        seed.push(SeedFact::Locus { name: n, kind });
    }
    let doc = IntentDoc {
        seed,
        ..empty_doc()
    };
    let bundle = migrate_doc(name("spin"), name("main"), doc).unwrap();
    bundle.modules.into_iter().next().unwrap()
}

fn instance_for(spec: &PatternSpec, module_anchor: klotho_ir::AnchorId) -> PatternInstance {
    let mut args = Vec::new();
    for param in spec.params {
        if param.default.is_some() {
            continue;
        }
        let value = match param.ty {
            klotho_ir::ParameterType::Name => ParameterValue::Name(name(param.name)),
            klotho_ir::ParameterType::I32 => ParameterValue::I32(1),
            klotho_ir::ParameterType::Bool => ParameterValue::Bool(true),
            klotho_ir::ParameterType::Anchor => ParameterValue::Anchor(module_anchor),
        };
        args.push(PatternArg {
            key: name(param.name),
            value,
        });
    }
    PatternInstance {
        anchor: module_anchor.child(format!("pattern:{}", spec.id).as_bytes()),
        module: module_anchor,
        instance: name("inst"),
        pattern: name(spec.id),
        version: spec.version,
        args,
    }
}

fn caps_for(spec: &PatternSpec) -> HostCaps {
    let mut caps = HostCaps::new();
    for (param, cap) in spec.requires {
        caps.grant(param, cap);
    }
    caps
}

fn attach(module: &mut klotho_ir::IntentModule, instance: &PatternInstance) {
    module.patterns.push(instance.clone());
    module.object_anchors.push(klotho_ir::ObjectAnchor {
        kind: AnchorKind::Pattern,
        name: instance.instance.clone(),
        anchor: instance.anchor,
    });
}

#[test]
fn stdlib_has_forty_one_first_patterns() {
    let ids = first_pattern_ids();
    assert_eq!(ids.len(), 41, "{ids:?}");
    for family in [
        "traversal.",
        "combat.",
        "ai.",
        "quest.",
        "narrative.",
        "world.",
        "ui.",
        "production.",
    ] {
        assert!(
            ids.iter().any(|id| id.starts_with(family)),
            "missing family {family}"
        );
    }
    assert!(lookup("traversal.door_key", 1).is_some());
    assert!(lookup("traversal.door_key", 2).is_some());
}

#[test]
fn every_pattern_positive_cost_journey_and_cook() {
    for spec in specs() {
        let module = host_for(spec);
        let instance = instance_for(spec, module.anchor);
        let expansion = expand_instance(&module, &instance, &caps_for(spec))
            .unwrap_or_else(|e| panic!("{}: {e}", spec.id));
        assert_eq!(expansion.journeys.len(), spec.journeys.len(), "{}", spec.id);
        for hook in spec.journeys {
            assert!(
                expansion.journeys.iter().any(|j| j.name.as_str() == *hook),
                "{} missing journey {hook}",
                spec.id
            );
        }
        assert!(
            expansion.cost.predicates <= spec.budget.predicates,
            "{} preds {}/{}",
            spec.id,
            expansion.cost.predicates,
            spec.budget.predicates
        );
        assert!(
            expansion.cost.rite_steps <= spec.budget.rite_steps,
            "{} rites {}/{}",
            spec.id,
            expansion.cost.rite_steps,
            spec.budget.rite_steps
        );
        assert_eq!(expansion.cost.per_tick, 0, "{}", spec.id);
        let doc = expansion.as_doc();
        assert!(
            doc.validate().is_ok(),
            "{}: {}",
            spec.id,
            doc.validate().unwrap_err()
        );
        let mut merged = module.body.clone();
        merged.canon_diffs.extend(doc.canon_diffs.iter().cloned());
        merged.seed.extend(doc.seed.iter().cloned());
        merged.minds.extend(doc.minds.iter().cloned());
        cook(&merged).unwrap_or_else(|e| panic!("{} cook: {e}", spec.id));
        for name in doc.seed.iter().filter_map(|f| match f {
            SeedFact::Locus { name, .. } => Some(name.as_str()),
            _ => None,
        }) {
            assert!(
                name.contains("main__inst__") || spec.params.iter().any(|p| p.name == name),
                "{} generated name {name} must derive from module/instance/local",
                spec.id
            );
        }
        for span in &expansion.spans {
            let again = instance
                .anchor
                .child(format!("v{}:{}", spec.version, span.local_id.as_str()).as_bytes());
            assert!(
                expansion.anchors.iter().any(|a| a.anchor == again),
                "{} missing child anchor for {}",
                spec.id,
                span.local_id
            );
        }
    }
}

#[test]
fn every_pattern_negative_missing_arg_and_capability() {
    for spec in specs() {
        let module = host_for(spec);
        let mut instance = instance_for(spec, module.anchor);
        if let Some(param) = spec.params.iter().find(|p| p.default.is_none()) {
            instance.args.retain(|a| a.key.as_str() != param.name);
            let err = expand_instance(&module, &instance, &caps_for(spec)).unwrap_err();
            assert!(
                matches!(err, PatternError::Arg { .. }),
                "{}: {err}",
                spec.id
            );
        }
        if let Some((param, cap)) = spec.requires.first() {
            let instance = instance_for(spec, module.anchor);
            let err = expand_instance(&module, &instance, &HostCaps::new()).unwrap_err();
            match err {
                PatternError::Capability { locus, cap: c } => {
                    assert_eq!(locus, *param, "{}", spec.id);
                    assert_eq!(c, *cap, "{}", spec.id);
                }
                other => panic!("{}: {other}", spec.id),
            }
        }
        if let Some((param, cap)) = spec.conflicts.first() {
            let instance = instance_for(spec, module.anchor);
            let mut caps = caps_for(spec);
            caps.grant(param, cap);
            let err = expand_instance(&module, &instance, &caps).unwrap_err();
            assert!(
                matches!(err, PatternError::Conflict { .. }),
                "{}: {err}",
                spec.id
            );
        }
        let mut bad = instance_for(spec, module.anchor);
        bad.pattern = name("no.such.pattern");
        let err = expand_instance(&module, &bad, &caps_for(spec)).unwrap_err();
        assert!(
            matches!(err, PatternError::Unknown(_)),
            "{}: {err}",
            spec.id
        );
        let mut ver = instance_for(spec, module.anchor);
        ver.version = 99;
        let err = expand_instance(&module, &ver, &caps_for(spec)).unwrap_err();
        assert!(
            matches!(err, PatternError::Version { .. }),
            "{}: {err}",
            spec.id
        );
    }
}

#[test]
fn door_key_migration_adds_consume_default_and_journey() {
    let spec = lookup("traversal.door_key", 1).unwrap();
    let module = host_for(spec);
    let v1 = instance_for(spec, module.anchor);
    let v2 = migrate_instance(&v1, 2).unwrap();
    assert_eq!(v2.version, 2);
    assert!(
        v2.args
            .iter()
            .any(|a| a.key.as_str() == "consume_key" && a.value == ParameterValue::Bool(true))
    );
    let (before, after) = crate::migration_journeys("traversal.door_key", 1, 2).unwrap();
    assert!(before.contains(&"unlocked"));
    assert!(after.contains(&"consumed"));
    let expanded = expand_instance(
        &module,
        &v2,
        &caps_for(lookup("traversal.door_key", 2).unwrap()),
    )
    .unwrap();
    assert!(
        expanded
            .journeys
            .iter()
            .any(|j| j.name.as_str() == "consumed")
    );
    let again = migrate_instance(&v1, 1).unwrap();
    assert_eq!(again.version, 1);
}

#[test]
fn expansion_is_order_and_repeat_deterministic() {
    let spec = lookup("traversal.door_key", 2).unwrap();
    let module = host_for(spec);
    let a = instance_for(spec, module.anchor);
    let first = expand_instance(&module, &a, &caps_for(spec)).unwrap();
    let second = expand_instance(&module, &a, &caps_for(spec)).unwrap();
    assert_eq!(
        to_ron(&first.as_doc()).unwrap(),
        to_ron(&second.as_doc()).unwrap()
    );
    let mut acc = Vec::new();
    for spec in specs() {
        let module = host_for(spec);
        let instance = instance_for(spec, module.anchor);
        let expansion = expand_instance(&module, &instance, &caps_for(spec)).unwrap();
        acc.extend_from_slice(to_ron(&expansion.as_doc()).unwrap().as_bytes());
    }
    let digest = hash_bytes(&acc);
    assert_ne!(digest, Hash::ZERO);
}

#[test]
fn generated_names_ignore_insertion_order() {
    let spec = lookup("combat.light_heavy", 1).unwrap();
    let mut module = host_for(spec);
    let mut a = instance_for(spec, module.anchor);
    a.instance = name("alpha");
    a.anchor = module.anchor.child(b"pattern:alpha");
    let mut b = instance_for(spec, module.anchor);
    b.instance = name("beta");
    b.anchor = module.anchor.child(b"pattern:beta");
    attach(&mut module, &a);
    attach(&mut module, &b);
    let mut caps = caps_for(spec);
    let forward = expand_module(&module, &mut caps).unwrap();
    module.patterns.reverse();
    let mut caps = caps_for(spec);
    let reverse = expand_module(&module, &mut caps).unwrap();
    assert_eq!(forward.body.canon_diffs, reverse.body.canon_diffs);
    let names: Vec<_> = forward
        .body
        .canon_diffs
        .iter()
        .filter_map(|d| match d {
            klotho_ir::CanonDiff::AddRite(r) => Some(r.id.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert!(names.iter().any(|n| n.contains("alpha")));
    assert!(names.iter().any(|n| n.contains("beta")));
}

#[test]
fn expand_then_flatten_has_no_pattern_type() {
    let spec = lookup("world.place_shell", 1).unwrap();
    let mut module = host_for(spec);
    let instance = instance_for(spec, module.anchor);
    attach(&mut module, &instance);
    let hash = module.content_hash().unwrap();
    let bundle = migrate_doc(name("spin"), name("main"), empty_doc()).unwrap();
    let mut bundle = bundle;
    bundle.modules = vec![module];
    bundle.project.modules[0].hash = hash;
    bundle.project.lock.entries[0].hash = hash;
    let expanded = expand_bundle(&bundle).unwrap();
    assert!(expanded.modules.iter().all(|m| m.patterns.is_empty()));
    let flat = expanded.project.flatten(&expanded.modules).unwrap();
    assert!(flat.anchors.iter().all(|a| a.kind != AnchorKind::Pattern));
    cook(&flat.doc).unwrap();
}

#[test]
fn empty_expand_is_flatten_identity() {
    let bundle = migrate_doc(
        name("hearth"),
        name("main"),
        IntentDoc {
            seed: vec![SeedFact::Locus {
                name: name("chair"),
                kind: LocusKind::Relic,
            }],
            ..empty_doc()
        },
    )
    .unwrap();
    let expanded = expand_bundle(&bundle).unwrap();
    assert_eq!(
        bundle.project.flatten(&bundle.modules).unwrap().doc,
        expanded.project.flatten(&expanded.modules).unwrap().doc
    );
}
