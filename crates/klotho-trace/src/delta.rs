//! Per-tick net / kernel output. Rejects are not hashed into the prefix.

use klotho_core::{RejectReason, Tick};

use crate::event::{ProposalKind, TraceEvent};

/// Events admitted this tick, plus legal rejects (K19).
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct TraceDelta {
    /// Tick these events committed on.
    pub tick: Tick,
    /// Admitted events, in commit order.
    pub events: Vec<TraceEvent>,
    /// Legal rejects. Not a [`klotho_core::KernelFault`].
    pub rejects: Vec<(ProposalKind, RejectReason)>,
    /// Snapshot blob size published this tick (0 if none).
    pub snap_bytes: u32,
}

impl TraceDelta {
    /// Empty delta at `tick`.
    #[must_use]
    pub fn empty(tick: Tick) -> Self {
        Self {
            tick,
            events: Vec::new(),
            rejects: Vec::new(),
            snap_bytes: 0,
        }
    }
}
