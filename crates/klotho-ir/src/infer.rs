//! Infer packets. No [`crate::Agency`]. Claimed facts are subset against `Knows`.

use serde::{Deserialize, Serialize};

use klotho_core::Sigil;

use crate::error::IrError;
use crate::name::Name;
use crate::target::IntentTarget;
use crate::verb::Verb;

/// Cooked or authoring model id (`"dialogue-fill"`).
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(pub Name);

/// Fact the model claims the locus `Knows`. Kernel subsets this against Trace.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FactId(pub Name);

/// Stale-tolerant proposal from `klotho-infer`. Runtime wraps `Proposal::Infer`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferIntent {
    /// Which model produced this.
    pub model: ModelId,
    /// Optional acting locus.
    pub locus: Option<Sigil>,
    /// Verb. `Time` / claimed Timing is `UnclaimedAgency` at the kernel, not here.
    pub verb: Verb,
    /// Target.
    pub target: IntentTarget,
    /// Facts the model asserts. Illegal facts → `HallucinatedFact`.
    pub claimed_facts: Vec<FactId>,
}

impl InferIntent {
    /// Structural checks. Does not enforce K10 (kernel does, via missing Agency).
    pub fn validate(&self) -> Result<(), IrError> {
        self.model.0.check()?;
        self.target.check()?;
        for f in &self.claimed_facts {
            f.0.check()?;
        }
        Ok(())
    }
}
