//! P0/P1/P2 live-service evidence and fail-closed public claims.

use ed25519_dalek::{Signature, VerifyingKey};
use klotho_core::Hash;
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::LiveError;

/// Evidence level for a named service target.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum EvidenceLevel {
    /// Public mocks, deterministic fixtures, and conformance tests.
    P0,
    /// Signed confidential service/security/moderation/scale evidence.
    P1,
    /// External platform/service acceptance bound to P1 and the package.
    P2,
}

impl EvidenceLevel {
    /// Stable token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
            Self::P2 => "P2",
        }
    }

    /// Public statement for this achieved level.
    #[must_use]
    pub const fn public_statement(self) -> &'static str {
        match self {
            Self::P0 => "P0 public multiplayer/live boundary",
            Self::P1 => "P1 confidential multiplayer/live validation",
            Self::P2 => "P2 external multiplayer/live acceptance",
        }
    }
}

/// Reproducible public P0 evidence identity.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicEvidence {
    /// Named service target and scale profile.
    pub target: String,
    /// Package/candidate identity.
    pub package_hash: Hash,
    /// Hash of deterministic public reports.
    pub report_hash: Hash,
    /// Responsible owner.
    pub owner: String,
    /// Evidence date.
    pub date: String,
}

impl PublicEvidence {
    /// Validate visible, reproducible P0 fields.
    pub fn validate(&self) -> Result<(), LiveError> {
        if self.target.trim().is_empty()
            || self.package_hash == Hash::ZERO
            || self.report_hash == Hash::ZERO
            || self.owner.trim().is_empty()
            || !iso_date(&self.date)
        {
            return Err(LiveError::Evidence(
                "public live evidence is incomplete".into(),
            ));
        }
        Ok(())
    }

    /// Content address of the visible envelope.
    #[must_use]
    pub fn id(&self) -> Hash {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.target.as_bytes());
        bytes.extend_from_slice(self.package_hash.as_bytes());
        bytes.extend_from_slice(self.report_hash.as_bytes());
        bytes.extend_from_slice(self.owner.as_bytes());
        bytes.extend_from_slice(self.date.as_bytes());
        hash_bytes(&bytes)
    }
}

/// Redacted public envelope for signed confidential P1 evidence.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateEvidence {
    /// Named service target.
    pub target: String,
    /// Exact package/candidate.
    pub package_hash: Hash,
    /// Public commitment to protected service/security/moderation/scale bytes.
    pub protected_commitment: Hash,
    /// Named accountable owner.
    pub owner: String,
    /// Evidence date.
    pub date: String,
    /// Expiry date.
    pub expiry: String,
    /// Pass/fail remains visible.
    pub passed: bool,
    /// Ed25519 signature over the envelope id.
    pub signature: Vec<u8>,
    /// Verifying key.
    pub verifying_key: [u8; 32],
}

impl PrivateEvidence {
    /// Public id excluding signature/key.
    #[must_use]
    pub fn id(&self) -> Hash {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.target.as_bytes());
        bytes.extend_from_slice(self.package_hash.as_bytes());
        bytes.extend_from_slice(self.protected_commitment.as_bytes());
        bytes.extend_from_slice(self.owner.as_bytes());
        bytes.extend_from_slice(self.date.as_bytes());
        bytes.extend_from_slice(self.expiry.as_bytes());
        bytes.push(u8::from(self.passed));
        hash_bytes(&bytes)
    }

    /// Verify visible fields and signature. This does not reproduce protected
    /// evidence and therefore cannot independently prove its contents.
    pub fn validate_envelope(&self) -> Result<(), LiveError> {
        if self.target.trim().is_empty()
            || self.package_hash == Hash::ZERO
            || self.protected_commitment == Hash::ZERO
            || self.owner.trim().is_empty()
            || !iso_date(&self.date)
            || !iso_date(&self.expiry)
        {
            return Err(LiveError::Evidence(
                "private evidence envelope is incomplete".into(),
            ));
        }
        let key = VerifyingKey::from_bytes(&self.verifying_key)
            .map_err(|_| LiveError::Evidence("private evidence key is invalid".into()))?;
        if key.is_weak() {
            return Err(LiveError::Evidence("private evidence key is weak".into()));
        }
        let signature: [u8; 64] = self.signature.as_slice().try_into().map_err(|_| {
            LiveError::Evidence("private evidence signature length is invalid".into())
        })?;
        key.verify_strict(self.id().as_bytes(), &Signature::from_bytes(&signature))
            .map_err(|_| LiveError::Evidence("private evidence signature is invalid".into()))
    }
}

/// External service/platform acceptance for P2.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalAcceptance {
    /// Accepting service/platform authority.
    pub authority: String,
    /// Service target.
    pub target: String,
    /// Exact package/candidate.
    pub package_hash: Hash,
    /// P1 envelope id.
    pub p1_id: Hash,
    /// Commitment to the protected acceptance record.
    pub record_hash: Hash,
}

/// Compute the achieved level. A P1 envelope alone is not reproducible public
/// evidence: callers must also confirm protected bytes were resolved and match.
#[must_use]
pub fn achieved_level(
    public: &PublicEvidence,
    private: Option<&PrivateEvidence>,
    protected_bytes_resolved: bool,
    external: Option<&ExternalAcceptance>,
) -> EvidenceLevel {
    if public.validate().is_err() {
        return EvidenceLevel::P0;
    }
    let Some(private) = private else {
        return EvidenceLevel::P0;
    };
    if !protected_bytes_resolved
        || !private.passed
        || private.validate_envelope().is_err()
        || private.target != public.target
        || private.package_hash != public.package_hash
    {
        return EvidenceLevel::P0;
    }
    let Some(external) = external else {
        return EvidenceLevel::P1;
    };
    if external.authority.trim().is_empty()
        || external.target != public.target
        || external.package_hash != public.package_hash
        || external.p1_id != private.id()
        || external.record_hash == Hash::ZERO
    {
        return EvidenceLevel::P1;
    }
    EvidenceLevel::P2
}

/// Refuse unqualified production-ready/certified claims below P2.
pub fn forbid_overclaim(text: &str, level: EvidenceLevel) -> Result<(), LiveError> {
    if level == EvidenceLevel::P2 {
        return Ok(());
    }
    let lower = text.to_ascii_lowercase();
    for banned in [
        "multiplayer production-ready",
        "live-service production-ready",
        "certified live service",
        "externally accepted",
    ] {
        if lower.contains(banned) {
            return Err(LiveError::Evidence(format!(
                "{banned} is forbidden at {}",
                level.as_str()
            )));
        }
    }
    Ok(())
}

fn iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_evidence_cannot_overclaim() {
        let public = PublicEvidence {
            target: "netlock-32p-public".into(),
            package_hash: Hash::from_bytes([1; 32]),
            report_hash: Hash::from_bytes([2; 32]),
            owner: "QA Owner".into(),
            date: "2026-09-14".into(),
        };
        assert_eq!(
            achieved_level(&public, None, false, None),
            EvidenceLevel::P0
        );
        assert!(forbid_overclaim("multiplayer production-ready", EvidenceLevel::P0).is_err());
    }
}
