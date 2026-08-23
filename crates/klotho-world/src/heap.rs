//! This-tick desires. Not a source; not in the snapshot blob.

use klotho_ir::PlayerIntent;

/// Live Intent heap. Commit drains it; proposers only see `WorldView`.
#[derive(Clone, Debug, Default)]
pub struct IntentHeap {
    player: Vec<PlayerIntent>,
}

impl IntentHeap {
    /// Empty heap.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pending player packets, arrival order.
    #[must_use]
    pub fn player(&self) -> &[PlayerIntent] {
        &self.player
    }

    /// Number of pending player packets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.player.len()
    }

    /// No pending intents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.player.is_empty()
    }

    pub(crate) fn push_player(&mut self, p: PlayerIntent) {
        self.player.push(p);
    }

    pub(crate) fn clear(&mut self) {
        self.player.clear();
    }
}
