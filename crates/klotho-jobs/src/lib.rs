//! Island propose jobs. Steal queues, per-worker buffers, join by island id.
//!
//! Gameplay never schedules jobs (K46). `klotho-world/mutate` is not enabled.
//! Unsafe is limited to the steal deque. Further steal changes need Miri
//! and a Loom model of the deque (K39).

#![allow(unsafe_code)]
#![warn(missing_docs)]

use std::cell::UnsafeCell;
use std::collections::BTreeMap;

use klotho_commit::{AdmitBuf, IslandProposer, Proposal};
use klotho_core::Sigil;
use klotho_world::WorldView;

mod steal;

use steal::{StealDeque, next_island};

/// Worker cap. Callers above this are clamped.
pub const MAX_WORKERS: usize = 64;

type Tagged = (Proposal, u8);
type IslandChunk = (u16, Vec<Tagged>);

struct WorkerBuf {
    chunks: UnsafeCell<Vec<IslandChunk>>,
}

// Each worker writes only `bufs[w]`; join reads after `thread::scope` returns.
unsafe impl Sync for WorkerBuf {}

impl WorkerBuf {
    fn new() -> Self {
        Self {
            chunks: UnsafeCell::new(Vec::new()),
        }
    }

    fn push(&self, island: u16, items: Vec<Tagged>) {
        // SAFETY: only the owning worker calls this, and only on its slot.
        unsafe {
            (*self.chunks.get()).push((island, items));
        }
    }

    fn take(&self) -> Vec<IslandChunk> {
        // SAFETY: called after join; no worker still writes this slot.
        unsafe { core::mem::take(&mut *self.chunks.get()) }
    }
}

/// Propose each island into per-worker buffers, concat by island id, total sort.
///
/// `n_workers == 1` is serial (no steal). `n_workers > 1` uses steal queues.
#[must_use]
pub fn propose_islands(
    n_workers: usize,
    islands: &[(u16, Vec<Sigil>)],
    proposers: &[&dyn IslandProposer],
    view: &WorldView<'_>,
) -> Vec<(Proposal, u8)> {
    let n_workers = n_workers.clamp(1, MAX_WORKERS);
    if islands.is_empty() || proposers.is_empty() {
        return Vec::new();
    }
    if n_workers == 1 {
        return join_sorted(propose_serial(islands, proposers, view));
    }
    join_sorted(propose_parallel(n_workers, islands, proposers, view))
}

fn propose_serial(
    islands: &[(u16, Vec<Sigil>)],
    proposers: &[&dyn IslandProposer],
    view: &WorldView<'_>,
) -> Vec<IslandChunk> {
    let mut chunks = Vec::with_capacity(islands.len());
    for (island, _) in islands {
        chunks.push((*island, propose_one(*island, proposers, view)));
    }
    chunks
}

fn propose_one(
    island: u16,
    proposers: &[&dyn IslandProposer],
    view: &WorldView<'_>,
) -> Vec<Tagged> {
    let mut out = Vec::new();
    for (ix, p) in proposers.iter().enumerate() {
        let mut buf = AdmitBuf::new();
        p.propose_island(island, view, &mut buf);
        let Some(reg) = u8::try_from(ix).ok() else {
            break;
        };
        out.extend(buf.drain().into_iter().map(|prop| (prop, reg)));
    }
    out
}

fn propose_parallel(
    n_workers: usize,
    islands: &[(u16, Vec<Sigil>)],
    proposers: &[&dyn IslandProposer],
    view: &WorldView<'_>,
) -> Vec<IslandChunk> {
    let deques: Vec<StealDeque> = (0..n_workers)
        .map(|_| StealDeque::with_cap(islands.len()))
        .collect();
    for (i, (id, _)) in islands.iter().enumerate() {
        deques[i % n_workers].push_bottom(*id);
    }
    let bufs: Vec<WorkerBuf> = (0..n_workers).map(|_| WorkerBuf::new()).collect();
    std::thread::scope(|scope| {
        for w in 0..n_workers {
            let deques = &deques;
            let bufs = &bufs;
            scope.spawn(move || {
                while let Some(island) = next_island(w, deques) {
                    bufs[w].push(island, propose_one(island, proposers, view));
                }
            });
        }
    });
    let mut chunks = Vec::new();
    for b in &bufs {
        chunks.extend(b.take());
    }
    chunks
}

fn join_sorted(chunks: Vec<IslandChunk>) -> Vec<Tagged> {
    let mut by_island: BTreeMap<u16, Vec<Tagged>> = BTreeMap::new();
    for (island, items) in chunks {
        by_island.entry(island).or_default().extend(items);
    }
    let mut out = Vec::new();
    for (_island, items) in by_island {
        out.extend(items);
    }
    out.sort_by_key(|(p, ix)| p.admit_key(*ix));
    out
}
