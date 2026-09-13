//! KAI-15: writers' room, impact, and ten-locale conversation evidence.

use klotho_core::Hash;
use klotho_dialogue::{SHIPPING_LOCALES, observatory, replay_locales};
use klotho_eval::{ConversationHost, JourneyHost, conversation_journey, run_journey};
use klotho_ir::Name;

use crate::review_narrative;

#[test]
fn writers_room_reviews_in_narrative_order() {
    let p = observatory();
    let view = review_narrative(&p);
    assert_eq!(view.bible_version, 1);
    assert!(view.to_string().contains("bible v1"));
    let keys: Vec<_> = view
        .lines
        .iter()
        .map(|l| l.key.as_str().to_owned())
        .collect();
    let file: Vec<_> = p
        .dialogue
        .lines
        .iter()
        .map(|l| l.key.as_str().to_owned())
        .collect();
    assert_ne!(keys, file);
    assert_eq!(keys[0], "mira.greet");
    assert!(view.lines.iter().all(|l| l.vo));
    assert!(view.impact_edges > 0);
    assert!(view.impact_kinds.contains(&"loc"));
}

#[test]
fn ten_locale_conversation_completes_through_distaff_evidence() {
    let p = observatory();
    p.validate_release().unwrap();
    let lowered = p.lower().unwrap();
    let replay = replay_locales(
        &p.dialogue,
        &lowered,
        &p.locales,
        &[Name::from("plates_decoded")],
        &[Name::from("yes")],
    )
    .unwrap();
    assert_eq!(replay.presentation.len(), SHIPPING_LOCALES.len());
    let mut host = ConversationHost::from_project(&p, &[Name::from("plates_decoded")]).unwrap();
    run_journey(
        &mut host,
        &conversation_journey(&p),
        Hash::from_bytes([15; 32]),
    )
    .unwrap();
    assert_eq!(host.last_state(), "mira.yes");
}
