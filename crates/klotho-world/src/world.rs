//! Private `World` and published snapshot.

use std::sync::Arc;

use klotho_canon::{Canon, OPAQUE};
use klotho_core::{AffordanceId, Epoch, Hash, Tick};
use klotho_trace::TraceLog;

use crate::error::SnapError;
use crate::heap::IntentHeap;
use crate::proj::Projection;
use crate::snap_blob::SnapRow;
use crate::view::WorldView;
use crate::{MAX_LOCI, SNAPSHOT_CAP};

/// Field whose sources are Canon + Trace + Intent. Projection is derived.
pub struct World {
    canon: Arc<Canon>,
    canon_hash: Hash,
    trace: TraceLog,
    view: Projection,
    intents: IntentHeap,
    epoch: Epoch,
    tick: Tick,
    /// Double-buffer of published snapshots. Not a full World clone.
    snaps: [Option<Arc<WorldSnapshot>>; 2],
    snap_i: usize,
}

/// Deterministic checkpoint of Trace. Blob is the projection, not Manifests.
#[derive(Clone, Debug)]
pub struct WorldSnapshot {
    /// Hull / cook epoch.
    pub epoch: Epoch,
    /// Tick this blob was published.
    pub tick: Tick,
    /// Frozen Canon hash.
    pub canon_hash: Hash,
    /// K19 ancestry of this checkpoint.
    pub trace_prefix_hash: Hash,
    blob: Arc<Projection>,
}

impl World {
    /// Empty world on a cooked Canon. `canon_hash` is the cook digest.
    /// Locus cap is [`MAX_LOCI`] (Hearth).
    #[must_use]
    pub fn new(canon: Arc<Canon>, canon_hash: Hash) -> Self {
        Self::with_locus_cap(canon, canon_hash, MAX_LOCI)
    }

    /// Empty world with a packed-row cap. Cap is clamped to
    /// [`klotho_core::MAX_LOCI_PROCESS`].
    #[must_use]
    pub fn with_locus_cap(canon: Arc<Canon>, canon_hash: Hash, cap: usize) -> Self {
        let opaque = canon.affordance_id(OPAQUE);
        Self {
            canon,
            canon_hash,
            trace: TraceLog::new(),
            view: Projection::with_cap(opaque, cap),
            intents: IntentHeap::new(),
            epoch: Epoch::ZERO,
            tick: Tick::ZERO,
            snaps: [None, None],
            snap_i: 0,
        }
    }

    /// Packed-row cap for this world.
    #[must_use]
    pub fn locus_cap(&self) -> usize {
        self.view.locus_cap()
    }

    /// Live read view. Same query API as [`WorldSnapshot::view`].
    #[must_use]
    pub fn view(&self) -> WorldView<'_> {
        WorldView::at(&self.view, self.tick)
    }

    /// Frozen Canon.
    #[must_use]
    pub fn canon(&self) -> &Canon {
        &self.canon
    }

    /// Append-only history.
    #[must_use]
    pub fn trace(&self) -> &TraceLog {
        &self.trace
    }

    /// Current prefix hash.
    #[must_use]
    pub fn trace_prefix_hash(&self) -> Hash {
        self.trace.prefix_hash()
    }

    /// Cook digest passed to [`Self::new`].
    #[must_use]
    pub fn canon_hash(&self) -> Hash {
        self.canon_hash
    }

    /// Global tick.
    #[must_use]
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// Cook / hull epoch.
    #[must_use]
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// Live Intent heap (read).
    #[must_use]
    pub fn intents(&self) -> &IntentHeap {
        &self.intents
    }

    /// Publish a snapshot into the double-buffer and return it.
    ///
    /// Unchanged CoW chunks are shared with the live projection. Does not clone
    /// Trace, Intent, or Manifests.
    #[must_use]
    pub fn snapshot(&mut self) -> Arc<WorldSnapshot> {
        let i = 1 - self.snap_i;
        let snap = Arc::new(WorldSnapshot {
            epoch: self.epoch,
            tick: self.tick,
            canon_hash: self.canon_hash,
            trace_prefix_hash: self.trace.prefix_hash(),
            blob: Arc::new(self.view.clone()),
        });
        self.snaps[i] = Some(Arc::clone(&snap));
        self.snap_i = i;
        snap
    }

    pub(crate) fn projection(&self) -> &Projection {
        &self.view
    }

    pub(crate) fn projection_mut(&mut self) -> &mut Projection {
        &mut self.view
    }

    pub(crate) fn trace_mut(&mut self) -> &mut TraceLog {
        &mut self.trace
    }

    pub(crate) fn intents_mut(&mut self) -> &mut IntentHeap {
        &mut self.intents
    }

    pub(crate) fn set_tick(&mut self, t: Tick) {
        self.tick = t;
    }
}

impl WorldSnapshot {
    /// Same query API as [`World::view`].
    #[must_use]
    pub fn view(&self) -> WorldView<'_> {
        WorldView::at(&self.blob, self.tick)
    }

    /// Conservative heap size of the projection blob. Cap is [`SNAPSHOT_CAP`].
    #[must_use]
    pub fn approx_bytes(&self) -> usize {
        self.blob.approx_bytes()
    }

    /// True if the blob is under the v1 16 MB cap.
    #[must_use]
    pub fn under_cap(&self) -> bool {
        self.approx_bytes() < SNAPSHOT_CAP
    }

    /// Packed snapshot rows in packed-index order.
    #[must_use]
    pub fn snap_rows(&self) -> Vec<SnapRow> {
        self.blob.capture_snap_rows()
    }

    /// Rebuild a snapshot from rows and `space_ix`.
    pub fn from_snap_rows(
        epoch: Epoch,
        tick: Tick,
        canon_hash: Hash,
        trace_prefix_hash: Hash,
        opaque: Option<AffordanceId>,
        rows: Vec<SnapRow>,
    ) -> Result<Self, SnapError> {
        let blob = Projection::from_snap_rows(opaque, &rows)?;
        Ok(Self {
            epoch,
            tick,
            canon_hash,
            trace_prefix_hash,
            blob: Arc::new(blob),
        })
    }

    /// Canonical little-endian projection blob.
    pub fn encode(&self) -> Result<Vec<u8>, SnapError> {
        crate::snap_blob::encode_snapshot(self)
    }

    /// Decode a blob produced by [`Self::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self, SnapError> {
        crate::snap_blob::decode_snapshot(bytes)
    }

    pub(crate) fn projection(&self) -> &Projection {
        &self.blob
    }

    #[cfg(test)]
    pub(crate) fn shares_pose_chunk(&self, other: &Self, ix: usize) -> bool {
        self.blob.shares_pose_chunk(&other.blob, ix)
    }
}
