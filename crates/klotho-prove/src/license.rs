//! Machine-auditable license metadata. Not a legal opinion (K9).

use crate::Hash;
use crate::encode::CanonBuf;
use crate::error::ProveError;

/// License recorded on a provenance node.
///
/// `Unknown` is representable so a cook can still hash a blob; **export
/// fails** until every node is some other variant. Combining along
/// `wasDerivedFrom` edges **washes to Unknown** if any parent is Unknown.
/// Distinct non-Unknown spans are **not** algebraically merged — the graph
/// does not prove legal sufficiency.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub enum LicenseSpan {
    /// Missing or unreviewed. In-memory OK; export is not.
    #[default]
    Unknown,
    /// SPDX identifier plus a copyright line. Identifier is not validated
    /// against the SPDX list in v1 (audit metadata, not a license engine).
    Spdx {
        /// SPDX short id, e.g. `MIT`, `CC-BY-4.0`.
        id: String,
        /// Copyright notice as authored. May be empty.
        copyright: String,
    },
    /// Commissioned work. The contract itself is not stored; its hash is.
    Commissioned {
        /// Rights holder name as recorded at Pin / cook.
        holder: String,
        /// blake3 of the contract bytes (canonical LE, hashed by the tools crate).
        contract_hash: Hash,
    },
}

impl LicenseSpan {
    /// SPDX span. Empty `id` is [`ProveError::InvalidLicense`].
    pub fn spdx(id: impl Into<String>, copyright: impl Into<String>) -> Result<Self, ProveError> {
        let id = id.into();
        if id.is_empty() {
            return Err(ProveError::InvalidLicense);
        }
        Ok(Self::Spdx {
            id,
            copyright: copyright.into(),
        })
    }

    /// Commissioned span. Empty `holder` is [`ProveError::InvalidLicense`].
    pub fn commissioned(
        holder: impl Into<String>,
        contract_hash: Hash,
    ) -> Result<Self, ProveError> {
        let holder = holder.into();
        if holder.is_empty() {
            return Err(ProveError::InvalidLicense);
        }
        Ok(Self::Commissioned {
            holder,
            contract_hash,
        })
    }

    /// `true` unless this is [`Self::Unknown`].
    #[must_use]
    pub const fn is_exportable(&self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Conservative wash: Unknown anywhere ⇒ Unknown. Otherwise `self`.
    #[must_use]
    pub fn wash(&self, parent: &Self) -> Self {
        if matches!(self, Self::Unknown) || matches!(parent, Self::Unknown) {
            Self::Unknown
        } else {
            self.clone()
        }
    }

    pub(crate) fn encode(&self, buf: &mut CanonBuf) {
        match self {
            Self::Unknown => buf.u8(0),
            Self::Spdx { id, copyright } => {
                buf.u8(1);
                buf.bytes(id.as_bytes());
                buf.bytes(copyright.as_bytes());
            }
            Self::Commissioned {
                holder,
                contract_hash,
            } => {
                buf.u8(2);
                buf.bytes(holder.as_bytes());
                buf.arr32(contract_hash.as_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_spdx_is_invalid() {
        assert_eq!(LicenseSpan::spdx("", "c"), Err(ProveError::InvalidLicense));
    }

    #[test]
    fn empty_holder_is_invalid() {
        assert_eq!(
            LicenseSpan::commissioned("", Hash::ZERO),
            Err(ProveError::InvalidLicense)
        );
    }

    #[test]
    fn unknown_is_not_exportable() {
        assert!(!LicenseSpan::Unknown.is_exportable());
        assert!(LicenseSpan::spdx("MIT", "").unwrap().is_exportable());
    }

    #[test]
    fn wash_unknown_parent_wins() {
        let mit = LicenseSpan::spdx("MIT", "").unwrap();
        assert_eq!(mit.wash(&LicenseSpan::Unknown), LicenseSpan::Unknown);
        assert_eq!(LicenseSpan::Unknown.wash(&mit), LicenseSpan::Unknown);
        let cc = LicenseSpan::spdx("CC-BY-4.0", "").unwrap();
        // Distinct licenses are not merged.
        assert_eq!(mit.wash(&cc), mit);
    }
}
