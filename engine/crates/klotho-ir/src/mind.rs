//! Mind packets and authoring specs. No [`crate::Agency`].

use serde::{Deserialize, Serialize};

use klotho_core::Sigil;

use crate::error::IrError;
use crate::name::Name;
use crate::target::IntentTarget;
use crate::verb::Verb;

/// GOAP / Beat-emitted desire. Same admission path as the player, minus agency.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MindIntent {
    /// Acting NPC locus.
    pub locus: Sigil,
    /// Verb.
    pub verb: Verb,
    /// Target.
    pub target: IntentTarget,
    /// Debug ranking. Not replicated as authority.
    pub utility: u16,
}

impl MindIntent {
    /// Structural checks.
    pub fn validate(&self) -> Result<(), IrError> {
        self.target.check()
    }
}

/// Authoring: which locus has a mind, and canned dialogue templates.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct MindSpec {
    /// Seed name of the actor.
    pub locus: Name,
    /// GOAP goal labels (`stay_near_forge`). Planner is `klotho-mind`.
    pub goals: Vec<Name>,
    /// Slot templates (`"{name} won't sell that."`). Infer may fill slots, not facts.
    pub templates: Vec<String>,
}

impl MindSpec {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        self.locus.check()?;
        for g in &self.goals {
            g.check()?;
        }
        Ok(())
    }
}
