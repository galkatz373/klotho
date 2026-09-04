//! Headless Distaff session tests.

use klotho_author::AuthorError;
use klotho_core::{Hash, LocusKind, Mm, PoseMm, Tick, YawMd};
use klotho_ir::{IntentDoc, Name, ProvenanceId, Rel, SeedFact, StyleIntent};
use klotho_render::NullPresenter;

use crate::{EditorError, EditorSession, Pin};

fn origin() -> PoseMm {
    PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0))
}

fn moved() -> PoseMm {
    PoseMm::new(Mm(250), Mm(0), Mm(80), YawMd(0))
}

fn chair() -> Name {
    Name::from("chair")
}

fn stool_chair() -> IntentDoc {
    IntentDoc {
        style: StyleIntent {
            notes: String::new(),
            palettes: Vec::new(),
            kitbash_tags: vec![Name::from("prop.stool")],
        },
        canon_diffs: Vec::new(),
        seed: vec![
            SeedFact::Locus {
                name: chair(),
                kind: LocusKind::Relic,
            },
            SeedFact::Pose {
                of: chair(),
                pose: origin(),
            },
        ],
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

fn hall_and_chair() -> IntentDoc {
    let mut doc = stool_chair();
    doc.seed.insert(
        0,
        SeedFact::Locus {
            name: Name::from("hall"),
            kind: LocusKind::Place,
        },
    );
    doc.seed.push(SeedFact::Locus {
        name: Name::from("loose"),
        kind: LocusKind::Relic,
    });
    doc.seed.push(SeedFact::Rel {
        a: chair(),
        rel: Rel::In,
        b: Name::from("hall"),
    });
    doc.seed.push(SeedFact::Qty {
        of: chair(),
        res: Name::from("mass_g"),
        value: 12,
    });
    doc
}

#[test]
fn gizmo_translate_without_pin_is_gone_on_recook() {
    let mut session = EditorSession::new(stool_chair());
    session.cook().unwrap();
    let hash0 = session.cooked().unwrap().cook_hash;
    assert_eq!(session.seed_pose(&chair()), Some(origin()));
    assert_eq!(session.kernel_pose(&chair()), Some(origin()));

    session.gizmo_translate(&chair(), moved()).unwrap();
    assert_eq!(session.overlay().get(&chair()).copied(), Some(moved()));
    assert_eq!(session.preview_pose(&chair()), Some(moved()));
    assert_eq!(session.seed_pose(&chair()), Some(origin()));
    assert_eq!(session.kernel_pose(&chair()), Some(origin()));
    assert!(session.dirty());

    let mut presenter = NullPresenter::default();
    session.present(&mut presenter).unwrap();
    assert!(presenter.frames >= 1);

    session.recook().unwrap();
    assert!(
        session.overlay().is_empty(),
        "overlay must not leak into recook"
    );
    assert!(!session.dirty());
    assert_eq!(session.seed_pose(&chair()), Some(origin()));
    assert_eq!(session.kernel_pose(&chair()), Some(origin()));
    assert_eq!(session.cooked().unwrap().cook_hash, hash0);
    assert_eq!(session.preview_pose(&chair()), Some(origin()));
}

#[test]
fn pin_survives_recook() {
    let mut session = EditorSession::new(stool_chair());
    session.cook().unwrap();
    let hash0 = session.cooked().unwrap().cook_hash;

    session.gizmo_translate(&chair(), moved()).unwrap();
    session.pin_pose(&chair(), "place the chair").unwrap();
    assert!(
        session.overlay().get(&chair()).is_none(),
        "pinned overlay is consumed"
    );
    assert_eq!(session.seed_pose(&chair()), Some(moved()));
    assert_eq!(session.kernel_pose(&chair()), Some(moved()));

    session.recook().unwrap();
    assert_eq!(session.seed_pose(&chair()), Some(moved()));
    assert_eq!(session.kernel_pose(&chair()), Some(moved()));
    assert_ne!(session.cooked().unwrap().cook_hash, hash0);
    assert_eq!(
        session
            .doc()
            .seed
            .iter()
            .filter(
                |f| matches!(f, SeedFact::Pose { of, pose } if of == &chair() && *pose == moved())
            )
            .count(),
        1
    );
}

#[test]
fn play_dirties_do_not_survive_recook() {
    let mut session = EditorSession::new(stool_chair());
    session.cook().unwrap();
    let hash0 = session.cooked().unwrap().cook_hash;
    session.play().unwrap();
    session.set_play_pose(&chair(), moved()).unwrap();
    assert_eq!(session.kernel_pose(&chair()), Some(moved()));
    assert_eq!(session.seed_pose(&chair()), Some(origin()));
    assert!(session.overlay().is_empty());

    session.recook().unwrap();
    assert_eq!(session.seed_pose(&chair()), Some(origin()));
    assert_eq!(session.kernel_pose(&chair()), Some(origin()));
    assert_eq!(session.cooked().unwrap().cook_hash, hash0);
    assert!(!session.playing());
}

#[test]
fn pause_stops_step() {
    let mut session = EditorSession::new(stool_chair());
    session.cook().unwrap();
    session.play().unwrap();
    assert!(session.pause().should_step());
    assert!(session.should_step());

    session.set_paused(true);
    assert!(session.pause().paused());
    assert!(!session.pause().should_step());
    assert!(!session.should_step());
    let tick0 = session.kernel().unwrap().world().tick();
    assert!(!session.step().unwrap());
    assert_eq!(session.kernel().unwrap().world().tick(), tick0);

    session.set_paused(false);
    assert!(session.should_step());
    assert!(session.step().unwrap());
    assert_eq!(session.kernel().unwrap().world().tick(), Tick(1));

    session.open_pin_ui();
    assert!(session.pin_ui_open());
    assert!(!session.should_step());
    session.set_paused(false);
    assert!(!session.should_step());
    let tick1 = session.kernel().unwrap().world().tick();
    assert!(!session.step().unwrap());
    assert_eq!(session.kernel().unwrap().world().tick(), tick1);
}

#[test]
fn outliner_groups_by_place() {
    let mut session = EditorSession::new(hall_and_chair());
    session.cook().unwrap();
    let tree = session.outliner();
    assert_eq!(tree.places.len(), 1, "{tree}");
    assert_eq!(tree.places[0].place.name, Name::from("hall"));
    assert_eq!(tree.places[0].place.kind, LocusKind::Place);
    assert_eq!(tree.places[0].members.len(), 1, "{tree}");
    assert_eq!(tree.places[0].members[0].name, chair());
    assert_eq!(tree.places[0].members[0].kind, LocusKind::Relic);
    assert_eq!(tree.ungrouped.len(), 1, "{tree}");
    assert_eq!(tree.ungrouped[0].name, Name::from("loose"));
    assert_eq!(tree.ungrouped[0].kind, LocusKind::Relic);
    assert!(
        tree.ungrouped.iter().all(|e| e.name != chair()),
        "contained relic must not also be ungrouped: {tree}"
    );
}

#[test]
fn inspector_is_author_facing() {
    let mut session = EditorSession::new(hall_and_chair());
    session.cook().unwrap();
    session.select(Some(chair()));
    session.gizmo_translate(&chair(), moved()).unwrap();
    session.pin_pose(&chair(), "place the chair").unwrap();
    let view = session.inspector().expect("selected chair");
    assert_eq!(view.name, chair());
    assert_eq!(view.kind, LocusKind::Relic);
    assert!(
        view.rels
            .iter()
            .any(|(a, r, b)| a == &chair() && *r == Rel::In && b.as_str() == "hall"),
        "{view:?}"
    );
    assert!(
        view.qtys
            .iter()
            .any(|(res, v)| res.as_str() == "mass_g" && *v == 12),
        "{view:?}"
    );
    assert_eq!(view.last_pin_reason.as_deref(), Some("place the chair"));
    let shown = format!("{view:?}{view}");
    assert!(!shown.contains("PackedIx"), "{shown}");
    assert!(!shown.contains("AdmitBuf"), "{shown}");
    assert!(!shown.contains("order_key"), "{shown}");
    assert!(!shown.contains("PhysRequest"), "{shown}");
}

#[test]
fn cook_dashboard_reports_hash_and_dirty() {
    let mut session = EditorSession::new(stool_chair());
    session.cook().unwrap();
    let dash = session.cook_dashboard().expect("cooked");
    assert!(!dash.cook_hash.to_string().is_empty());
    assert_eq!(dash.cook_hash, session.cooked().unwrap().cook_hash);
    assert_eq!(dash.bindings, session.cooked().unwrap().bindings.len());
    assert_eq!(dash.grains, session.cooked().unwrap().grains.len());
    assert!(dash.grains > 0, "kitbash grains must be reported");
    assert!(!dash.dirty);
    assert!(dash.license_nodes > 0);
    assert_eq!(dash.license_unknown, 0);
    assert!(dash.exportable());

    session.gizmo_translate(&chair(), moved()).unwrap();
    let dirty = session.cook_dashboard().expect("cooked");
    assert!(dirty.dirty);
    assert_eq!(dirty.cook_hash, dash.cook_hash);

    session.recook().unwrap();
    let clean = session.cook_dashboard().expect("recooked");
    assert!(!clean.dirty);
    assert_eq!(clean.cook_hash, dash.cook_hash);
}

#[test]
fn empty_pin_reason_is_rejected() {
    let mut session = EditorSession::new(stool_chair());
    session.cook().unwrap();
    session.gizmo_translate(&chair(), moved()).unwrap();
    let err = session.pin_pose(&chair(), "").unwrap_err();
    assert_eq!(err, EditorError::Author(AuthorError::EmptyPinReason));
    assert_eq!(session.overlay().get(&chair()).copied(), Some(moved()));
    assert_eq!(session.seed_pose(&chair()), Some(origin()));

    let err = session
        .apply_pin(Pin::ToSeedTrace {
            fact: SeedFact::Pose {
                of: chair(),
                pose: moved(),
            },
            reason: "   ".into(),
        })
        .unwrap_err();
    assert_eq!(err, EditorError::Author(AuthorError::EmptyPinReason));
    assert_eq!(session.seed_pose(&chair()), Some(origin()));
}
