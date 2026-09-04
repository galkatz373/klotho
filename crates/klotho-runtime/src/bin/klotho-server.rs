//! Dedicated server process: Role::Server, in-process PoseDelta encode.
//!
//! Boots the Hearth kernel (or a `.warp` if given), constructs
//! [`klotho_net::Server`], ticks Sim, and encodes PoseDelta from world poses.
//! PoseDelta is never written to Trace. No TCP/UDP listener.

#![forbid(unsafe_code)]

use std::env;
use std::path::Path;
use std::process::ExitCode;

use hearth_slice::boot;
use klotho_core::{Epoch, Tick, Vel3};
use klotho_motion::Motion;
use klotho_net::{LISTEN_INTENT_HZ, Packet, PoseBlock, PoseFull, Role, Server, encode_packet};
use klotho_runtime::{kernel_from_cooked, load_cooked_warp};
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
    let server = Server::new(canon_hash, epoch, intent_hz).map_err(|e| e.to_string())?;
    assert_eq!(server.role(), Role::Server);

    let mut sim = Sim::new(kernel);
    let mut space = Space;
    let mut motion = Motion::hearth();
    for _ in 0..3 {
        let report = sim
            .tick(Tick(1), &mut [&mut space, &mut motion])
            .map_err(|e| format!("sim tick: {e:?}"))?;
        let view = sim.kernel().world().view();
        let loci: Vec<_> = view.loci().collect();
        let mut entries = Vec::new();
        for (i, s) in loci.into_iter().enumerate() {
            if i > u16::MAX as usize {
                break;
            }
            let Some(pose) = view.pose(s) else {
                continue;
            };
            let vel = view.vel(s).map(|(v, _)| v).unwrap_or(Vel3::ZERO);
            entries.push(PoseFull {
                local_ix: i as u16,
                pose,
                vel,
            });
        }
        let pkt = Packet::PoseDelta {
            tick: view.tick(),
            interest_gen: 0,
            block: PoseBlock::Full(entries),
        };
        let _encoded = encode_packet(&pkt).map_err(|e| e.to_string())?;
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
