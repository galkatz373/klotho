//! Process-boundary integration tests for the inference sidecar.

use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use klotho_canon::cook_diffs;
use klotho_core::{Budget, Hash, RejectReason, Tick};
use klotho_infer::{InferHost, InferJob, InferPoll};
use klotho_ir::{CanonDiff, Verb, from_ron};
use klotho_world::{World, WorldSnapshot};

fn snapshot() -> Arc<WorldSnapshot> {
    let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
    let canon = cook_diffs(&diffs).unwrap();
    let mut world = World::new(Arc::new(canon), Hash::ZERO);
    world.snapshot()
}

fn sidecar_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_klotho-infer-sidecar"))
}

#[cfg(unix)]
fn failing_command() -> Command {
    Command::new("false")
}

#[cfg(windows)]
fn failing_command() -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "exit", "1"]);
    command
}

fn wait_for_poll(host: &InferHost, now: Tick) -> InferPoll {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let poll = InferHost::poll(host, now, Budget::HEARTH.eval_slo_ticks);
        if !poll.intents.is_empty() || !poll.stale.is_empty() || !host.is_enabled() {
            return poll;
        }
        assert!(Instant::now() < deadline, "sidecar reply timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn snapshot_crosses_process_and_only_intent_returns() {
    let host = InferHost::spawn(sidecar_command()).unwrap();
    let id = InferHost::submit(
        &host,
        InferJob {
            snap: snapshot(),
            tick: Tick(0),
        },
    );
    assert_ne!(id.0, 0);
    let poll = wait_for_poll(&host, Tick(12));
    assert_eq!(poll.intents.len(), 1, "{poll:?}");
    assert!(poll.stale.is_empty(), "{poll:?}");
    assert_eq!(poll.intents[0].verb, Verb::Look);
    assert!(poll.intents[0].claimed_facts.is_empty());
}

#[test]
fn job_beyond_slo_is_dropped_before_sidecar_can_commit_anything() {
    let host = InferHost::spawn(sidecar_command()).unwrap();
    let id = InferHost::submit(
        &host,
        InferJob {
            snap: snapshot(),
            tick: Tick(0),
        },
    );
    assert_ne!(id.0, 0);
    let poll = InferHost::poll(&host, Tick(13), Budget::HEARTH.eval_slo_ticks);
    assert!(poll.intents.is_empty(), "{poll:?}");
    assert_eq!(poll.stale, [RejectReason::StaleEpoch]);
}

#[test]
fn child_exit_disables_inference() {
    let host = InferHost::spawn(failing_command()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while host.is_enabled() && Instant::now() < deadline {
        let _ = InferHost::poll(&host, Tick(0), Budget::HEARTH.eval_slo_ticks);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!host.is_enabled(), "child EOF must disable inference");
    assert_eq!(
        InferHost::submit(
            &host,
            InferJob {
                snap: snapshot(),
                tick: Tick(0),
            },
        )
        .0,
        0
    );
}
