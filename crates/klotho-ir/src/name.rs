//! Authoring identifiers. Cook binds these to packed table ids.

use serde::{Deserialize, Serialize};

use crate::error::IrError;

/// Non-empty authoring name (`"oak_door"`, `"mass_g"`, `"lockpick"`).
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Name(pub String);

impl Name {
    /// Reject the empty string. Deserializer still accepts it; [`crate::validate_doc`]
    /// / [`Self::check`] refuse it.
    pub fn new(s: impl Into<String>) -> Result<Self, IrError> {
        let s = s.into();
        if s.is_empty() {
            Err(IrError::EmptyName)
        } else {
            Ok(Self(s))
        }
    }

    /// Borrow the bytes as `str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn check(&self) -> Result<(), IrError> {
        if self.0.is_empty() {
            Err(IrError::EmptyName)
        } else {
            Ok(())
        }
    }
}

impl From<&str> for Name {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl core::fmt::Display for Name {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}
