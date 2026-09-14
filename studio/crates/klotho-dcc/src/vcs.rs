//! Binary-source leases, content pointers, disclosure, and vendor quarantine.

use std::collections::BTreeSet;

use klotho_core::Hash;
use serde::{Deserialize, Serialize};

use crate::DccError;

/// Exclusive lease for one binary DCC source.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DccLease {
    /// Content-addressed source identity.
    pub source: Hash,
    /// Human owner.
    pub owner: String,
    /// Absolute UTC expiry in milliseconds.
    pub expires_ms: u64,
    /// Optional handoff recipient.
    pub handoff_to: Option<String>,
}

impl DccLease {
    /// Check ownership and expiry for a write.
    pub fn permits(&self, actor: &str, now_ms: u64) -> Result<(), DccError> {
        if actor != self.owner || actor.trim().is_empty() {
            return Err(DccError::Policy("DCC lease owner mismatch".into()));
        }
        if now_ms >= self.expires_ms {
            return Err(DccError::Policy("DCC lease expired".into()));
        }
        Ok(())
    }

    /// Transfer an unexpired lease to its predeclared recipient.
    pub fn handoff(
        self,
        actor: &str,
        recipient: &str,
        now_ms: u64,
        expires_ms: u64,
    ) -> Result<Self, DccError> {
        self.permits(actor, now_ms)?;
        if self.handoff_to.as_deref() != Some(recipient)
            || recipient.trim().is_empty()
            || expires_ms <= now_ms
        {
            return Err(DccError::Policy("invalid DCC lease handoff".into()));
        }
        Ok(Self {
            source: self.source,
            owner: recipient.to_owned(),
            expires_ms,
            handoff_to: None,
        })
    }
}

/// Versioned binary-source pointer protected by an exclusive lease.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DccSourceLock {
    /// Current approved large-source pointer.
    pub pointer: LfsPointer,
    /// Current exclusive lease.
    pub lease: DccLease,
    /// Monotonic update generation used to reject stale writes.
    pub generation: u64,
}

impl DccSourceLock {
    /// Replace the pointer if ownership and expected generation are current.
    pub fn update(
        &mut self,
        actor: &str,
        now_ms: u64,
        expected_generation: u64,
        pointer: LfsPointer,
    ) -> Result<(), DccError> {
        self.lease.permits(actor, now_ms)?;
        pointer.validate()?;
        if expected_generation != self.generation {
            return Err(DccError::Policy("stale DCC source generation".into()));
        }
        self.pointer = pointer;
        self.generation = self.generation.saturating_add(1);
        Ok(())
    }

    /// Transfer ownership while preserving the current pointer and generation.
    pub fn handoff(
        &mut self,
        actor: &str,
        recipient: &str,
        now_ms: u64,
        expires_ms: u64,
    ) -> Result<(), DccError> {
        self.lease = self
            .lease
            .clone()
            .handoff(actor, recipient, now_ms, expires_ms)?;
        Ok(())
    }
}

/// Hash pointer for an approved large source in LFS/object storage.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LfsPointer {
    /// Object content hash.
    pub object: Hash,
    /// Exact object bytes.
    pub bytes: u64,
    /// Closed storage namespace, not a URL/path.
    pub namespace: String,
}

impl LfsPointer {
    /// Validate a non-ambient, content-addressed pointer.
    pub fn validate(&self) -> Result<(), DccError> {
        if self.object == Hash::ZERO
            || self.bytes == 0
            || self.namespace.trim().is_empty()
            || self.namespace.contains('/')
            || self.namespace.contains('\\')
            || self.namespace.contains("..")
        {
            return Err(DccError::Policy("invalid LFS/CAS pointer".into()));
        }
        Ok(())
    }
}

/// Vendor disclosure and data-handling policy.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisclosureManifest {
    /// Vendor organization.
    pub organization: String,
    /// NDA/project partition hash.
    pub agreement: Hash,
    /// Reference hashes permitted in the workspace.
    pub references: BTreeSet<Hash>,
    /// Remote providers permitted, empty means none.
    pub providers: BTreeSet<String>,
    /// Permitted territories.
    pub territories: BTreeSet<String>,
    /// Retention limit in days.
    pub retention_days: u16,
    /// Whether subcontracting is explicitly permitted.
    pub subcontracting: bool,
    /// Closed artifact destination namespaces.
    pub destinations: BTreeSet<String>,
}

impl DisclosureManifest {
    /// Fail closed on a missing agreement or open-ended destination.
    pub fn validate(&self) -> Result<(), DccError> {
        if self.organization.trim().is_empty()
            || self.agreement == Hash::ZERO
            || self.territories.is_empty()
            || self.retention_days == 0
            || self.destinations.is_empty()
            || self
                .destinations
                .iter()
                .any(|d| d.contains('/') || d.contains(".."))
        {
            return Err(DccError::Policy("incomplete vendor disclosure".into()));
        }
        Ok(())
    }
}

/// Isolated vendor workspace declaration.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VendorWorkspace {
    /// Closed quarantine namespace.
    pub namespace: String,
    /// Disclosure policy.
    pub disclosure: DisclosureManifest,
}

/// Vendor-drop lifecycle. Only `Released` can replace an approved binding.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineState {
    /// Bytes received but not parsed.
    Received,
    /// Parser-isolated validation completed.
    Validated,
    /// Rights evidence completed.
    RightsComplete,
    /// Named owners released the drop.
    Released,
    /// Rejected and permanently non-bindable.
    Rejected,
}

/// Immutable vendor drop in quarantine.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuarantineDrop {
    /// Drop content hash.
    pub content: Hash,
    /// Workspace namespace.
    pub namespace: String,
    /// Current state.
    pub state: QuarantineState,
    /// Validation evidence hash.
    pub validation: Hash,
    /// Release-rights evidence hash.
    pub rights: Hash,
    /// Named acceptance owner.
    pub accepted_by: String,
}

impl QuarantineDrop {
    /// Whether this drop may become a project binding.
    pub fn require_released(&self) -> Result<(), DccError> {
        if self.state != QuarantineState::Released
            || self.content == Hash::ZERO
            || self.validation == Hash::ZERO
            || self.rights == Hash::ZERO
            || self.accepted_by.trim().is_empty()
        {
            return Err(DccError::Policy("vendor drop remains quarantined".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> Hash {
        Hash([byte; 32])
    }

    #[test]
    fn binary_lease_never_allows_expiry_or_other_owner_to_overwrite() {
        let lease = DccLease {
            source: hash(1),
            owner: "artist.a".into(),
            expires_ms: 100,
            handoff_to: Some("artist.b".into()),
        };
        assert_eq!(lease.permits("artist.a", 99), Ok(()));
        assert!(lease.permits("artist.b", 99).is_err());
        assert!(lease.permits("artist.a", 100).is_err());
    }

    #[test]
    fn handoff_preserves_latest_binary_update_and_rejects_stale_writer() {
        let mut locked = DccSourceLock {
            pointer: LfsPointer {
                object: hash(1),
                bytes: 10,
                namespace: "approved_sources".into(),
            },
            lease: DccLease {
                source: hash(9),
                owner: "artist.a".into(),
                expires_ms: 100,
                handoff_to: Some("artist.b".into()),
            },
            generation: 4,
        };
        locked
            .update(
                "artist.a",
                50,
                4,
                LfsPointer {
                    object: hash(2),
                    bytes: 20,
                    namespace: "approved_sources".into(),
                },
            )
            .unwrap();
        locked.handoff("artist.a", "artist.b", 60, 200).unwrap();
        assert_eq!(locked.generation, 5);
        assert_eq!(locked.pointer.object, hash(2));
        assert!(
            locked
                .update(
                    "artist.a",
                    70,
                    4,
                    LfsPointer {
                        object: hash(3),
                        bytes: 30,
                        namespace: "approved_sources".into(),
                    },
                )
                .is_err()
        );
        locked
            .update(
                "artist.b",
                70,
                5,
                LfsPointer {
                    object: hash(4),
                    bytes: 40,
                    namespace: "approved_sources".into(),
                },
            )
            .unwrap();
        assert_eq!(locked.generation, 6);
    }

    #[test]
    fn content_pointer_and_vendor_drop_fail_closed() {
        let pointer = LfsPointer {
            object: hash(2),
            bytes: 10,
            namespace: "approved_sources".into(),
        };
        assert_eq!(pointer.validate(), Ok(()));
        let mut drop = QuarantineDrop {
            content: hash(3),
            namespace: "vendor_alpha".into(),
            state: QuarantineState::RightsComplete,
            validation: hash(4),
            rights: hash(5),
            accepted_by: "art.owner".into(),
        };
        assert!(drop.require_released().is_err());
        drop.state = QuarantineState::Released;
        assert_eq!(drop.require_released(), Ok(()));
    }

    #[test]
    fn disclosure_rejects_open_paths_and_missing_policy() {
        let mut manifest = DisclosureManifest {
            organization: "Vendor Alpha".into(),
            agreement: hash(6),
            references: [hash(7)].into(),
            providers: BTreeSet::new(),
            territories: ["IL".into(), "US".into()].into(),
            retention_days: 30,
            subcontracting: false,
            destinations: ["vendor_quarantine".into()].into(),
        };
        assert_eq!(manifest.validate(), Ok(()));
        manifest.destinations = ["../approved".into()].into();
        assert!(manifest.validate().is_err());
    }
}
