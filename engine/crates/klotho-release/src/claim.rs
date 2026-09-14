//! P0/P1/P2 claim levels and redacted evidence ids (K93).
//!
//! A public interface or redacted hash is never presented as proof that
//! private console work passed. Unredacted P1 bytes live in an access-controlled
//! workspace; resolving them from the public tree fails closed.

use std::fs;
use std::path::{Path, PathBuf};

use klotho_core::Hash;
use klotho_platform::{AdapterClass, PlatformTarget};
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::ReleaseError;
use crate::factory::ReleaseSigningKey;

/// Public root of the GDK access-controlled workspace.
pub const GDK_WORKSPACE: &str = "private/gdk";

/// Public root of the Prospero access-controlled workspace.
pub const PROSPERO_WORKSPACE: &str = "private/prospero";

/// Evidence and service claim level (K93).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum ClaimLevel {
    /// Public interfaces, deterministic fixtures, mocks, conformance tests.
    P0,
    /// Proprietary adapter passed on named internal hardware. Requires signed
    /// unredacted evidence in the access-controlled workspace.
    P1,
    /// Platform-holder acceptance bound to the exact package and P1 evidence.
    P2,
}

impl ClaimLevel {
    /// Stable identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
            Self::P2 => "P2",
        }
    }

    /// Human-readable public statement. Never "certified" below P2.
    #[must_use]
    pub const fn public_statement(self) -> &'static str {
        match self {
            Self::P0 => "P0 public boundary",
            Self::P1 => "P1 confidential validation",
            Self::P2 => "P2 external acceptance",
        }
    }

    /// Certified or shipped console SKU may be claimed only at P2.
    #[must_use]
    pub const fn may_claim_certified(self) -> bool {
        matches!(self, Self::P2)
    }

    /// Parse a claim-level token.
    pub fn parse(raw: &str) -> Result<Self, ReleaseError> {
        match raw {
            "P0" => Ok(Self::P0),
            "P1" => Ok(Self::P1),
            "P2" => Ok(Self::P2),
            other => Err(ReleaseError::claim(format!(
                "claim level {other} is not P0, P1, or P2"
            ))),
        }
    }
}

/// Inputs used to compose a public evidence envelope.
pub struct EvidenceDraft {
    /// Declared level.
    pub level: ClaimLevel,
    /// Target named by the evidence.
    pub target: PlatformTarget,
    /// ISO date `YYYY-MM-DD`.
    pub date: String,
    /// Package this evidence is bound to.
    pub package_hash: Hash,
    /// Responsible owner.
    pub owner: String,
    /// ISO expiry `YYYY-MM-DD`.
    pub expiry: String,
    /// Pass/fail.
    pub passed: bool,
}

/// Public envelope of one evidence item. Protected contents are absent.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    /// Content hash of this public envelope.
    pub id: Hash,
    /// Declared level. The achieved level may be lower.
    pub level: ClaimLevel,
    /// Target named by the evidence.
    pub target: PlatformTarget,
    /// ISO date `YYYY-MM-DD`. Never redacted.
    pub date: String,
    /// Package this evidence is bound to. Never redacted.
    pub package_hash: Hash,
    /// Responsible owner. Never redacted.
    pub owner: String,
    /// ISO expiry `YYYY-MM-DD`. Never redacted.
    pub expiry: String,
    /// Pass/fail. Never redacted.
    pub passed: bool,
    /// Hash of unredacted bytes. Not the bytes.
    pub redacted_commitment: Hash,
}

impl EvidenceRecord {
    /// Build a public envelope and fill `id` from the other fields.
    pub fn compose(draft: EvidenceDraft, unredacted: &[u8]) -> Result<Self, ReleaseError> {
        check_iso_date("date", &draft.date)?;
        check_iso_date("expiry", &draft.expiry)?;
        if draft.owner.trim().is_empty() {
            return Err(ReleaseError::claim("evidence owner is empty"));
        }
        if draft.target == PlatformTarget::Desktop {
            return Err(ReleaseError::claim(
                "console evidence cannot target desktop",
            ));
        }
        let mut record = Self {
            id: Hash::ZERO,
            level: draft.level,
            target: draft.target,
            date: draft.date,
            package_hash: draft.package_hash,
            owner: draft.owner,
            expiry: draft.expiry,
            passed: draft.passed,
            redacted_commitment: hash_bytes(unredacted),
        };
        record.id = envelope_hash(&record);
        Ok(record)
    }

    /// Recompute the envelope hash and refuse drift.
    pub fn validate(&self) -> Result<(), ReleaseError> {
        check_iso_date("date", &self.date)?;
        check_iso_date("expiry", &self.expiry)?;
        if self.owner.trim().is_empty() {
            return Err(ReleaseError::claim("evidence owner is empty"));
        }
        if envelope_hash(self) != self.id {
            return Err(ReleaseError::claim("evidence id does not match envelope"));
        }
        Ok(())
    }
}

fn envelope_hash(record: &EvidenceRecord) -> Hash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(record.level.as_str().as_bytes());
    bytes.extend_from_slice(record.target.as_str().as_bytes());
    bytes.extend_from_slice(record.date.as_bytes());
    bytes.extend_from_slice(record.package_hash.as_bytes());
    bytes.extend_from_slice(record.owner.as_bytes());
    bytes.extend_from_slice(record.expiry.as_bytes());
    bytes.push(u8::from(record.passed));
    bytes.extend_from_slice(record.redacted_commitment.as_bytes());
    hash_bytes(&bytes)
}

fn check_iso_date(name: &str, value: &str) -> Result<(), ReleaseError> {
    let bytes = value.as_bytes();
    let ok = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                true
            } else {
                b.is_ascii_digit()
            }
        });
    if ok {
        Ok(())
    } else {
        Err(ReleaseError::claim(format!("{name} must be YYYY-MM-DD")))
    }
}

/// Access-controlled workspace that may hold unredacted P1 bytes.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct AccessWorkspace {
    root: PathBuf,
}

impl AccessWorkspace {
    /// Open `root`. The directory need not exist; resolve then fails closed.
    #[must_use]
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// GDK workspace under a repository root.
    #[must_use]
    pub fn gdk(repo: impl AsRef<Path>) -> Self {
        Self::open(repo.as_ref().join(GDK_WORKSPACE))
    }

    /// Prospero workspace under a repository root.
    #[must_use]
    pub fn prospero(repo: impl AsRef<Path>) -> Self {
        Self::open(repo.as_ref().join(PROSPERO_WORKSPACE))
    }

    /// Workspace root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Store unredacted bytes under `id`. Public CI never calls this.
    pub fn store(&self, id: Hash, bytes: &[u8]) -> Result<(), ReleaseError> {
        if bytes.is_empty() {
            return Err(ReleaseError::claim("unredacted evidence is empty"));
        }
        fs::create_dir_all(&self.root).map_err(|e| ReleaseError::Io(e.to_string()))?;
        fs::write(self.unredacted_path(id), bytes).map_err(|e| ReleaseError::Io(e.to_string()))
    }

    /// Resolve unredacted bytes. A missing file is not public reproduction.
    pub fn resolve(&self, record: &EvidenceRecord) -> Result<Vec<u8>, ReleaseError> {
        record.validate()?;
        let path = self.unredacted_path(record.id);
        let bytes = fs::read(&path).map_err(|_| {
            ReleaseError::claim(format!(
                "evidence {} is a redacted id, not public reproduction",
                record.id
            ))
        })?;
        if hash_bytes(&bytes) != record.redacted_commitment {
            return Err(ReleaseError::claim(
                "unredacted bytes do not match redacted commitment",
            ));
        }
        Ok(bytes)
    }

    fn unredacted_path(&self, id: Hash) -> PathBuf {
        self.root.join(format!("{id}.p1"))
    }
}

/// Platform-holder acceptance required for P2.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolderAcceptance {
    /// Platform holder (`Microsoft`, `Sony`).
    pub holder: String,
    /// Package the acceptance covers.
    pub package_hash: Hash,
    /// P1 evidence id the acceptance covers.
    pub p1_id: Hash,
    /// Hash of the holder record. The record itself is not public.
    pub record_hash: Hash,
}

impl HolderAcceptance {
    /// Fail closed on empty holder or zero hashes.
    pub fn validate(&self) -> Result<(), ReleaseError> {
        if self.holder.trim().is_empty() {
            return Err(ReleaseError::claim("holder name is empty"));
        }
        if self.package_hash == Hash::ZERO
            || self.p1_id == Hash::ZERO
            || self.record_hash == Hash::ZERO
        {
            return Err(ReleaseError::claim("holder acceptance hashes are zero"));
        }
        Ok(())
    }
}

/// Signed binding of P1 unredacted evidence to a package hash.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct BoundP1 {
    /// Public envelope.
    pub record: EvidenceRecord,
    /// Ed25519 signature over `record.id || package_hash`.
    pub signature: [u8; 64],
    /// Verifying key of the signing authority.
    pub verifying_key: [u8; 32],
}

/// Bind unredacted P1 evidence to `package_hash`.
pub fn bind_p1(
    record: EvidenceRecord,
    unredacted: &[u8],
    workspace: &AccessWorkspace,
    signing: &ReleaseSigningKey,
) -> Result<BoundP1, ReleaseError> {
    record.validate()?;
    if record.level != ClaimLevel::P1 {
        return Err(ReleaseError::claim("P1 bind requires a P1 envelope"));
    }
    if !record.passed {
        return Err(ReleaseError::claim("P1 bind requires a passing envelope"));
    }
    if unredacted.is_empty() {
        return Err(ReleaseError::claim("P1 unredacted evidence is empty"));
    }
    if hash_bytes(unredacted) != record.redacted_commitment {
        return Err(ReleaseError::claim(
            "P1 unredacted bytes do not match commitment",
        ));
    }
    workspace.store(record.id, unredacted)?;
    let stored = workspace.resolve(&record)?;
    if stored != unredacted {
        return Err(ReleaseError::claim("P1 workspace round-trip drifted"));
    }
    let mut msg = Vec::new();
    msg.extend_from_slice(record.id.as_bytes());
    msg.extend_from_slice(record.package_hash.as_bytes());
    let signature = signing.sign_bytes(&msg)?;
    Ok(BoundP1 {
        record,
        signature,
        verifying_key: signing.verifying_bytes(),
    })
}

/// Achieved claim for an adapter + optional confidential/holder evidence.
pub fn achieved_level(
    adapter: AdapterClass,
    p1: Option<&BoundP1>,
    holder: Option<&HolderAcceptance>,
    workspace: &AccessWorkspace,
) -> ClaimLevel {
    if adapter != AdapterClass::Proprietary {
        return ClaimLevel::P0;
    }
    let Some(p1) = p1 else {
        return ClaimLevel::P0;
    };
    if p1.record.level != ClaimLevel::P1 || !p1.record.passed {
        return ClaimLevel::P0;
    }
    if workspace.resolve(&p1.record).is_err() {
        return ClaimLevel::P0;
    }
    let Some(holder) = holder else {
        return ClaimLevel::P1;
    };
    if holder.validate().is_err()
        || holder.package_hash != p1.record.package_hash
        || holder.p1_id != p1.record.id
    {
        return ClaimLevel::P1;
    }
    ClaimLevel::P2
}

/// Refuse overclaim language unless the achieved level allows it.
pub fn forbid_overclaim(text: &str, level: ClaimLevel) -> Result<(), ReleaseError> {
    if level.may_claim_certified() {
        return Ok(());
    }
    let lower = text.to_ascii_lowercase();
    for banned in [
        "console-ready",
        "multiplayer production-ready",
        "certified console",
        "shipped console sku",
        "shipped console SKU",
    ] {
        if lower.contains(&banned.to_ascii_lowercase()) {
            return Err(ReleaseError::claim(format!(
                "{banned} is forbidden at {}",
                level.as_str()
            )));
        }
    }
    if lower.contains("certified") && !lower.contains("not") && !lower.contains("never") {
        return Err(ReleaseError::claim(format!(
            "certified is forbidden at {}",
            level.as_str()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory::ReleaseSigningKey;

    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("klotho-claim-{}-{serial}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn redacted_id_is_not_public_reproduction() {
        let record = EvidenceRecord::compose(
            EvidenceDraft {
                level: ClaimLevel::P1,
                target: PlatformTarget::Gdk,
                date: "2026-09-14".into(),
                package_hash: Hash::from_bytes([1; 32]),
                owner: "Platform Owner".into(),
                expiry: "2027-09-14".into(),
                passed: true,
            },
            b"unredacted hardware log",
        )
        .unwrap();
        record.validate().unwrap();
        let ws = AccessWorkspace::open(scratch());
        let err = ws.resolve(&record).unwrap_err();
        assert!(err.to_string().contains("not public reproduction"));
        assert!(!record.id.to_string().is_empty());
        assert_eq!(record.level, ClaimLevel::P1);
        assert_eq!(record.target, PlatformTarget::Gdk);
        assert!(record.passed);
    }

    #[test]
    fn p1_bind_requires_unredacted_workspace_bytes() {
        let pkg = Hash::from_bytes([2; 32]);
        let record = EvidenceRecord::compose(
            EvidenceDraft {
                level: ClaimLevel::P1,
                target: PlatformTarget::Prospero,
                date: "2026-09-14".into(),
                package_hash: pkg,
                owner: "Platform Owner".into(),
                expiry: "2027-09-14".into(),
                passed: true,
            },
            b"devkit suite",
        )
        .unwrap();
        let ws = AccessWorkspace::open(scratch());
        let signing = ReleaseSigningKey::from_bytes([3; 32]);
        let bound = bind_p1(record.clone(), b"devkit suite", &ws, &signing).unwrap();
        assert_eq!(bound.record.id, record.id);
        assert_eq!(ws.resolve(&record).unwrap(), b"devkit suite");
    }

    #[test]
    fn mock_adapter_cannot_leave_p0() {
        let ws = AccessWorkspace::open(scratch());
        assert_eq!(
            achieved_level(AdapterClass::PublicMock, None, None, &ws),
            ClaimLevel::P0
        );
    }

    #[test]
    fn p2_requires_holder_bound_to_package_and_p1() {
        let pkg = Hash::from_bytes([4; 32]);
        let unredacted = b"named internal hardware";
        let record = EvidenceRecord::compose(
            EvidenceDraft {
                level: ClaimLevel::P1,
                target: PlatformTarget::Gdk,
                date: "2026-09-14".into(),
                package_hash: pkg,
                owner: "Platform Owner".into(),
                expiry: "2027-09-14".into(),
                passed: true,
            },
            unredacted,
        )
        .unwrap();
        let ws = AccessWorkspace::open(scratch());
        let signing = ReleaseSigningKey::from_bytes([5; 32]);
        let p1 = bind_p1(record, unredacted, &ws, &signing).unwrap();
        assert_eq!(
            achieved_level(AdapterClass::Proprietary, Some(&p1), None, &ws),
            ClaimLevel::P1
        );
        let holder = HolderAcceptance {
            holder: "Microsoft".into(),
            package_hash: pkg,
            p1_id: p1.record.id,
            record_hash: hash_bytes(b"holder-letter"),
        };
        assert_eq!(
            achieved_level(AdapterClass::Proprietary, Some(&p1), Some(&holder), &ws),
            ClaimLevel::P2
        );
        let mut drifted = holder.clone();
        drifted.package_hash = Hash::from_bytes([9; 32]);
        assert_eq!(
            achieved_level(AdapterClass::Proprietary, Some(&p1), Some(&drifted), &ws),
            ClaimLevel::P1
        );
    }

    #[test]
    fn overclaim_language_is_rejected_below_p2() {
        let err = forbid_overclaim("certified console SKU", ClaimLevel::P0).unwrap_err();
        assert!(err.to_string().contains("forbidden"));
        forbid_overclaim("P0 public boundary", ClaimLevel::P0).unwrap();
        forbid_overclaim("certified console SKU", ClaimLevel::P2).unwrap();
    }
}
