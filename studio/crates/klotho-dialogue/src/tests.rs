//! KAI-15 gates: quest/continuity faults, 10-locale replay, impact, release.

use klotho_core::Hash;
use klotho_ir::{CanonDiff, Name};
use klotho_prove::hash_bytes;

use crate::bible::Presence;
use crate::dialogue::DialogueCond;
use crate::error::DialogueError;
use crate::loc::{LocaleId, Message, SHIPPING_LOCALES, pseudo_locale};
use crate::quest::{QuestNode, ReentryKind};
use crate::release::release_check;
use crate::replay::{replay_locales, walk};
use crate::sample::observatory;

fn n(s: &str) -> Name {
    Name::from(s)
}

#[test]
fn observatory_validates_and_releases() {
    let p = observatory();
    p.validate_release().unwrap();
    assert_eq!(p.locales.len(), SHIPPING_LOCALES.len());
}

#[test]
fn impossible_quest_cycle_is_detected() {
    let mut p = observatory();
    p.quests.quests[0].prerequisites = vec![n("open_observatory")];
    p.quests.quests[0].available_at_start = false;
    let err = p.quests.validate(&p.bible).unwrap_err();
    assert!(matches!(err, DialogueError::Quest { .. }));
    assert!(err.to_string().contains("cycle"));
}

#[test]
fn orphan_objective_is_detected() {
    let mut p = observatory();
    p.quests.quests.push(QuestNode {
        anchor: p.anchor.child(b"orphan"),
        id: n("side_job"),
        prerequisites: Vec::new(),
        grants: vec![n("side")],
        failure: None,
        cancel: None,
        reentry: ReentryKind::Never,
        critical: false,
        available_at_start: false,
        escape: false,
        ending: false,
    });
    let err = p.quests.validate(&p.bible).unwrap_err();
    assert!(matches!(err, DialogueError::Quest { .. }));
    assert!(err.to_string().contains("orphan"));
}

#[test]
fn unreachable_ending_from_mutex_is_detected() {
    let mut p = observatory();
    p.quests.exclusions.push(crate::quest::QuestExclusion {
        a: n("decode_plates"),
        b: n("open_observatory"),
    });
    p.quests.quests[1].prerequisites = vec![n("decode_plates"), n("open_observatory")];
    let err = p.quests.validate(&p.bible).unwrap_err();
    assert!(matches!(err, DialogueError::Quest { .. }));
}

#[test]
fn continuity_contradiction_is_detected() {
    let mut p = observatory();
    p.bible.locations.push(crate::bible::LocationFact {
        anchor: p.anchor.child(b"yard"),
        name: n("yard"),
    });
    p.bible.presence.push(Presence {
        character: n("mira"),
        beat: n("arrival"),
        location: n("yard"),
    });
    let err = p.bible.validate().unwrap_err();
    assert!(matches!(err, DialogueError::Continuity { .. }));
}

#[test]
fn premature_knowledge_is_detected() {
    let mut p = observatory();
    p.dialogue.lines[0].grants = vec![n("plates_decoded")];
    let err = p.dialogue.validate(&p.bible, &p.quests).unwrap_err();
    assert!(matches!(err, DialogueError::Continuity { .. }));
    assert!(err.to_string().contains("premature"));
}

#[test]
fn unknown_prerequisite_is_detected() {
    let mut p = observatory();
    p.quests.quests[1].prerequisites = vec![n("missing_quest")];
    let err = p.quests.validate(&p.bible).unwrap_err();
    assert!(matches!(err, DialogueError::Quest { .. }));
}

#[test]
fn conversation_replays_deterministically_in_ten_locales() {
    let p = observatory();
    p.validate().unwrap();
    let lowered = p.lower().unwrap();
    let replay = replay_locales(
        &p.dialogue,
        &lowered,
        &p.locales,
        &[n("plates_decoded")],
        &[n("yes")],
    )
    .unwrap();
    assert_eq!(replay.presentation.len(), 10);
    assert!(replay.played.iter().any(|k| k.as_str() == "mira.offer"));
    assert!(
        replay
            .knows
            .iter()
            .any(|k| k.as_str() == "observatory_open")
    );
    let en = replay.presentation.get("en").copied().unwrap();
    let ja = replay.presentation.get("ja").copied().unwrap();
    assert_ne!(en, ja, "locale text must change presentation hash");
    for loc in SHIPPING_LOCALES {
        let cat = p
            .locales
            .iter()
            .find(|c| c.locale.as_str() == *loc)
            .unwrap();
        assert_eq!(
            lowered.with_locale(cat, &p.dialogue).branch_hash(),
            replay.branch_hash
        );
    }
}

#[test]
fn blocked_offer_without_knows_fails_closed() {
    let p = observatory();
    let err = walk(&p.dialogue, &[], &[]).unwrap_err();
    assert!(matches!(err, DialogueError::Replay { .. }));
}

#[test]
fn canon_edit_invalidates_dependent_dialogue_quest_vo_loc() {
    let p = observatory();
    let graph = p.impact();
    let secret = p.bible.secrets[0].anchor;
    let dirty = graph.invalidate(&[secret]);
    assert!(dirty.contains(&p.dialogue.lines[2].anchor), "offer line");
    assert!(dirty.iter().any(|a| {
        graph
            .edges
            .iter()
            .any(|e| e.to == *a && e.kind == crate::impact::ImpactKind::Loc)
    }));
    assert!(dirty.iter().any(|a| {
        graph
            .edges
            .iter()
            .any(|e| e.to == *a && e.kind == crate::impact::ImpactKind::Vo)
    }));
    let covered: Vec<_> = dirty.iter().copied().collect();
    let err = graph.check_evidence(&[secret], &covered).unwrap_err();
    assert!(matches!(err, DialogueError::Stale { .. }));
    graph
        .check_evidence(&[p.anchor.child(b"unrelated")], &covered)
        .unwrap();
}

#[test]
fn missing_key_vo_cc_rights_fail_release() {
    let p = observatory();
    let mut missing_key = p.clone();
    missing_key.locales[0].strings.remove(&n("mira.greet"));
    assert!(matches!(
        release_check(&missing_key.dialogue, &missing_key.locales),
        Err(DialogueError::Release { missing, .. }) if missing == "key"
    ));

    let mut missing_vo = p.clone();
    missing_vo.dialogue.lines[0].vo = None;
    assert!(matches!(
        release_check(&missing_vo.dialogue, &missing_vo.locales),
        Err(DialogueError::Release { missing, .. }) if missing == "vo"
    ));

    let mut missing_cc = p.clone();
    missing_cc.dialogue.lines[0].cc.body.clear();
    assert!(matches!(
        release_check(&missing_cc.dialogue, &missing_cc.locales),
        Err(DialogueError::Release { missing, .. }) if missing == "cc"
    ));

    let mut missing_rights = p.clone();
    if let Some(vo) = missing_rights.dialogue.lines[0].vo.as_mut() {
        vo.rights.approval = Hash::ZERO;
    }
    assert!(matches!(
        release_check(&missing_rights.dialogue, &missing_rights.locales),
        Err(DialogueError::Release { missing, .. }) if missing == "rights"
    ));

    let mut missing_approval = p.clone();
    missing_approval.locales[1].approval = None;
    assert!(matches!(
        release_check(&missing_approval.dialogue, &missing_approval.locales),
        Err(DialogueError::Release { missing, .. }) if missing == "linguistic_approval"
    ));
}

#[test]
fn lowering_is_model_free_and_locale_independent() {
    let p = observatory();
    let a = p.lower().unwrap();
    let b = p.lower().unwrap();
    assert_eq!(a.branch_hash(), b.branch_hash());
    assert!(a.canon_diffs.iter().all(|d| matches!(
        d,
        CanonDiff::AddBeat(_)
            | CanonDiff::AddLaw(_)
            | CanonDiff::AddRite(_)
            | CanonDiff::AddAffordance(_)
    )));
    let cargo = include_str!("../Cargo.toml");
    assert!(!cargo.contains("klotho-ai"));
    assert!(!cargo.contains("klotho-infer"));
    assert!(!cargo.contains("klotho-commit"));
    assert!(!cargo.contains("klotho-world"));
}

#[test]
fn narrative_order_is_not_file_order() {
    let p = observatory();
    let file: Vec<_> = p
        .dialogue
        .lines
        .iter()
        .map(|l| l.key.as_str().to_owned())
        .collect();
    let narrative: Vec<_> = p
        .narrative_lines()
        .iter()
        .map(|n| n.as_str().to_owned())
        .collect();
    assert_ne!(file, narrative);
    assert_eq!(narrative[0], "mira.greet");
    assert_eq!(narrative[1], "mira.idle");
}

#[test]
fn executable_loc_text_and_pseudo_locale() {
    let p = observatory();
    let mut bad = Message {
        key: n("mira.greet"),
        pattern: "hi ${name}".into(),
        gender: None,
        plural: None,
        context: String::new(),
    };
    assert!(bad.check().is_err());
    bad.pattern = "hi {name}".into();
    bad.check().unwrap();
    let en = p
        .locales
        .iter()
        .find(|c| c.locale.as_str() == "en")
        .unwrap();
    let xa = pseudo_locale(en);
    assert_eq!(xa.locale, LocaleId::from("en-XA"));
    assert!(
        xa.strings
            .get(&n("mira.greet"))
            .unwrap()
            .pattern
            .starts_with("[!")
    );
    xa.validate(&p.bible, &p.dialogue).unwrap();
}

#[test]
fn diagnostics_point_at_anchors() {
    let mut p = observatory();
    p.bible.presence.push(Presence {
        character: n("mira"),
        beat: n("arrival"),
        location: n("keep"),
    });
    // duplicate same location is ok; force contradiction
    p.bible.locations.push(crate::bible::LocationFact {
        anchor: p.anchor.child(b"yard"),
        name: n("yard"),
    });
    p.bible.presence.push(Presence {
        character: n("mira"),
        beat: n("arrival"),
        location: n("yard"),
    });
    let d = p.bible.validate().unwrap_err().to_diagnostic();
    assert!(d.points_to_anchor());
    let _ = DialogueCond::Always;
    let _ = hash_bytes(b"kai-15");
}
