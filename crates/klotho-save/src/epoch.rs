//! Automatic 30 s epoch compaction with a ≤ 120 s Trace suffix.

use std::sync::Arc;

use klotho_trace::TraceEvent;
use klotho_world::WorldSnapshot;

use crate::blob::{SaveBlob, pause_save};

/// Automatic epoch interval, seconds.
pub const AUTOSAVE_SECS: u64 = 30;
/// Maximum suffix window, seconds.
pub const SUFFIX_SECS: u64 = 120;

/// Convert a wall duration to ticks at `hz`.
#[must_use]
pub fn ticks_for_secs(secs: u64, hz: u32) -> u64 {
    secs.saturating_mul(u64::from(hz))
}

/// Last epoch snapshot plus live suffix events since that snap.
#[derive(Clone, Debug, Default)]
pub struct EpochStore {
    blob: Option<SaveBlob>,
}

impl EpochStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Last compacted blob, if any.
    #[must_use]
    pub fn current(&self) -> Option<&SaveBlob> {
        self.blob.as_ref()
    }

    /// Pause-menu save: current snap as a fresh epoch, empty suffix.
    pub fn pause_now(&mut self, snap: Arc<WorldSnapshot>) -> SaveBlob {
        let blob = pause_save(&snap).expect("published snapshot is a valid pause save");
        self.blob = Some(blob.clone());
        blob
    }

    /// Append events after the epoch tick; roll a new epoch on the 30 s clock
    /// or when the suffix would exceed 120 s. Events at or before the epoch
    /// tick are dropped.
    pub fn on_publish(&mut self, snap: Arc<WorldSnapshot>, new_events: &[TraceEvent], hz: u32) {
        if self.blob.is_none() {
            let mut blob = pause_save(&snap).expect("published snapshot is a valid pause save");
            for e in new_events {
                if e.tick > snap.tick {
                    blob.suffix.push(e.clone());
                }
            }
            self.blob = Some(blob);
            return;
        }
        let blob = self.blob.as_mut().expect("just checked");
        let epoch_tick = blob.trace_from_tick;
        for e in new_events {
            if e.tick > epoch_tick {
                blob.suffix.push(e.clone());
            }
        }
        let autosave = ticks_for_secs(AUTOSAVE_SECS, hz);
        let suffix_cap = ticks_for_secs(SUFFIX_SECS, hz);
        let dt = snap.tick.0.saturating_sub(epoch_tick.0);
        let suffix_span = blob
            .suffix
            .iter()
            .map(|e| e.tick.0.saturating_sub(epoch_tick.0))
            .max()
            .unwrap_or(0);
        let roll = (autosave > 0 && dt >= autosave) || (suffix_cap > 0 && suffix_span > suffix_cap);
        if !roll {
            return;
        }
        blob.canon_hash = snap.canon_hash;
        blob.epoch = snap.epoch;
        blob.prefix = snap.trace_prefix_hash;
        blob.trace_from_tick = snap.tick;
        blob.suffix.retain(|e| e.tick > snap.tick);
        blob.snap = snap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_for_secs_matches_hz() {
        assert_eq!(ticks_for_secs(30, 60), 1800);
        assert_eq!(ticks_for_secs(30, 30), 900);
        assert_eq!(ticks_for_secs(120, 60), 7200);
        assert_eq!(AUTOSAVE_SECS, 30);
        assert_eq!(SUFFIX_SECS, 120);
    }
}
