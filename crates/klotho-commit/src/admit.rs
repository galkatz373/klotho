//! Write-only buffer SyncProposers fill. Kernel drains it.

use klotho_core::Tick;
use klotho_world::WorldView;

use crate::proposal::Proposal;

/// Proposer output. Inner is private so proposers cannot rewrite history.
#[derive(Default)]
pub struct AdmitBuf {
    inner: Vec<Proposal>,
}

impl AdmitBuf {
    /// Empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a proposal. Only append is allowed.
    pub fn push(&mut self, p: Proposal) {
        self.inner.push(p);
    }

    pub(crate) fn drain(&mut self) -> Vec<Proposal> {
        core::mem::take(&mut self.inner)
    }
}

/// Deterministic proposer (Space, Motion, Mind). No hidden integrator state.
pub trait SyncProposer: Send {
    /// Stable name for debug / Trace.
    fn name(&self) -> &'static str;
    /// Write proposals for this tick.
    fn propose(&mut self, view: &WorldView, dt: Tick, out: &mut AdmitBuf);
}
