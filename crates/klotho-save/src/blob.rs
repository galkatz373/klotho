//! Pause save and load. Copies a published snapshot; does not step a kernel.

use std::sync::Arc;

use klotho_core::{Epoch, Hash, Tick};
use klotho_trace::TraceEvent;
use klotho_world::WorldSnapshot;

use crate::error::SaveError;

/// Canon hash + epoch + prefix + projection snap + Trace suffix.
#[derive(Clone, Debug)]
pub struct SaveBlob {
    /// Frozen Canon hash.
    pub canon_hash: Hash,
    /// Cook / hull epoch.
    pub epoch: Epoch,
    /// Trace prefix ancestry of this checkpoint.
    pub prefix: Hash,
    /// Projection snapshot.
    pub snap: Arc<WorldSnapshot>,
    /// Trace events after [`Self::trace_from_tick`]. Empty on pause save.
    pub suffix: Vec<TraceEvent>,
    /// Snapshot tick (`trace_from_tick`).
    pub trace_from_tick: Tick,
}

/// Copy the published snapshot with an empty suffix. Does not append Trace.
pub fn pause_save(snap: &Arc<WorldSnapshot>) -> Result<SaveBlob, SaveError> {
    Ok(SaveBlob {
        canon_hash: snap.canon_hash,
        epoch: snap.epoch,
        prefix: snap.trace_prefix_hash,
        snap: Arc::clone(snap),
        suffix: Vec::new(),
        trace_from_tick: snap.tick,
    })
}

/// Refuse mismatched ancestry. The canon hash is always checked: a save from
/// another Canon (or epoch) must never restore, even in-process.
pub fn check_load(
    blob: &SaveBlob,
    expected_prefix: Hash,
    expected_canon: Hash,
) -> Result<(), SaveError> {
    if blob.prefix != expected_prefix {
        return Err(SaveError::PrefixMismatch);
    }
    if blob.canon_hash != expected_canon {
        return Err(SaveError::CanonMismatch);
    }
    Ok(())
}

/// Restore the blob if ancestry matches.
pub fn load(
    blob: SaveBlob,
    expected_prefix: Hash,
    expected_canon: Hash,
) -> Result<SaveBlob, SaveError> {
    check_load(&blob, expected_prefix, expected_canon)?;
    Ok(blob)
}
