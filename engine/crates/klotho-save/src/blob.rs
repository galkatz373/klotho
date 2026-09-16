//! Pause save and load. Copies a published snapshot; does not step a kernel.

use std::sync::Arc;

use klotho_core::{Epoch, Hash, LocusKind, Tick};
use klotho_trace::{TraceEvent, fold_prefix};
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
    /// Platform compatibility for authoritative physical checkpoints.
    pub portability: SavePortability,
}

/// Physics checkpoints are local to their OS and CPU family until a stronger
/// cross-platform determinism result is available.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum SavePortability {
    /// No movable hull was present when the checkpoint was made.
    Portable,
    /// The checkpoint may resume only on this OS and CPU family.
    SamePlatform {
        /// Encoded operating system family.
        os: u8,
        /// Encoded CPU family.
        arch: u8,
    },
}

impl SavePortability {
    /// Current supported host identity; unknown platforms fail closed.
    #[must_use]
    pub fn current() -> Self {
        let os = if cfg!(target_os = "linux") {
            1
        } else if cfg!(target_os = "macos") {
            2
        } else if cfg!(target_os = "windows") {
            3
        } else {
            0
        };
        let arch = if cfg!(target_arch = "x86_64") {
            1
        } else if cfg!(target_arch = "aarch64") {
            2
        } else {
            0
        };
        Self::SamePlatform { os, arch }
    }

    pub(crate) fn compatible(self) -> bool {
        match self {
            Self::Portable => true,
            Self::SamePlatform { os: 0, .. } | Self::SamePlatform { arch: 0, .. } => false,
            other => other == Self::current(),
        }
    }
}

/// Copy the published snapshot with an empty suffix. Does not append Trace.
pub fn pause_save(snap: &Arc<WorldSnapshot>) -> Result<SaveBlob, SaveError> {
    let physical = requires_platform(snap);
    let portability = if physical {
        let host = SavePortability::current();
        if !host.compatible() {
            return Err(SaveError::PlatformMismatch);
        }
        host
    } else {
        SavePortability::Portable
    };
    Ok(SaveBlob {
        canon_hash: snap.canon_hash,
        epoch: snap.epoch,
        prefix: snap.trace_prefix_hash,
        snap: Arc::clone(snap),
        suffix: Vec::new(),
        trace_from_tick: snap.tick,
        portability,
    })
}

/// Refuse mismatched ancestry. `expected_prefix` is the terminal prefix after
/// the suffix, not merely the checkpoint's base prefix.
pub fn check_load(
    blob: &SaveBlob,
    expected_prefix: Hash,
    expected_canon: Hash,
) -> Result<(), SaveError> {
    if blob.canon_hash != blob.snap.canon_hash || blob.canon_hash != expected_canon {
        return Err(SaveError::CanonMismatch);
    }
    if blob.epoch != blob.snap.epoch
        || blob.prefix != blob.snap.trace_prefix_hash
        || blob.trace_from_tick != blob.snap.tick
    {
        return Err(SaveError::PrefixMismatch);
    }
    if fold_prefix(blob.prefix, &blob.suffix) != expected_prefix {
        return Err(SaveError::PrefixMismatch);
    }
    if !blob.portability.compatible()
        || (requires_platform(&blob.snap) && blob.portability == SavePortability::Portable)
    {
        return Err(SaveError::PlatformMismatch);
    }
    Ok(())
}

fn requires_platform(snap: &WorldSnapshot) -> bool {
    let view = snap.view();
    view.loci().any(|s| {
        matches!(s.kind(), Some(LocusKind::Actor | LocusKind::Relic)) && view.hull(s).is_some()
    })
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

/// Validate ancestry and rebuild the checkpoint by applying its Trace suffix.
pub fn restore(
    blob: &SaveBlob,
    expected_prefix: Hash,
    expected_canon: Hash,
) -> Result<Arc<WorldSnapshot>, SaveError> {
    check_load(blob, expected_prefix, expected_canon)?;
    Ok(Arc::new(blob.snap.replay_suffix(&blob.suffix)))
}
