//! Headless runtime. Plays a recorded `PlayerIntent` script through [`klotho_sim`].
//!
//! PR 17 will construct and poll the isolator via `InferHost::new`,
//! `InferHost::submit`, and `InferHost::poll`. This crate is the CI allowlist
//! for those calls; PR 08 does not invoke them. No `klotho-caps` / InferToken.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use hearth_slice::{boot, replay};
use klotho_core::Tick;
use klotho_ir::{PlayerIntent, from_ron};
use klotho_sim::{METRIC_PROJ_US, METRIC_SNAP_BYTES, Sim};

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
    let mut kernel = boot();
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
    let deltas = replay(&mut kernel, &intents);
    let mut sim = Sim::new(kernel);
    // One extra empty tick so the phase loop (Step → InferKick → NetFlush) runs.
    let report = sim
        .tick(Tick(1), &mut [])
        .map_err(|e| format!("sim tick: {e:?}"))?;

    let rejects: usize = deltas.iter().map(|d| d.rejects.len()).sum();
    println!(
        "intents={n} rejects={rejects} {}={} {}={}",
        METRIC_SNAP_BYTES, report.snap_bytes, METRIC_PROJ_US, report.proj_us
    );
    Ok(())
}
