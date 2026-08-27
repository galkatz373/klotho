//! 8 workers ≡ 1 worker Ash prefix hash on ≥ 8 injected disjoint islands.

use ash_slice::boot;
use klotho_commit::{AdmitBuf, CommitKernel, IslandProposer, Proposal};
use klotho_core::{AabbMm, BlobId, Budget, Hash, IVec3, LocusKind, Mm, PoseMm, Sigil, Tick, YawMd};
use klotho_ir::{IntentTarget, MindIntent, Verb};
use klotho_jobs::propose_islands;
use klotho_world::WorldView;

fn relic(id: u128) -> Sigil {
    Sigil::pack(LocusKind::Relic, 0, id).unwrap()
}

fn box_mm(half: i32) -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -half,
            y: 0,
            z: -half,
        },
        IVec3 {
            x: half,
            y: 500,
            z: half,
        },
    )
}

struct Stamp;

impl IslandProposer for Stamp {
    fn name(&self) -> &'static str {
        "stamp"
    }

    fn propose_island(&self, island: u16, view: &WorldView, out: &mut AdmitBuf) {
        let mut members: Vec<Sigil> = view
            .loci()
            .filter(|&s| {
                view.island(s).map(|(id, _)| id) == Some(island) && view.posed_hull(s).is_some()
            })
            .collect();
        members.sort_unstable();
        for s in members {
            out.push(Proposal::Mind(MindIntent {
                locus: s,
                verb: Verb::Use,
                target: IntentTarget::None,
                utility: 0,
            }));
        }
    }
}

fn inject_disjoint(k: &mut CommitKernel, n: usize) -> Vec<Sigil> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let s = relic(50_000 + i as u128);
        let x = (i as i32) * 50_000;
        {
            let mut w = k.world_mut();
            w.insert_locus(s, LocusKind::Relic).unwrap();
            w.set_hull(s, box_mm(100), BlobId::ZERO).unwrap();
            w.set_pose(s, PoseMm::new(Mm(x), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            w.set_island(s, 0, 0).unwrap();
        }
        out.push(s);
    }
    out
}

fn run_workers(n_workers: usize) -> Hash {
    let mut k = boot();
    let planted = inject_disjoint(&mut k, 8);
    let islands = k.partition();
    assert!(
        islands.len() >= 8,
        "expected ≥ 8 disjoint islands, got {}",
        islands.len()
    );
    for s in &planted {
        let (id, _) = k.world().view().island(*s).expect("island");
        assert!(
            islands.iter().any(|(i, m)| *i == id && m.contains(s)),
            "planted {s} missing from partition {islands:?}"
        );
    }
    let stamp = Stamp;
    let batch = {
        let view = k.world().view();
        propose_islands(n_workers, &islands, &[&stamp], &view)
    };
    for (p, ix) in batch {
        k.ingest_from(p, ix);
    }
    let d = k.step(Tick(1), Budget::HEARTH, &mut []).expect("step");
    assert!(d.rejects.is_empty(), "jobs path must admit stamps: {d:?}");
    assert!(
        !d.events.is_empty(),
        "stamps must commit Trace events: {d:?}"
    );
    k.world().trace_prefix_hash()
}

#[test]
fn eight_workers_match_one_worker_ash_prefix() {
    let one = run_workers(1);
    let two = run_workers(2);
    let eight = run_workers(8);
    assert_ne!(one, Hash::ZERO);
    assert_eq!(one, two);
    assert_eq!(one, eight);
}

#[test]
fn default_ash_is_not_eight_islands() {
    let k = boot();
    let islands = klotho_commit::partition_islands(&k.world().view());
    assert!(
        islands.len() < 8,
        "default Ash must not already have 8 islands (vacuous gate): {islands:?}"
    );
}
