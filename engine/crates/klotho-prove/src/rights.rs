//! Release-rights evidence for generated, retrieved, and vendor assets (K71).

use core::fmt;

use klotho_core::Hash;
use serde::{Deserialize, Serialize};

/// How an external asset entered the project.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RightsRoute {
    /// Retrieved from an approved library.
    Retrieval,
    /// Produced by a model or procedural generator.
    Generated,
    /// Delivered under a vendor agreement.
    Vendor,
    /// Commissioned directly from a human creator.
    Commissioned,
}

/// Evidence required before an external asset may enter a release binding.
///
/// This record is an auditable policy input, not a legal opinion. Hashes name
/// the immutable source documents; legal approval remains a named human act.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRights {
    /// Intake route.
    pub route: RightsRoute,
    /// Origin/provider/vendor record hash.
    pub origin: Hash,
    /// Terms, contract, or license document hash.
    pub terms: Hash,
    /// Output ownership or contractual representation hash.
    pub ownership: Hash,
    /// Indemnity position or explicit no-indemnity acknowledgement hash.
    pub indemnity: Hash,
    /// Reference/source permission record hash.
    pub source_permission: Hash,
    /// Likeness, voice, and performer consent record when relevant; an
    /// explicit not-applicable decision is also hashed.
    pub consent: Hash,
    /// Territory/union/export/trademark review record hash.
    pub restrictions: Hash,
    /// Named legal approver. Empty until review is complete.
    pub approved_by: String,
    /// Signed approval record hash.
    pub approval: Hash,
}

impl ReleaseRights {
    /// Validate complete release rights. Zero hashes and anonymous approval
    /// fail closed rather than being treated as not applicable implicitly.
    pub fn validate(&self) -> Result<(), RightsError> {
        for (field, value) in [
            ("origin", self.origin),
            ("terms", self.terms),
            ("ownership", self.ownership),
            ("indemnity", self.indemnity),
            ("source_permission", self.source_permission),
            ("consent", self.consent),
            ("restrictions", self.restrictions),
            ("approval", self.approval),
        ] {
            if value == Hash::ZERO {
                return Err(RightsError::Missing(field));
            }
        }
        if self.approved_by.trim().is_empty() {
            return Err(RightsError::Missing("approved_by"));
        }
        Ok(())
    }
}

/// A missing release-rights field.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum RightsError {
    /// Required field was absent/zero.
    Missing(&'static str),
}

impl fmt::Display for RightsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(field) => write!(f, "missing release-rights field {field}"),
        }
    }
}

impl core::error::Error for RightsError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> Hash {
        Hash([byte; 32])
    }

    #[test]
    fn release_rights_fail_closed_and_require_named_approval() {
        let mut rights = ReleaseRights {
            route: RightsRoute::Vendor,
            origin: hash(1),
            terms: hash(2),
            ownership: hash(3),
            indemnity: hash(4),
            source_permission: hash(5),
            consent: hash(6),
            restrictions: hash(7),
            approved_by: "legal.owner".into(),
            approval: hash(8),
        };
        assert_eq!(rights.validate(), Ok(()));
        rights.consent = Hash::ZERO;
        assert_eq!(rights.validate(), Err(RightsError::Missing("consent")));
    }
}
