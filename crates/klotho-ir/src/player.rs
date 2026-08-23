//! Runtime player packet. The only intent type that may carry [`crate::Agency`].

use serde::{Deserialize, Serialize};

use klotho_core::{PlayerId, Tick};

use crate::agency::Agency;
use crate::analog::Analog;
use crate::error::IrError;
use crate::target::IntentTarget;
use crate::verb::Verb;

/// Device-emitted desire. Runtime wraps `Proposal::Player`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerIntent {
    /// Local player slot.
    pub player: PlayerId,
    /// Tick the packet was sampled. Stale packets are `StaleEpoch` at ingest.
    pub at: Tick,
    /// Verb.
    pub verb: Verb,
    /// Target locus or none.
    pub target: IntentTarget,
    /// Analog extras.
    pub analog: Analog,
    /// Claimed skill channels. Infer cannot construct this struct.
    pub agency: Agency,
}

impl PlayerIntent {
    /// Structural checks (not Canon admission).
    pub fn validate(&self) -> Result<(), IrError> {
        self.target.check()?;
        self.analog.check()?;
        self.agency.check()
    }
}
