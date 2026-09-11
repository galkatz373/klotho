//! Exact automatic checkpoints on a 30 s clock.

use std::sync::Arc;

use klotho_world::WorldSnapshot;

use crate::blob::{SaveBlob, pause_save};

/// Automatic epoch interval, seconds.
pub const AUTOSAVE_SECS: u64 = 30;
/// Convert a wall duration to ticks at `hz`.
#[must_use]
pub fn ticks_for_secs(secs: u64, hz: u32) -> u64 {
    secs.saturating_mul(u64::from(hz))
}

/// Last exact automatic checkpoint.
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

    /// Replace the checkpoint with the current published snapshot on the 30 s
    /// clock. Trace replay retention is a separate runtime concern.
    pub fn on_publish(&mut self, snap: Arc<WorldSnapshot>, hz: u32) {
        if self.blob.is_none() {
            self.blob = Some(pause_save(&snap).expect("published snapshot is a valid checkpoint"));
            return;
        }
        let blob = self.blob.as_ref().expect("just checked");
        let autosave = ticks_for_secs(AUTOSAVE_SECS, hz);
        let dt = snap.tick.0.saturating_sub(blob.trace_from_tick.0);
        if autosave == 0 || dt < autosave {
            return;
        }
        self.blob = Some(pause_save(&snap).expect("published snapshot is a valid checkpoint"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_for_secs_matches_hz() {
        assert_eq!(ticks_for_secs(30, 60), 1800);
        assert_eq!(ticks_for_secs(30, 30), 900);
        assert_eq!(AUTOSAVE_SECS, 30);
    }
}
