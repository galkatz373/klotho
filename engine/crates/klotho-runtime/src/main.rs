//! Headless runtime. Plays a recorded `PlayerIntent` script through [`klotho_sim`].
//!
//! Owns [`klotho_infer::InferHost`]. Sync proposers register as space, then
//! motion, then mind (K18/K25). Optional first arg: a `.warp` cooked package.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::Path;
#[cfg(feature = "infer")]
use std::process::Command;
use std::process::ExitCode;

#[cfg(test)]
use hearth_slice::boot;
use hearth_slice::{boot_with_locus_cap, hearth_doc};
use klotho_commit::Proposal;
use klotho_core::Tick;
use klotho_infer::{InferHost, InferJob};
use klotho_interest::InterestConfig;
use klotho_ir::{InferIntent, MindSpec, PlayerIntent, from_ron};
use klotho_mind::Mind;
use klotho_motion::Motion;
use klotho_phys::Phys;
use klotho_sim::{FrameReport, METRIC_PROJ_US, METRIC_SNAP_BYTES, Sim};
use klotho_space::Space;

use klotho_runtime::{
    RuntimeProfile, apply_interest, ingest_island_jobs, kernel_from_cooked_profile,
    load_cooked_warp,
};

fn main() -> ExitCode {
    if env::args().nth(1).as_deref() == Some("--klotho-infer-sidecar") {
        return match klotho_infer::run_sidecar() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("infer sidecar: {error}");
                ExitCode::FAILURE
            }
        };
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let profile = RuntimeProfile::compiled();
    let mut args = env::args().skip(1);
    let first = args.next();
    let (kernel, minds, script) = match first.as_deref() {
        Some(path) if path.ends_with(".warp") => {
            let cooked = load_cooked_warp(Path::new(path))?;
            let minds = cooked.doc.minds.clone();
            let kernel = kernel_from_cooked_profile(&cooked, profile)?;
            (kernel, minds, args.next())
        }
        other => (
            boot_with_locus_cap(profile.locus_cap()),
            hearth_doc().minds,
            other.map(str::to_string),
        ),
    };
    let mut mind = bind_mind(&kernel, minds);
    let intents: Vec<PlayerIntent> = load_intents(script.as_deref())?;

    let n = intents.len();
    let mut sim = Sim::with_budget(kernel, profile.budget());
    let host = start_infer()?;
    let mut space = Space;
    let mut motion = Motion::hearth();
    let phys = Phys;
    let mut rejects = 0usize;
    let mut report = None;
    for mut pi in intents {
        pi.at = sim.kernel().world().tick();
        ingest_infer(&host, &mut sim);
        // Devices emit PlayerIntent; the runtime wraps Proposal::Player.
        sim.ingest(wrap_player(pi));
        let r = tick_profiled(&mut sim, profile, &mut space, &mut motion, &mut mind, &phys)?;
        rejects += r.delta.rejects.len();
        report = Some(r);
        kick_infer(&host, &mut sim);
    }
    let report = match report {
        Some(r) => r,
        None => {
            ingest_infer(&host, &mut sim);
            let r = tick_profiled(&mut sim, profile, &mut space, &mut motion, &mut mind, &phys)?;
            kick_infer(&host, &mut sim);
            r
        }
    };
    println!(
        "intents={n} rejects={rejects} {}={} {}={}",
        METRIC_SNAP_BYTES, report.snap_bytes, METRIC_PROJ_US, report.proj_us
    );
    Ok(())
}

#[cfg(feature = "infer")]
fn start_infer() -> Result<InferHost, String> {
    let executable = env::current_exe().map_err(|e| format!("infer executable: {e}"))?;
    let mut command = Command::new(executable);
    command.arg("--klotho-infer-sidecar");
    InferHost::spawn(command).map_err(|e| format!("spawn infer sidecar: {e}"))
}

#[cfg(not(feature = "infer"))]
fn start_infer() -> Result<InferHost, String> {
    Ok(InferHost::new())
}

fn tick_profiled(
    sim: &mut Sim,
    profile: RuntimeProfile,
    space: &mut Space,
    motion: &mut Motion,
    mind: &mut Mind,
    phys: &Phys,
) -> Result<FrameReport, String> {
    if !profile.uses_island_jobs() {
        return sim
            .tick(Tick(1), &mut [space, motion, mind])
            .map_err(|e| format!("sim tick: {e:?}"));
    }
    apply_interest(sim.kernel_mut(), &InterestConfig::default());
    sim.phase_interest();
    sim.phase_partition();
    sim.phase_propose_jobs();
    let us_propose = ingest_island_jobs(sim.kernel_mut(), profile.workers(), &[phys, motion, mind]);
    sim.phase_join();
    let mut report = sim
        .phase_step(Tick(1), &mut [])
        .map_err(|e| format!("sim tick: {e:?}"))?;
    report.us_propose = us_propose;
    Ok(report)
}

fn load_intents(script: Option<&str>) -> Result<Vec<PlayerIntent>, String> {
    match script {
        Some(path) => {
            let src = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            from_ron(&src).map_err(|e| format!("parse {path}: {e}"))
        }
        None => from_ron(include_str!(
            "../../../examples/hearth-slice/fixtures/golden_03_carry.ron"
        ))
        .map_err(|e| format!("default script: {e}")),
    }
}

fn bind_mind(kernel: &klotho_commit::CommitKernel, minds: Vec<MindSpec>) -> Mind {
    Mind::bind_with(
        minds,
        |n| kernel.canon().pin(n),
        |n| kernel.canon().resource_id(n),
    )
}

#[cfg(test)]
fn hearth_mind(kernel: &klotho_commit::CommitKernel) -> Mind {
    bind_mind(kernel, hearth_doc().minds)
}

fn ingest_infer(host: &InferHost, sim: &mut Sim) {
    let now = sim.kernel().world().tick();
    let slo = sim.budget().eval_slo_ticks;
    let polled = InferHost::poll(host, now, slo);
    for ii in polled.intents {
        sim.ingest(wrap_infer(ii));
    }
}

fn kick_infer(host: &InferHost, sim: &mut Sim) {
    let snap = sim.kernel_mut().snapshot();
    let tick = sim.kernel().world().tick();
    let _ = InferHost::submit(host, InferJob { snap, tick });
}

/// Devices emit [`PlayerIntent`]; this is the only Player wrap site in the runtime.
fn wrap_player(pi: PlayerIntent) -> Proposal {
    Proposal::Player(pi)
}

/// Isolator emits [`InferIntent`]; this is the only Infer wrap site in the runtime.
fn wrap_infer(ii: InferIntent) -> Proposal {
    Proposal::Infer(ii)
}

#[cfg(test)]
mod tests {
    use klotho_core::{Budget, Mm, PlayerId, PoseMm, Tick, YawMd};
    use klotho_input::{DeviceSample, InputMapper};
    use klotho_ir::{Analog, Verb};
    use klotho_manifest::{EYE_HEIGHT_MM, Observer};
    use klotho_platform::LookAccum;

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

    #[test]
    fn infer_intent_is_wrapped_as_infer_proposal() {
        let ii = InferIntent {
            model: klotho_ir::ModelId(klotho_ir::Name::from("stub")),
            locus: None,
            verb: Verb::Look,
            target: klotho_ir::IntentTarget::None,
            claimed_facts: Vec::new(),
        };
        match wrap_infer(ii) {
            Proposal::Infer(p) => assert_eq!(p.verb, Verb::Look),
            other => panic!("expected Infer, got {other:?}"),
        }
    }

    #[test]
    fn infer_off_npcs_act() {
        let kernel = boot_with_locus_cap(RuntimeProfile::Hearth.locus_cap());
        let mut mind = hearth_mind(&kernel);
        let mut sim = Sim::new(kernel);
        let host = InferHost::new();
        let planned = mind.plan(&sim.kernel().world().view());
        assert!(
            !planned.is_empty(),
            "GOAP must emit without infer jobs: {planned:?}"
        );
        let polled = InferHost::poll(
            &host,
            sim.kernel().world().tick(),
            Budget::HEARTH.eval_slo_ticks,
        );
        assert!(polled.intents.is_empty(), "infer-off poll is empty");
        let mut space = Space;
        let mut motion = Motion::hearth();
        let r = sim
            .tick(Tick(1), &mut [&mut space, &mut motion, &mut mind])
            .unwrap();
        assert!(
            !r.delta.events.is_empty(),
            "expected a committed Mind act, got {r:?}"
        );
    }

    #[test]
    fn pause_menu_save_goes_through_save_from_snapshot() {
        use std::sync::Arc;

        use klotho_core::Hash;
        use klotho_ui::{LoadError, Session, check_load, save_from_snapshot};

        let mut kernel = boot();
        let snap = kernel.snapshot();
        let mut session = Session::new();
        session.publish(Arc::clone(&snap));
        session.set_paused(true);
        let pi = InputMapper::hearth().map(&DeviceSample::new(PlayerId(0), Tick(0)));
        assert!(session.accept_player(pi).is_none());
        assert!(!session.should_step());
        let quad = save_from_snapshot(&snap);
        assert_eq!(quad.canon_hash, snap.canon_hash);
        assert_eq!(quad.trace_prefix_hash, snap.trace_prefix_hash);
        assert_eq!(quad.trace_from_tick, snap.tick);
        assert_eq!(
            check_load(&quad, Hash::ZERO, snap.canon_hash),
            Err(LoadError::PrefixMismatch)
        );
        assert_eq!(
            check_load(&quad, snap.trace_prefix_hash, snap.canon_hash),
            Ok(())
        );
    }

    #[test]
    fn observer_is_built_from_look_not_renderer() {
        let mut look = LookAccum::new();
        look.apply_analog(Analog {
            look_yaw: YawMd(15_000),
            look_pitch: -5_000,
            ..Analog::default()
        });
        let ground = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let o: Observer = look.observer(ground);
        assert_eq!(o.eye.y, EYE_HEIGHT_MM);
        assert_eq!(o.eye.yaw, YawMd(15_000));
        assert_eq!(o.pitch_md, -5_000);
    }
}
