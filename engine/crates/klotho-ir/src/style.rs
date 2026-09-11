//! Style Intent: presentation hints. Missing kitbash tags fail at cook, not here.

use serde::{Deserialize, Serialize};

use crate::error::IrError;
use crate::name::Name;

/// Style + required kitbash tags. Retrieval is `klotho-compile`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct StyleIntent {
    /// Free text (`"chunky readable silhouettes"`).
    pub notes: String,
    /// Palette names (`stone`, `metal`, `organic`).
    pub palettes: Vec<Name>,
    /// Tags the kitbash must provide. Missing tag = cook error (PR 11b).
    pub kitbash_tags: Vec<Name>,
}

impl StyleIntent {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        for n in self.palettes.iter().chain(self.kitbash_tags.iter()) {
            n.check()?;
        }
        Ok(())
    }
}
