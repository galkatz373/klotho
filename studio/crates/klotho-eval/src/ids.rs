//! Journey identity.

use serde::{Deserialize, Serialize};

use klotho_ir::Name;

/// Stable journey identity. Independent of list position.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JourneyId(pub Name);

impl JourneyId {
    /// Named journey.
    #[must_use]
    pub fn new(name: impl Into<Name>) -> Self {
        Self(name.into())
    }

    /// Borrow the authoring name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for JourneyId {
    fn from(s: &str) -> Self {
        Self(Name::from(s))
    }
}

impl core::fmt::Display for JourneyId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
