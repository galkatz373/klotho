//! Dedicated server process: Role::Server, in-process PoseDelta encode.
//!
//! Boots the Hearth kernel (or a `.warp` if given), constructs
//! [`klotho_net::Server`], ticks Sim, and flushes PoseDelta from world poses.
//! PoseDelta is never written to Trace. No TCP/UDP listener.

#![forbid(unsafe_code)]

use std::env;
use std::path::Path;
use std::process::ExitCode;

use hearth_slice::boot;
use klotho_core::{Epoch, Tick, Vel3};
use klotho_motion::Motion;
use klotho_net::{InterestDict, Keypair, LISTEN_INTENT_HZ, Role, Server};
use klotho_runtime::{ingest_server_intents, kernel_from_cooked, load_cooked_warp};
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
    let mut args = env::args().skip(1);
    let kernel = match args.next() {
        Some(path) if path.ends_with(".warp") => {
            let cooked = load_cooked_warp(Path::new(&path))?;
            kernel_from_cooked(&cooked)?
        }
        _ => boot(),
    };
    let canon_hash = kernel.world().canon_hash();
    let epoch = Epoch::ZERO;
    let intent_hz = LISTEN_INTENT_HZ;
    let mut server = Server::new(canon_hash, epoch, intent_hz).map_err(|e| e.to_string())?;
    assert_eq!(server.role(), Role::Server);
    let dummy = Keypair::generate().map_err(|e| e.to_string())?;
    let player = server
        .accept_join(&dummy.verifying_bytes())
        .map_err(|e| e.to_string())?;

    let mut sim = Sim::new(kernel);
    server
        .sidecar_mut()
        .set_rewind_ticks(sim.budget().rewind_ticks);
    let mut space = Space;
    let mut motion = Motion::hearth();
    let mut dict_set = false;
    for _ in 0..3 {
        ingest_server_intents(&mut sim, &mut server);
        let report = sim
            .tick(Tick(1), &mut [&mut space, &mut motion])
            .map_err(|e| format!("sim tick: {e:?}"))?;
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
