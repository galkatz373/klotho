//! Dedicated server process: Role::Server, in-process PoseDelta encode.
//!
//! Boots the Hearth kernel (or a `.warp` if given), constructs
//! [`klotho_net::Server`], ticks Sim, and flushes PoseDelta from world poses.
//! PoseDelta is never written to Trace. No TCP/UDP listener.

#![forbid(unsafe_code)]

use std::env;
use std::path::Path;
use std::process::ExitCode;

use hearth_slice::{boot_with_locus_cap, hearth_doc};
use klotho_core::{Tick, Vel3};
use klotho_interest::InterestConfig;
use klotho_ir::MindSpec;
use klotho_mind::Mind;
use klotho_motion::Motion;
use klotho_net::{InterestDict, Keypair, LISTEN_INTENT_HZ, Role, Server};
use klotho_phys::Phys;
use klotho_runtime::{
    RuntimeProfile, apply_interest, ingest_island_jobs, ingest_server_intents,
    kernel_from_cooked_profile, load_cooked_warp,
};
use klotho_sim::Sim;
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
    let profile = RuntimeProfile::compiled();
    let mut args = env::args().skip(1);
    let (kernel, minds) = match args.next() {
        Some(path) if path.ends_with(".warp") => {
            let cooked = load_cooked_warp(Path::new(&path))?;
            let minds = cooked.doc.minds.clone();
            (kernel_from_cooked_profile(&cooked, profile)?, minds)
        }
        _ => (boot_with_locus_cap(profile.locus_cap()), hearth_doc().minds),
    };
    let mut mind = bind_mind(&kernel, minds);
    let canon_hash = kernel.world().canon_hash();
    let epoch = kernel.world().epoch();
    let intent_hz = if profile.uses_island_jobs() {
        u8::try_from(profile.auth_hz()).expect("profile auth Hz fits u8")
    } else {
        LISTEN_INTENT_HZ
    };
    let mut server = Server::new(canon_hash, epoch, intent_hz).map_err(|e| e.to_string())?;
    assert_eq!(server.role(), Role::Server);
    let dummy = Keypair::generate().map_err(|e| e.to_string())?;
    let player = server
        .accept_join(&dummy.verifying_bytes())
        .map_err(|e| e.to_string())?;

    let mut sim = Sim::with_budget(kernel, profile.budget());
    server
        .sidecar_mut()
        .set_rewind_ticks(sim.budget().rewind_ticks);
    let mut space = Space;
    let mut motion = Motion::hearth();
    let phys = Phys;
    let mut dict_set = false;
    for _ in 0..3 {
        ingest_server_intents(&mut sim, &mut server);
        let report = if profile.uses_island_jobs() {
            apply_interest(sim.kernel_mut(), &InterestConfig::default());
            sim.phase_interest();
            sim.phase_partition();
            sim.phase_propose_jobs();
            let us = ingest_island_jobs(
                sim.kernel_mut(),
                profile.workers(),
                &[&phys, &motion, &mind],
            );
            sim.phase_join();
            let mut report = sim
                .phase_step(Tick(1), &mut [])
                .map_err(|e| format!("sim tick: {e:?}"))?;
            report.us_propose = us;
            report
        } else {
            sim.tick(Tick(1), &mut [&mut space, &mut motion, &mut mind])
                .map_err(|e| format!("sim tick: {e:?}"))?
        };
        let view = sim.kernel().world().view();
        let loci: Vec<_> = view.loci().collect();
        let mut poses = Vec::new();
        let mut sigils = Vec::new();
        for s in loci {
            let Some(pose) = view.pose(s) else {
                continue;
            };
            let vel = view.vel(s).map(|(v, _)| v).unwrap_or(Vel3::ZERO);
            sigils.push(s);
            poses.push((s, pose, vel));
        }
        if !dict_set {
            server
                .set_interest(
                    player,
                    InterestDict {
                        interest_gen: 0,
                        places: vec![],
                        sigils,
                    },
                )
                .map_err(|e| e.to_string())?;
            dict_set = true;
        }
        let _ = server
            .flush_pose(player, view.tick(), &poses)
            .map_err(|e| e.to_string())?;
        let _ = report;
    }

    let canon = format!("{canon_hash}");
    let prefix: String = canon.chars().take(8).collect();
    println!(
        "role=Server epoch={} intent_hz={} canon={prefix}",
        server.epoch().0,
        server.intent_hz()
    );
    Ok(())
}

fn bind_mind(kernel: &klotho_commit::CommitKernel, minds: Vec<MindSpec>) -> Mind {
    Mind::bind(
        minds,
        |name| kernel.canon().pin(name),
        kernel.canon().resource_id("heat"),
    )
}
