//! Acceptance contract carried by every request (K66).

use serde::{Deserialize, Serialize};

use klotho_ir::{AnchorId, Name};

use crate::ids::JourneyId;

/// Allowed module/anchor set. Empty sets mean unrestricted.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeScope {
    /// Module identities this change may write.
    pub modules: Vec<AnchorId>,
    /// Object identities this change may write.
    pub anchors: Vec<AnchorId>,
}

/// One semantic claim the change must keep true.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticClaim {
    /// Stable claim id.
    pub id: Name,
    /// Human/agent note. Not runtime truth.
    pub text: String,
}

/// Named invariant in the schema/Canon catalog.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvariantRef {
    /// Catalog id.
    pub id: Name,
}

/// Quality target against an approved reference.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityTarget {
    /// Capture or metric name.
    pub id: Name,
    /// Reference identity.
    pub reference: Name,
}

/// Budget target for an affected scene.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetTarget {
    /// Profile or scene name.
    pub id: Name,
    /// Cap in the profile's native unit.
    pub cap: u32,
}

/// Hard invariants, journeys, quality, budgets, and allowed scope.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceContract {
    /// Semantic claims.
    pub claims: Vec<SemanticClaim>,
    /// Journeys this change must pass.
    pub journeys: Vec<JourneyId>,
    /// Catalog invariants.
    pub invariants: Vec<InvariantRef>,
    /// Quality targets.
    pub quality: Vec<QualityTarget>,
    /// Budget targets.
    pub budgets: Vec<BudgetTarget>,
    /// Journeys that must not regress.
    pub non_regression: Vec<JourneyId>,
    /// Write scope the contract permits.
    pub allowed_scope: ChangeScope,
}

impl AcceptanceContract {
    /// Declared journeys plus non-regression, de-duplicated, stable order.
    #[must_use]
    pub fn declared_journeys(&self) -> Vec<JourneyId> {
        let mut out = self.journeys.clone();
        for id in &self.non_regression {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    }
}
