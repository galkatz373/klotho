//! Intent targets and predicate slots.

use serde::{Deserialize, Serialize};

use klotho_core::Sigil;

use crate::error::IrError;
use crate::name::Name;

/// Who or what an intent is aimed at.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum IntentTarget {
    /// No target (Look, Move).
    #[default]
    None,
    /// Runtime / cooked packed id.
    Sigil(Sigil),
    /// Authoring and recorded scripts. Cook binds to a Sigil.
    Name(Name),
}

impl IntentTarget {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        match self {
            Self::None | Self::Sigil(_) => Ok(()),
            Self::Name(n) => n.check(),
        }
    }
}

/// Predicate slot. `Self` is the acting locus of the current proposal.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Slot {
    /// Acting locus. Serialized as `Self`.
    #[serde(rename = "Self")]
    This,
    /// Intent target.
    Target,
    /// Bound by `ExistsRelated` / `CountRelated`.
    Other,
    /// Cook-time name pinned to a seed Sigil.
    Name(Name),
}

impl Slot {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        match self {
            Self::This | Self::Target | Self::Other => Ok(()),
            Self::Name(n) => n.check(),
        }
    }
}
