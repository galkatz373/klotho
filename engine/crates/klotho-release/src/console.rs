//! Console SKU packaging, signing, and submission workflow (KAI-23).
//!
//! Public candidates stay at the achieved claim level. They never advertise a
//! certified or shipped console SKU without P2 holder acceptance.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use klotho_core::Hash;
use klotho_ir::to_ron;
use klotho_platform::{GraphicsApi, PlatformIdentity, PlatformTarget, ReplayEvidence};
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::ReleaseError;
use crate::cert::{CertReport, run_mock_suites};
use crate::claim::{
    AccessWorkspace, BoundP1, ClaimLevel, EvidenceRecord, HolderAcceptance, achieved_level,
    forbid_overclaim,
};
use crate::factory::{Approval, ReleaseSigningKey, check_approvals};
use crate::farm::{DeviceFarm, run_farm};

/// Public console SKU identities. Native HAL only.
pub const CONSOLE_SKUS: [ConsoleSku; 2] = [
    ConsoleSku {
        id: "gdk-d3d12-high",
        target: PlatformTarget::Gdk,
        graphics: GraphicsApi::GdkD3d12,
        resolution: "3840x2160",
        quality: "high",
        refresh_hz: 60,
    },
    ConsoleSku {
        id: "prospero-gnm-high",
        target: PlatformTarget::Prospero,
        graphics: GraphicsApi::ProsperoGnmAgc,
        resolution: "3840x2160",
        quality: "high",
        refresh_hz: 60,
    },
];

/// One console SKU identity.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ConsoleSku {
    /// Stable SKU id.
    pub id: &'static str,
    /// Console target.
    pub target: PlatformTarget,
    /// Native graphics API.
    pub graphics: GraphicsApi,
    /// Locked resolution.
    pub resolution: &'static str,
    /// Quality tier.
    pub quality: &'static str,
    /// Refresh.
    pub refresh_hz: u16,
}

impl ConsoleSku {
    /// Lookup by id.
    pub fn find(id: &str) -> Result<&'static Self, ReleaseError> {
        CONSOLE_SKUS
            .iter()
            .find(|sku| sku.id == id)
            .ok_or_else(|| ReleaseError::scan(format!("SKU {id} is not a console SKU")))
    }

    fn identity(self, sdk_revision: &str) -> PlatformIdentity {
        match self.target {
            PlatformTarget::Gdk => PlatformIdentity::gdk(sdk_revision),
            PlatformTarget::Prospero => PlatformIdentity::prospero(sdk_revision),
            PlatformTarget::Desktop => PlatformIdentity::desktop(sdk_revision),
        }
    }
}

/// Packed console candidate. Symbols stay out of `files`.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ConsolePackage {
    /// SKU id.
    pub sku: String,
    /// Relative path → bytes.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Signed console SKU plus the achieved claim.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SignedConsoleSku {
    /// SKU id.
    pub sku: String,
    /// Packed files.
    pub package: ConsolePackage,
    /// Content hash of the package files.
    pub package_hash: Hash,
    /// Ed25519 signature over the package hash.
    pub signature: [u8; 64],
    /// Verifying key.
    pub verifying_key: [u8; 32],
    /// Achieved claim. Public builds are P0.
    pub claim_level: ClaimLevel,
}

/// Console factory output.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ConsoleRelease {
    /// Signed SKUs.
    pub skus: Vec<SignedConsoleSku>,
    /// Per-SKU cert reports.
    pub reports: Vec<CertReport>,
    /// Dashboard.
    pub dashboard: ConsoleDashboard,
}

/// Distaff / ops summary of a console candidate.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ConsoleDashboard {
    /// Packed SKU count.
    pub sku_count: usize,
    /// True when every SKU is signed.
    pub signed: bool,
    /// Achieved claim. Never "certified" below P2.
    pub claim_level: ClaimLevel,
    /// Public statement matching `claim_level`.
    pub public_statement: &'static str,
}

/// Inputs to a console candidate build.
pub struct ConsoleFactoryRequest<'a> {
    /// Warp bytes packed as `game.warp`.
    pub warp: &'a [u8],
    /// SKU ids to pack. Must be a subset of [`CONSOLE_SKUS`].
    pub sku_ids: &'a [String],
    /// SDK revision recorded on mock identities.
    pub sdk_revision: &'a str,
    /// Named human approvals.
    pub approvals: &'a [Approval],
    /// Signing authority.
    pub signing: &'a ReleaseSigningKey,
    /// Optional bound P1. Public CI leaves this `None`.
    pub p1: Option<&'a BoundP1>,
    /// Optional holder acceptance. Public CI leaves this `None`.
    pub holder: Option<&'a HolderAcceptance>,
    /// Access-controlled workspace used to resolve P1.
    pub workspace: &'a AccessWorkspace,
    /// Kernel replay used by the suites.
    pub replay: ReplayEvidence<'a>,
}

/// Pack, scan, approve, sign, and run mock suites for console SKUs.
pub fn console_candidate_build(
    request: ConsoleFactoryRequest<'_>,
) -> Result<ConsoleRelease, ReleaseError> {
    if request.warp.is_empty() {
        return Err(ReleaseError::scan("console warp is empty"));
    }
    if request.sku_ids.is_empty() {
        return Err(ReleaseError::scan("console SKU allowlist is empty"));
    }
    let mut signed = Vec::new();
    let mut reports = Vec::new();
    let mut claim = ClaimLevel::P0;
    for id in request.sku_ids {
        let sku = ConsoleSku::find(id)?;
        if !sku.target.is_console() || matches!(sku.graphics, GraphicsApi::DesktopWgpu) {
            return Err(ReleaseError::cert(
                "desktop wgpu is not a console certification path",
            ));
        }
        let identity = sku.identity(request.sdk_revision);
        let mut files = BTreeMap::new();
        files.insert("game.warp".into(), request.warp.to_vec());
        files.insert(
            "sku.ron".into(),
            to_ron(&ConsoleSkuWire {
                id: sku.id,
                target: sku.target.as_str(),
                graphics_api: sku.graphics.as_str(),
                resolution: sku.resolution,
                quality: sku.quality,
                refresh_hz: sku.refresh_hz,
                claim_level: ClaimLevel::P0.as_str(),
            })
            .map_err(|e| ReleaseError::Package(e.to_string()))?
            .into_bytes(),
        );
        let package = ConsolePackage {
            sku: sku.id.to_owned(),
            files,
        };
        let package_hash = console_package_hash(&package);
        check_approvals(request.approvals, package_hash)?;
        let signature = request.signing.sign_bytes(package_hash.as_bytes())?;
        let sku_claim = achieved_level(
            identity.adapter_class,
            request.p1,
            request.holder,
            request.workspace,
        );
        if let Some(p1) = request.p1 {
            if p1.record.package_hash != package_hash {
                return Err(ReleaseError::claim(
                    "P1 evidence is bound to another package",
                ));
            }
        }
        forbid_overclaim(sku_claim.public_statement(), sku_claim)?;
        if sku_claim > claim {
            claim = sku_claim;
        }
        let report = run_mock_suites(identity, request.replay, package_hash)?;
        reports.push(report);
        signed.push(SignedConsoleSku {
            sku: sku.id.to_owned(),
            package,
            package_hash,
            signature,
            verifying_key: request.signing.verifying_bytes(),
            claim_level: sku_claim,
        });
    }
    Ok(ConsoleRelease {
        skus: signed,
        reports,
        dashboard: ConsoleDashboard {
            sku_count: request.sku_ids.len(),
            signed: true,
            claim_level: claim,
            public_statement: claim.public_statement(),
        },
    })
}

/// Content-addressed hash of a console package.
#[must_use]
pub fn console_package_hash(package: &ConsolePackage) -> Hash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(package.sku.as_bytes());
    for (path, file) in &package.files {
        bytes.extend_from_slice(path.as_bytes());
        bytes.extend_from_slice(&(file.len() as u32).to_le_bytes());
        bytes.extend_from_slice(file);
    }
    hash_bytes(&bytes)
}

#[derive(Serialize)]
struct ConsoleSkuWire {
    id: &'static str,
    target: &'static str,
    graphics_api: &'static str,
    resolution: &'static str,
    quality: &'static str,
    refresh_hz: u16,
    claim_level: &'static str,
}

/// Submission record produced for Distaff / cert import.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertSubmission {
    /// SKU id.
    pub sku: String,
    /// Package hash.
    pub package_hash: Hash,
    /// Achieved claim.
    pub claim_level: ClaimLevel,
    /// Public statement.
    pub public_statement: String,
    /// Public evidence id, if any.
    pub evidence_id: Option<Hash>,
}

/// Build a submission from a signed SKU. Does not claim certified below P2.
pub fn submit_console_sku(
    sku: &SignedConsoleSku,
    evidence: Option<&EvidenceRecord>,
) -> Result<CertSubmission, ReleaseError> {
    forbid_overclaim(sku.claim_level.public_statement(), sku.claim_level)?;
    if let Some(record) = evidence {
        record.validate()?;
        if record.package_hash != sku.package_hash {
            return Err(ReleaseError::claim("evidence is bound to another package"));
        }
    }
    Ok(CertSubmission {
        sku: sku.sku.clone(),
        package_hash: sku.package_hash,
        claim_level: sku.claim_level,
        public_statement: sku.claim_level.public_statement().to_owned(),
        evidence_id: evidence.map(|r| r.id),
    })
}

/// Import a public redacted evidence record. Does not treat it as reproduction.
pub fn import_redacted(record: EvidenceRecord) -> Result<EvidenceRecord, ReleaseError> {
    record.validate()?;
    Ok(record)
}

/// Confirm the public access-controlled workspace layout.
pub fn public_workspace_layout(repo: &Path) -> Result<(PathBuf, PathBuf), ReleaseError> {
    let gdk = repo.join("private/gdk/README.md");
    let prospero = repo.join("private/prospero/README.md");
    if !gdk.is_file() {
        return Err(ReleaseError::cert("missing private/gdk/README.md"));
    }
    if !prospero.is_file() {
        return Err(ReleaseError::cert("missing private/prospero/README.md"));
    }
    Ok((gdk, prospero))
}

/// Run the public farm and keep the claim at P0.
pub fn public_farm_gate(
    farm: &DeviceFarm,
    replay: ReplayEvidence<'_>,
    package_hash: Hash,
) -> Result<ClaimLevel, ReleaseError> {
    let reports = run_farm(farm, replay, package_hash)?;
    if reports.iter().any(|r| r.claim_level != ClaimLevel::P0) {
        return Err(ReleaseError::cert("public farm left P0"));
    }
    Ok(ClaimLevel::P0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory::{Approval, Principal, REQUIRED_ROLES, ReleaseRole, ReleaseSigningKey};
    use klotho_core::{Epoch, Hash};
    use klotho_ir::Name;
    use klotho_platform::ReplayEvidence;

    fn replay<'a>(bytes: &'a [u8]) -> ReplayEvidence<'a> {
        ReplayEvidence {
            label: "kernel",
            canon_hash: Hash::from_bytes([1; 32]),
            epoch: Epoch(1),
            expected_trace_prefix_hash: Hash::from_bytes([2; 32]),
            observed_trace_prefix_hash: Hash::from_bytes([2; 32]),
            replay: bytes,
        }
    }

    fn approvals() -> Vec<Approval> {
        let owners = [
            (ReleaseRole::Legal, "Legal Owner"),
            (ReleaseRole::Ratings, "Ratings Owner"),
            (ReleaseRole::Privacy, "Privacy Owner"),
            (ReleaseRole::Storefront, "Storefront Owner"),
            (ReleaseRole::Release, "Release Owner"),
            (ReleaseRole::Signing, "Signing Owner"),
        ];
        owners
            .into_iter()
            .map(|(role, owner)| Approval {
                role,
                principal: Principal::Human(Name::from(owner)),
                package_hash: Hash::ZERO,
            })
            .collect()
    }

    #[test]
    fn public_console_candidates_stay_p0() {
        let ws = AccessWorkspace::open(std::env::temp_dir().join("klotho-console-empty"));
        let signing = ReleaseSigningKey::from_bytes([7; 32]);
        let ids = vec!["gdk-d3d12-high".into(), "prospero-gnm-high".into()];
        let release = console_candidate_build(ConsoleFactoryRequest {
            warp: b"warp-bytes",
            sku_ids: &ids,
            sdk_revision: "public-mock",
            approvals: &approvals(),
            signing: &signing,
            p1: None,
            holder: None,
            workspace: &ws,
            replay: replay(b"replay"),
        })
        .unwrap();
        assert_eq!(release.skus.len(), 2);
        assert_eq!(release.dashboard.claim_level, ClaimLevel::P0);
        assert_eq!(release.dashboard.public_statement, "P0 public boundary");
        assert!(!release.dashboard.claim_level.may_claim_certified());
        for sku in &release.skus {
            assert_eq!(sku.claim_level, ClaimLevel::P0);
            let submission = submit_console_sku(sku, None).unwrap();
            assert_eq!(submission.claim_level, ClaimLevel::P0);
            assert!(submission.evidence_id.is_none());
        }
        assert_eq!(REQUIRED_ROLES.len(), 6);
    }

    #[test]
    fn public_workspace_layout_is_present() {
        // KAI-01: engine tests also run from a clean export containing only
        // `engine/`, so never read the real repo root (`../../..`) here.
        // Build a scratch repo layout instead and check the contract.
        use std::sync::atomic::{AtomicU64, Ordering};
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let repo = std::env::temp_dir().join(format!(
            "klotho-console-layout-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join("private/gdk")).unwrap();
        std::fs::create_dir_all(repo.join("private/prospero")).unwrap();
        std::fs::write(repo.join("private/gdk/README.md"), b"gdk").unwrap();
        std::fs::write(repo.join("private/prospero/README.md"), b"prospero").unwrap();
        let (gdk, prospero) = public_workspace_layout(&repo).unwrap();
        assert_eq!(gdk, repo.join("private/gdk/README.md"));
        assert_eq!(prospero, repo.join("private/prospero/README.md"));
        assert!(
            public_workspace_layout(&std::env::temp_dir().join(format!(
                "klotho-console-layout-missing-{}-{serial}",
                std::process::id()
            )))
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn unknown_console_sku_is_rejected() {
        let ws = AccessWorkspace::open(std::env::temp_dir().join("klotho-console-empty"));
        let signing = ReleaseSigningKey::from_bytes([7; 32]);
        let ids = vec!["win-d3d12-high".into()];
        let err = console_candidate_build(ConsoleFactoryRequest {
            warp: b"warp-bytes",
            sku_ids: &ids,
            sdk_revision: "public-mock",
            approvals: &approvals(),
            signing: &signing,
            p1: None,
            holder: None,
            workspace: &ws,
            replay: replay(b"replay"),
        })
        .unwrap_err();
        assert!(err.to_string().contains("not a console SKU"));
    }
}
