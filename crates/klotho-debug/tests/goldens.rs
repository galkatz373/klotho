//! Cross-OS golden Trace prefix hashes. P0: mismatch fails CI.

use hearth_slice::boot as hearth_boot;
use klotho_debug::TracePlayer;
use klotho_ir::from_ron;

fn intents(src: &str) -> Vec<klotho_ir::PlayerIntent> {
    from_ron(src).expect("PlayerIntent RON")
}

fn play_prefix(mut player: TracePlayer, src: &str) -> String {
    let played = player.play(&intents(src)).expect("kernel");
    played.trace_prefix_hash.to_string()
}

fn assert_hash(label: &str, got: String, expected: &str) {
    assert_eq!(
        got,
        expected.trim(),
        "P0 golden mismatch: {label} got {got}"
    );
}

#[test]
fn hearth_01_lockpick_prefix() {
    assert_hash(
        "hearth_01_lockpick",
        play_prefix(
            TracePlayer::new(hearth_boot()),
            include_str!("../../../examples/hearth-slice/fixtures/golden_01_lockpick.ron"),
        ),
        include_str!("../fixtures/hearth_01_lockpick.hash"),
    );
}

#[test]
fn hearth_03_carry_prefix() {
    assert_hash(
        "hearth_03_carry",
        play_prefix(
            TracePlayer::new(hearth_boot()),
            include_str!("../../../examples/hearth-slice/fixtures/golden_03_carry.ron"),
        ),
        include_str!("../fixtures/hearth_03_carry.hash"),
    );
}

#[test]
fn hearth_04_ignite_prefix_and_rejects() {
    let mut player = TracePlayer::new(hearth_boot());
    let played = player
        .play(&intents(include_str!(
            "../../../examples/hearth-slice/fixtures/golden_04_ignite.ron"
        )))
        .expect("kernel");
    assert_hash(
        "hearth_04_ignite",
        played.trace_prefix_hash.to_string(),
        include_str!("../fixtures/hearth_04_ignite.hash"),
    );
    assert_eq!(played.events.len(), 9);
    let last = played.events.last().expect("ninth ignite");
    let text = klotho_debug::format_rejects(&last.rejected);
    assert!(
        last.rejected
            .iter()
            .any(|(_, r)| matches!(r, klotho_core::RejectReason::Law(_))),
        "{text}"
    );
}

#[test]
fn ash_01_fire_prefix() {
    assert_hash(
        "ash_01_fire",
        play_prefix(
            TracePlayer::new(ash_slice::boot()),
            include_str!("../../../examples/ash-slice/fixtures/golden_01_fire.ron"),
        ),
        include_str!("../fixtures/ash_01_fire.hash"),
    );
}
