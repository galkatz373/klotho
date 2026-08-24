//! Headless runtime. Plays a recorded `PlayerIntent` script through [`klotho_sim`].
//!
//! PR 17 will construct and poll the isolator via `InferHost::new`,
//! `InferHost::submit`, and `InferHost::poll`. This crate is the CI allowlist
//! for those calls; PR 08 does not invoke them. No `klotho-caps` / InferToken.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use hearth_slice::boot;
use klotho_commit::Proposal;
use klotho_core::Tick;
use klotho_ir::{PlayerIntent, from_ron};
use klotho_sim::{METRIC_PROJ_US, METRIC_SNAP_BYTES, Sim};
use klotho_space::Space;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let script = env::args().nth(1);
    let kernel = boot();
    let intents: Vec<PlayerIntent> = match script.as_deref() {
        Some(path) => {
            let src = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            from_ron(&src).map_err(|e| format!("parse {path}: {e}"))?
        }
        None => from_ron(include_str!(
            "../../../examples/hearth-slice/fixtures/golden_03_carry.ron"
        ))
        .map_err(|e| format!("default script: {e}"))?,
    };

    let n = intents.len();
    let mut sim = Sim::new(kernel);
    let mut space = Space;
    let mut rejects = 0usize;
    let mut report = None;
    for mut pi in intents {
        pi.at = sim.kernel().world().tick();
        // Devices emit PlayerIntent; the runtime wraps Proposal::Player.
        sim.ingest(wrap_player(pi));
        let r = sim
            .tick(Tick(1), &mut [&mut space])
            .map_err(|e| format!("sim tick: {e:?}"))?;
        rejects += r.delta.rejects.len();
        report = Some(r);
    }
    let report = match report {
        Some(r) => r,
        None => sim
            .tick(Tick(1), &mut [&mut space])
            .map_err(|e| format!("sim tick: {e:?}"))?,
    };
    println!(
        "intents={n} rejects={rejects} {}={} {}={}",
        METRIC_SNAP_BYTES, report.snap_bytes, METRIC_PROJ_US, report.proj_us
    );
    Ok(())
}

/// Devices emit [`PlayerIntent`]; this is the only wrap site in the runtime.
fn wrap_player(pi: PlayerIntent) -> Proposal {
    Proposal::Player(pi)
}

#[cfg(test)]
mod tests {
    use klotho_core::{PlayerId, Tick};
    use klotho_input::{DeviceSample, InputMapper};
    use klotho_ir::Verb;

    use super::*;

    #[test]
    fn injected_device_is_wrapped_as_player_proposal() {
        let mut sample = DeviceSample::new(PlayerId(0), Tick(4));
        sample.buttons.insert(klotho_input::Button::KeyE);
        let pi = InputMapper::hearth().map(&sample);
        match wrap_player(pi) {
            Proposal::Player(p) => {
                assert_eq!(p.verb, Verb::Use);
                assert_eq!(p.at, Tick(4));
            }
            other => panic!("expected Player, got {other:?}"),
        }
    }
}
