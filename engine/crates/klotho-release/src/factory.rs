//! One-command desktop candidate build, named human approvals, and signing.

use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use klotho_compile::{Cooked, DESKTOP_SKUS, DesktopPackage, DesktopSku, ShipContent, pack_desktop};
use klotho_core::Hash;
use klotho_ir::{Name, to_ron};
use klotho_prove::{ReleaseRights, hash_bytes};
use serde::{Deserialize, Serialize};

use crate::ReleaseError;
use crate::achievement::AchievementDef;
use crate::crash::SymbolStore;
use crate::privacy::PrivacyManifest;
use crate::rating::RatingEvidence;
use crate::scan::{ScanReport, scan_candidate};

/// Human roles that must separately approve a candidate (K80).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseRole {
    /// Legal / rights owner.
    Legal,
    /// Ratings owner.
    Ratings,
    /// Privacy owner.
    Privacy,
    /// Storefront owner.
    Storefront,
    /// Release owner.
    Release,
    /// Signing authority.
    Signing,
}

impl ReleaseRole {
    /// Stable role name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Legal => "legal",
            Self::Ratings => "ratings",
            Self::Privacy => "privacy",
            Self::Storefront => "storefront",
            Self::Release => "release",
            Self::Signing => "signing",
        }
    }
}

/// Required first-title approval set.
pub const REQUIRED_ROLES: [ReleaseRole; 6] = [
    ReleaseRole::Legal,
    ReleaseRole::Ratings,
    ReleaseRole::Privacy,
    ReleaseRole::Storefront,
    ReleaseRole::Release,
    ReleaseRole::Signing,
];

/// Who is recorded on an approval. Agents cannot sign or Pin a release.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum Principal {
    /// Named human owner.
    Human(Name),
    /// Agent identity. Always rejected for release.
    Agent(Name),
}

/// One named approval bound to a package hash.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    /// Role.
    pub role: ReleaseRole,
    /// Human or (illegal) agent.
    pub principal: Principal,
    /// Package hash this approval covers.
    pub package_hash: Hash,
}

/// Operations runbook shipped with the candidate evidence, not the game.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationsRunbook {
    /// Install steps.
    pub install: String,
    /// Update steps.
    pub update: String,
    /// Rollback steps.
    pub rollback: String,
    /// Crash/symbol handling.
    pub crash: String,
    /// Support diagnostics.
    pub support: String,
}

impl OperationsRunbook {
    fn validate(&self) -> Result<(), ReleaseError> {
        for (name, value) in [
            ("install", self.install.as_str()),
            ("update", self.update.as_str()),
            ("rollback", self.rollback.as_str()),
            ("crash", self.crash.as_str()),
            ("support", self.support.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ReleaseError::scan(format!("runbook {name} empty")));
            }
        }
        Ok(())
    }
}

/// Signing authority key. Loaded from outside the agent sandbox.
pub struct ReleaseSigningKey {
    sk: SigningKey,
}

impl ReleaseSigningKey {
    /// Construct from a 32-byte seed provided by the signing owner.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            sk: SigningKey::from_bytes(&bytes),
        }
    }

    /// 32-byte verifying key recorded on the signed candidate.
    #[must_use]
    pub fn verifying_bytes(&self) -> [u8; 32] {
        self.sk.verifying_key().to_bytes()
    }

    /// Sign arbitrary evidence bytes. Used by console P1 binding.
    pub(crate) fn sign_bytes(&self, msg: &[u8]) -> Result<[u8; 64], crate::ReleaseError> {
        if self.sk.verifying_key().is_weak() {
            return Err(crate::ReleaseError::signature("weak signing key"));
        }
        Ok(self.sk.sign(msg).to_bytes())
    }
}

/// Evidence packed into the candidate besides the cooked warp.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ReleaseExtras {
    /// SKU ids that may be packed. Must be a subset of [`DESKTOP_SKUS`].
    pub sku_allowlist: BTreeSet<String>,
    /// Achievement declarations.
    pub achievements: Vec<AchievementDef>,
    /// Ratings / notices.
    pub ratings: RatingEvidence,
    /// Privacy manifest.
    pub privacy: PrivacyManifest,
    /// Release-rights rows.
    pub rights: Vec<ReleaseRights>,
    /// Operations runbook.
    pub runbook: OperationsRunbook,
    /// Symbol offsets stored beside the package.
    pub symbols: BTreeMap<u64, String>,
}

/// Inputs to the one-command candidate build.
pub struct FactoryRequest<'a> {
    /// Reference cook.
    pub cooked: &'a Cooked,
    /// Locales, credits, HUD, play recording, NOTICE.
    pub content: &'a ShipContent,
    /// Ratings, privacy, rights, allowlist, symbols.
    pub extras: &'a ReleaseExtras,
    /// Named human approvals. Hashes are filled after packing if [`Hash::ZERO`].
    pub approvals: &'a [Approval],
    /// Signing authority key.
    pub signing: &'a ReleaseSigningKey,
}

/// One signed desktop SKU candidate.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SignedSku {
    /// SKU id.
    pub sku: String,
    /// Game package without symbols.
    pub package: DesktopPackage,
    /// Content hash of the package files.
    pub package_hash: Hash,
    /// Ed25519 signature over the package hash.
    pub signature: [u8; 64],
    /// Verifying key of the signing authority.
    pub verifying_key: [u8; 32],
}

/// Factory output: per-SKU packages, shared symbols, and a dashboard.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SignedRelease {
    /// One candidate per allowlisted SKU.
    pub skus: Vec<SignedSku>,
    /// Symbols keyed by package hash. Never inserted into `skus[].package`.
    pub symbols: BTreeMap<Hash, SymbolStore>,
    /// Scan report.
    pub scan: ScanReport,
    /// Dashboard summary.
    pub dashboard: ReleaseDashboard,
}

/// Distaff / ops summary of a candidate.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ReleaseDashboard {
    /// Packed SKU count.
    pub sku_count: usize,
    /// True when every SKU is signed.
    pub signed: bool,
    /// Roles still missing. Empty on a successful build.
    pub pending_roles: Vec<ReleaseRole>,
    /// Claim level. Desktop factory is P0.
    pub claim_level: &'static str,
}

/// Content-addressed hash of a desktop package.
#[must_use]
pub fn package_hash(package: &DesktopPackage) -> Hash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(package.sku.as_bytes());
    for (path, file) in &package.files {
        bytes.extend_from_slice(path.as_bytes());
        bytes.extend_from_slice(&(file.len() as u32).to_le_bytes());
        bytes.extend_from_slice(file);
    }
    hash_bytes(&bytes)
}

/// Pack, scan, approve, and sign every allowlisted desktop SKU.
///
/// # Errors
///
/// Returns [`ReleaseError`] when the allowlist, scan, approvals, or signature fail.
pub fn candidate_build(request: FactoryRequest<'_>) -> Result<SignedRelease, ReleaseError> {
    request.extras.runbook.validate()?;
    let skus = selected_skus(&request.extras.sku_allowlist)?;
    let mut signed = Vec::new();
    let mut symbols = BTreeMap::new();
    let mut last_scan = ScanReport { clean: true };
    for sku in skus {
        let mut package = pack_desktop(request.cooked, sku, request.content)?;
        insert_extra(&mut package, "ratings.ron", &request.extras.ratings)?;
        insert_extra(&mut package, "privacy.ron", &request.extras.privacy)?;
        insert_extra(
            &mut package,
            "achievements.ron",
            &request.extras.achievements,
        )?;
        last_scan = scan_candidate(
            &package,
            &request.extras.rights,
            &request.extras.ratings,
            &request.extras.privacy,
        )?;
        let hash = package_hash(&package);
        check_approvals(request.approvals, hash)?;
        let signature = sign_hash(request.signing, hash)?;
        let store = SymbolStore {
            package_hash: hash,
            entries: request.extras.symbols.clone(),
        };
        if package.files.keys().any(|path| path.contains("symbol")) {
            return Err(ReleaseError::scan("symbols leaked into package"));
        }
        symbols.insert(hash, store);
        signed.push(SignedSku {
            sku: sku.id.to_owned(),
            package,
            package_hash: hash,
            signature,
            verifying_key: request.signing.verifying_bytes(),
        });
    }
    let dashboard = ReleaseDashboard {
        sku_count: signed.len(),
        signed: true,
        pending_roles: Vec::new(),
        claim_level: "P0",
    };
    Ok(SignedRelease {
        skus: signed,
        symbols,
        scan: last_scan,
        dashboard,
    })
}

/// Verify every SKU signature against its package hash.
///
/// # Errors
///
/// Returns [`ReleaseError::Signature`] when a signature or key is invalid.
pub fn verify_release(release: &SignedRelease) -> Result<(), ReleaseError> {
    for sku in &release.skus {
        if package_hash(&sku.package) != sku.package_hash {
            return Err(ReleaseError::signature("package hash drifted"));
        }
        let vk = VerifyingKey::from_bytes(&sku.verifying_key)
            .map_err(|_| ReleaseError::signature("bad verifying key"))?;
        if vk.is_weak() {
            return Err(ReleaseError::signature("weak verifying key"));
        }
        let sig = Signature::from_bytes(&sku.signature);
        vk.verify_strict(sku.package_hash.as_bytes(), &sig)
            .map_err(|_| ReleaseError::signature("signature rejected"))?;
        if let Some(store) = release.symbols.get(&sku.package_hash) {
            if store.package_hash != sku.package_hash {
                return Err(ReleaseError::signature("symbol store hash drifted"));
            }
        }
    }
    Ok(())
}

fn selected_skus(allowlist: &BTreeSet<String>) -> Result<Vec<&'static DesktopSku>, ReleaseError> {
    if allowlist.is_empty() {
        return Err(ReleaseError::scan("SKU allowlist is empty"));
    }
    let mut selected = Vec::new();
    for id in allowlist {
        let sku = DESKTOP_SKUS
            .iter()
            .find(|sku| sku.id == id)
            .ok_or_else(|| ReleaseError::scan(format!("SKU {id} is not a desktop SKU")))?;
        selected.push(sku);
    }
    Ok(selected)
}

fn insert_extra<T: Serialize>(
    package: &mut DesktopPackage,
    path: &str,
    value: &T,
) -> Result<(), ReleaseError> {
    let text = to_ron(value).map_err(|e| ReleaseError::Package(e.to_string()))?;
    package.files.insert(path.into(), text.into_bytes());
    Ok(())
}

pub(crate) fn check_approvals(
    approvals: &[Approval],
    package_hash: Hash,
) -> Result<(), ReleaseError> {
    let mut seen = BTreeSet::new();
    let mut owners = BTreeSet::new();
    for approval in approvals {
        match &approval.principal {
            Principal::Agent(name) => {
                return Err(ReleaseError::approval(format!(
                    "agent {} cannot approve a release",
                    name.as_str()
                )));
            }
            Principal::Human(name) => {
                if approval.package_hash != Hash::ZERO && approval.package_hash != package_hash {
                    return Err(ReleaseError::approval(format!(
                        "{} approval is for another package",
                        approval.role.as_str()
                    )));
                }
                if !seen.insert(approval.role) {
                    return Err(ReleaseError::approval(format!(
                        "duplicate {} approval",
                        approval.role.as_str()
                    )));
                }
                if !owners.insert(name.as_str()) {
                    return Err(ReleaseError::approval(format!(
                        "{} is not a separate named owner",
                        name.as_str()
                    )));
                }
            }
        }
    }
    for role in REQUIRED_ROLES {
        if !seen.contains(&role) {
            return Err(ReleaseError::approval(format!(
                "missing {} approval",
                role.as_str()
            )));
        }
    }
    Ok(())
}

fn sign_hash(key: &ReleaseSigningKey, hash: Hash) -> Result<[u8; 64], ReleaseError> {
    let vk = key.sk.verifying_key();
    if vk.is_weak() {
        return Err(ReleaseError::signature("weak signing key"));
    }
    Ok(key.sk.sign(hash.as_bytes()).to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{extras, human_approvals, ship_content};

    fn cooked() -> Cooked {
        klotho_compile::cook_doc(&hearth_slice::hearth_doc()).unwrap()
    }

    #[test]
    fn candidate_build_signs_three_desktop_skus() {
        let cooked = cooked();
        let content = ship_content();
        let extras = extras();
        let approvals = human_approvals();
        let signing = ReleaseSigningKey::from_bytes([7; 32]);
        let release = candidate_build(FactoryRequest {
            cooked: &cooked,
            content: &content,
            extras: &extras,
            approvals: &approvals,
            signing: &signing,
        })
        .unwrap();
        assert_eq!(release.skus.len(), 3);
        assert!(release.dashboard.signed);
        assert_eq!(release.dashboard.claim_level, "P0");
        verify_release(&release).unwrap();
        for sku in &release.skus {
            assert!(sku.package.files.contains_key("game.warp"));
            assert!(sku.package.files.contains_key("ratings.ron"));
            assert!(sku.package.files.contains_key("privacy.ron"));
            assert!(sku.package.files.keys().all(|p| !p.contains("symbol")));
            assert!(release.symbols.contains_key(&sku.package_hash));
        }
    }

    #[test]
    fn agent_approval_is_rejected() {
        let cooked = cooked();
        let content = ship_content();
        let extras = extras();
        let mut approvals = human_approvals();
        approvals[5].principal = Principal::Agent(Name::from("authoring-agent"));
        let signing = ReleaseSigningKey::from_bytes([7; 32]);
        let err = candidate_build(FactoryRequest {
            cooked: &cooked,
            content: &content,
            extras: &extras,
            approvals: &approvals,
            signing: &signing,
        })
        .unwrap_err();
        assert!(err.to_string().contains("agent"));
    }

    #[test]
    fn unknown_sku_is_rejected() {
        let cooked = cooked();
        let content = ship_content();
        let mut extras = extras();
        extras.sku_allowlist.insert("gdk-d3d12-high".into());
        let approvals = human_approvals();
        let signing = ReleaseSigningKey::from_bytes([7; 32]);
        assert!(
            candidate_build(FactoryRequest {
                cooked: &cooked,
                content: &content,
                extras: &extras,
                approvals: &approvals,
                signing: &signing,
            })
            .is_err()
        );
    }
}
