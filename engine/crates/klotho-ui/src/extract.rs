//! Snapshot + observer mind + Canon → [`UiManifest`].

use klotho_canon::Canon;
use klotho_core::Sigil;
use klotho_ir::Rel;
use klotho_manifest::{UiManifest, Widget, WidgetKind};
use klotho_world::WorldSnapshot;

/// Attention widgets for `observer`. Fact names are emitted only here, and
/// only when `view.knows(observer, fact)`.
#[must_use]
pub fn extract_ui(snap: &WorldSnapshot, observer: Sigil, canon: &Canon) -> UiManifest {
    let view = snap.view();
    let mut widgets = Vec::new();

    for (i, name) in canon.facts.iter().enumerate() {
        let Ok(fact) = u16::try_from(i) else {
            break;
        };
        if view.knows(observer, fact) {
            widgets.push(Widget {
                kind: WidgetKind::Text,
                body: name.as_str().to_string(),
            });
        }
    }

    if let Some(stamina) = canon.resource_id("stamina") {
        widgets.push(Widget {
            kind: WidgetKind::Bar {
                value: view.qty(observer, stamina),
                cap: 100,
            },
            body: "stamina".into(),
        });
    }

    if let Some(copper) = canon.resource_id("copper") {
        let mut owed = 0;
        for s in view.loci() {
            if view.has_rel(s, Rel::Owes, observer) {
                owed += view.qty(s, copper);
            }
        }
        if owed > 0 {
            widgets.push(Widget {
                kind: WidgetKind::Text,
                body: format!("owed {owed} copper"),
            });
        }
    }

    if let Some(mass) = canon.resource_id("mass_g") {
        let mut carried = 0;
        for s in view.loci() {
            if view.has_rel(s, Rel::WieldedBy, observer) {
                carried += view.qty(s, mass);
            }
        }
        if carried > 0 {
            widgets.push(Widget {
                kind: WidgetKind::Text,
                body: format!("mass {carried}g"),
            });
        }
    }

    if let Some((rid, _)) = view.first_rite(observer) {
        let body = canon
            .rites
            .get(usize::from(rid.0))
            .map(|r| r.name.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or("rite")
            .to_string();
        widgets.push(Widget {
            kind: WidgetKind::Prompt,
            body,
        });
    }

    UiManifest::from_widgets(snap.epoch, widgets)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::{CommitKernel, Proposal};
    use klotho_core::{Budget, Hash, LocusKind, PlayerId, Sigil, Tick};
    use klotho_ir::{Agency, Analog, CanonDiff, IntentTarget, PlayerIntent, Rel, Verb, from_ron};
    use klotho_manifest::WidgetKind;
    use klotho_trace::{TraceBody, TraceEvent};
    use klotho_world::World;

    use super::extract_ui;
    use crate::{HudSkin, HudViewport, skin_hud};

    const DENIED: &str = "mira_heard_noise";
    const KNOWN: &str = "player_knows_secret";
    const SILENT: &str = "never_told";

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn kernel(src: &str) -> CommitKernel {
        let d: Vec<CanonDiff> = from_ron(src).unwrap();
        let canon = cook_diffs(&d).unwrap();
        CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO))
    }

    fn fact_id(k: &CommitKernel, name: &str) -> u16 {
        u16::try_from(
            k.canon()
                .facts
                .iter()
                .position(|n| n.as_str() == name)
                .unwrap_or_else(|| panic!("fact {name}")),
        )
        .unwrap()
    }

    fn player_use() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(0),
            at: Tick(0),
            verb: Verb::Use,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: Agency::none(),
        }
    }

    fn bodies(ui: &klotho_manifest::UiManifest) -> Vec<&str> {
        ui.widgets.iter().map(|w| w.body.as_str()).collect()
    }

    #[test]
    fn empty_canon_has_no_widgets() {
        let mut k = kernel("[]");
        let observer = actor(1);
        k.world_mut()
            .insert_locus(observer, LocusKind::Actor)
            .unwrap();
        let snap = k.snapshot();
        let ui = extract_ui(&snap, observer, k.canon());
        assert!(ui.widgets.is_empty());
    }

    #[test]
    fn denied_fact_has_no_widget_path() {
        let src = r#"[
            AddLaw(Law(
                id: "intern.facts",
                when: Or(Knows(Self, "mira_heard_noise"), Or(Knows(Self, "player_knows_secret"), Knows(Self, "never_told"))),
                body: Pred(must: Qty(Self, "stamina", Ge, 0), ought: None),
            )),
        ]"#;
        let mut k = kernel(src);
        let observer = actor(1);
        let other = actor(2);
        let denied = fact_id(&k, DENIED);
        let known = fact_id(&k, KNOWN);
        {
            let mut w = k.world_mut();
            w.insert_locus(observer, LocusKind::Actor).unwrap();
            w.insert_locus(other, LocusKind::Actor).unwrap();
            w.append(TraceEvent::new(
                Tick(1),
                TraceBody::Learned {
                    mind: other,
                    fact: denied,
                },
            ));
            w.append(TraceEvent::new(
                Tick(1),
                TraceBody::Learned {
                    mind: observer,
                    fact: known,
                },
            ));
        }
        let snap = k.snapshot();
        let observer_ui = extract_ui(&snap, observer, k.canon());
        let other_ui = extract_ui(&snap, other, k.canon());

        assert!(
            observer_ui
                .widgets
                .iter()
                .any(|w| w.body == KNOWN && matches!(w.kind, WidgetKind::Text)),
            "{observer_ui:?}"
        );
        for w in &observer_ui.widgets {
            assert!(
                !w.body.contains(DENIED),
                "observer leaked denied fact: {w:?}"
            );
            assert!(
                !w.body.contains(SILENT),
                "observer leaked unknown fact: {w:?}"
            );
        }
        assert!(
            other_ui.widgets.iter().any(|w| w.body == DENIED),
            "other may show the fact they Knows: {other_ui:?}"
        );
        assert!(
            other_ui
                .widgets
                .iter()
                .all(|w| w.body != KNOWN && w.body != SILENT),
            "{other_ui:?}"
        );
        assert!(
            observer_ui
                .widgets
                .iter()
                .all(|w| w.body != DENIED && w.body != SILENT)
        );

        let styled = skin_hud(
            &observer_ui,
            HudViewport::new(1_920, 1_080),
            HudSkin::default(),
        );
        assert!(styled.elements.iter().any(|e| e.body == KNOWN));
        assert!(
            styled
                .elements
                .iter()
                .all(|e| e.body != DENIED && e.body != SILENT),
            "skin invented a denied fact: {styled:?}"
        );
    }

    #[test]
    fn learned_on_unpacked_mind_is_silent() {
        let src = r#"[
            AddLaw(Law(
                id: "intern.facts",
                when: Knows(Self, "mira_heard_noise"),
                body: Pred(must: Qty(Self, "stamina", Ge, 0), ought: None),
            )),
        ]"#;
        let mut k = kernel(src);
        let ghost = actor(9);
        let observer = actor(1);
        let denied = fact_id(&k, DENIED);
        k.world_mut()
            .insert_locus(observer, LocusKind::Actor)
            .unwrap();
        k.world_mut().append(TraceEvent::new(
            Tick(1),
            TraceBody::Learned {
                mind: ghost,
                fact: denied,
            },
        ));
        let snap = k.snapshot();
        let ui = extract_ui(&snap, ghost, k.canon());
        assert!(
            ui.widgets.iter().all(|w| !w.body.contains(DENIED)),
            "{ui:?}"
        );
        let observer_ui = extract_ui(&snap, observer, k.canon());
        assert!(
            observer_ui.widgets.iter().all(|w| !w.body.contains(DENIED)),
            "{observer_ui:?}"
        );
    }

    #[test]
    fn hud_stamina_owed_mass_and_rite() {
        let src = r#"[
            AddLaw(Law(
                id: "intern.qty",
                when: Qty(Self, "stamina", Ge, 0),
                body: Pred(must: Or(Qty(Self, "copper", Ge, 0), Qty(Self, "mass_g", Ge, 0)), ought: None),
            )),
            AddRite(RiteGraph(id: "lockpick", cap_steps: 8, cap_ticks: 180, entry: 0, nodes: [
                Wait(45, Some(Timing)),
                Complete(Success),
            ])),
        ]"#;
        let mut k = kernel(src);
        let observer = actor(1);
        let debtor = actor(2);
        let barrel = relic(3);
        k.bind_player(PlayerId(0), observer);
        let stamina = k.canon().resource_id("stamina").unwrap();
        let copper = k.canon().resource_id("copper").unwrap();
        let mass = k.canon().resource_id("mass_g").unwrap();
        {
            let mut w = k.world_mut();
            w.insert_locus(observer, LocusKind::Actor).unwrap();
            w.insert_locus(debtor, LocusKind::Actor).unwrap();
            w.insert_locus(barrel, LocusKind::Relic).unwrap();
            w.set_qty(observer, stamina, 80).unwrap();
            w.set_qty(debtor, copper, 7).unwrap();
            w.add_rel(debtor, Rel::Owes, observer).unwrap();
            w.set_qty(barrel, mass, 250).unwrap();
            w.add_rel(barrel, Rel::WieldedBy, observer).unwrap();
        }
        k.ingest(Proposal::Player(player_use()));
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        assert!(k.world().view().first_rite(observer).is_some());

        let snap = k.snapshot();
        let ui = extract_ui(&snap, observer, k.canon());
        assert_eq!(
            bodies(&ui),
            ["stamina", "owed 7 copper", "mass 250g", "lockpick"]
        );
        assert!(matches!(
            ui.widgets[0].kind,
            WidgetKind::Bar {
                value: 80,
                cap: 100
            }
        ));
        assert!(matches!(ui.widgets[3].kind, WidgetKind::Prompt));
    }

    #[test]
    fn zero_owed_and_mass_are_silent() {
        let src = r#"[
            AddLaw(Law(
                id: "intern.qty",
                when: Qty(Self, "stamina", Ge, 0),
                body: Pred(must: Or(Qty(Self, "copper", Ge, 0), Qty(Self, "mass_g", Ge, 0)), ought: None),
            )),
        ]"#;
        let mut k = kernel(src);
        let observer = actor(1);
        let stamina = k.canon().resource_id("stamina").unwrap();
        {
            let mut w = k.world_mut();
            w.insert_locus(observer, LocusKind::Actor).unwrap();
            w.set_qty(observer, stamina, 0).unwrap();
        }
        let snap = k.snapshot();
        let ui = extract_ui(&snap, observer, k.canon());
        assert_eq!(bodies(&ui), ["stamina"]);
        assert!(matches!(
            ui.widgets[0].kind,
            WidgetKind::Bar { value: 0, cap: 100 }
        ));
    }

    #[test]
    fn unknown_resource_has_no_widget() {
        let src = r#"[
            AddLaw(Law(
                id: "intern.heat",
                when: Qty(Self, "heat", Ge, 0),
                body: Pred(must: Qty(Self, "heat", Ge, 0), ought: None),
            )),
        ]"#;
        let mut k = kernel(src);
        let observer = actor(1);
        let heat = k.canon().resource_id("heat").unwrap();
        {
            let mut w = k.world_mut();
            w.insert_locus(observer, LocusKind::Actor).unwrap();
            w.set_qty(observer, heat, 400).unwrap();
        }
        let snap = k.snapshot();
        let ui = extract_ui(&snap, observer, k.canon());
        assert!(ui.widgets.is_empty(), "{ui:?}");
    }
}
