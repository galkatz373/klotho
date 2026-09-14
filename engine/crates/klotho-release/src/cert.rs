//! Console certification checklist and compliance suites (KAI-23).
//!
//! Suites run against a [`PlatformHal`]. Public CI uses [`MemoryPlatformHal`].
//! Passing a mock suite is P0, never hardware validation.

use klotho_core::Hash;
use klotho_platform::{MemoryPlatformHal, PlatformHal, PlatformIdentity, ReplayEvidence};
use serde::{Deserialize, Serialize};

use crate::ReleaseError;
use crate::claim::ClaimLevel;

/// Integer budgets for the public console compliance suites.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct CertBudgets {
    /// Present CPU microseconds.
    pub frame_us: u32,
    /// Present GPU microseconds.
    pub gpu_us: u32,
    /// Resident bytes.
    pub memory_bytes: u64,
    /// Occupied save bytes.
    pub storage_bytes: u64,
    /// Network latency microseconds.
    pub latency_us: u32,
    /// Network loss parts-per-million.
    pub loss_ppm: u32,
}

impl CertBudgets {
    /// First-title High-tier console mock budgets.
    pub const HIGH: Self = Self {
        frame_us: 16_666,
        gpu_us: 11_000,
        memory_bytes: 8 * 1024 * 1024 * 1024,
        storage_bytes: 50 * 1024 * 1024 * 1024,
        latency_us: 50_000,
        loss_ppm: 10_000,
    };
}

/// One checklist row. Names are stable; they are not holder TRC numbers.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecklistItem {
    /// Stable id (`kernel-replay`).
    pub id: String,
    /// Domain covered.
    pub domain: String,
}

/// Public certification checklist.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertChecklist {
    /// Checklist version.
    pub version: u16,
    /// Required items.
    pub items: Vec<ChecklistItem>,
}

/// Required public suite ids.
pub const REQUIRED_SUITES: [&str; 8] = [
    "kernel-replay",
    "presentation",
    "memory",
    "storage",
    "suspend-resume",
    "controller",
    "accessibility",
    "network",
];

impl CertChecklist {
    /// Public default checklist.
    #[must_use]
    pub fn required() -> Self {
        Self {
            version: 1,
            items: REQUIRED_SUITES
                .iter()
                .map(|id| ChecklistItem {
                    id: (*id).to_owned(),
                    domain: (*id).to_owned(),
                })
                .collect(),
        }
    }

    /// Every required suite is present and version is 1.
    pub fn validate(&self) -> Result<(), ReleaseError> {
        if self.version != 1 {
            return Err(ReleaseError::cert("checklist version must be 1"));
        }
        for id in REQUIRED_SUITES {
            if !self.items.iter().any(|item| item.id == id) {
                return Err(ReleaseError::cert(format!("checklist missing {id}")));
            }
        }
        Ok(())
    }
}

/// Result of running the public suites against one adapter.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CertReport {
    /// Adapter identity.
    pub identity: PlatformIdentity,
    /// Suites that passed.
    pub passed: Vec<String>,
    /// Achieved claim. Mock adapters stay P0.
    pub claim_level: ClaimLevel,
    /// Package the suites were bound to.
    pub package_hash: Hash,
}

/// Run the public compliance suites.
pub fn run_suites(
    hal: &mut impl PlatformHal,
    replay: ReplayEvidence<'_>,
    package_hash: Hash,
    budgets: CertBudgets,
    checklist: &CertChecklist,
) -> Result<CertReport, ReleaseError> {
    checklist.validate()?;
    let identity = hal.identity().clone();
    if !identity.is_valid() {
        return Err(ReleaseError::cert("platform target/backend mismatch"));
    }
    if identity.target.is_console() && !identity.is_console_native() {
        return Err(ReleaseError::cert(
            "desktop wgpu is not a console certification path",
        ));
    }

    hal.record_replay(replay)
        .map_err(|e| ReleaseError::cert(e.to_string()))?;

    let present = hal
        .sample_present()
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if present.frame_us > budgets.frame_us || present.gpu_us > budgets.gpu_us {
        return Err(ReleaseError::cert("presentation over budget"));
    }

    let memory = hal
        .sample_memory()
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if memory.resident_bytes > budgets.memory_bytes {
        return Err(ReleaseError::cert("memory over budget"));
    }

    hal.write_storage(0, b"console-save")
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    let loaded = hal
        .read_storage(0)
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if loaded != b"console-save" {
        return Err(ReleaseError::cert("storage round-trip drifted"));
    }
    let storage = hal
        .sample_storage()
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if storage.used_bytes > budgets.storage_bytes {
        return Err(ReleaseError::cert("storage over budget"));
    }

    let token = hal
        .suspend()
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if hal.write_storage(1, b"nope").is_ok() {
        return Err(ReleaseError::cert("storage mutated while suspended"));
    }
    hal.resume(token)
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    let after = hal
        .read_storage(0)
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if after != b"console-save" {
        return Err(ReleaseError::cert("save did not survive suspend/resume"));
    }

    let controller = hal
        .sample_controller()
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if !controller.connected || !controller.remap || controller.buttons == 0 {
        return Err(ReleaseError::cert("controller suite failed"));
    }

    let a11y = hal
        .sample_accessibility()
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if !a11y.remap || !a11y.subtitles || a11y.text_scale_milli == 0 {
        return Err(ReleaseError::cert("accessibility suite failed"));
    }

    let network = hal
        .sample_network()
        .map_err(|e| ReleaseError::cert(e.to_string()))?;
    if network.latency_us > budgets.latency_us || network.loss_ppm > budgets.loss_ppm {
        return Err(ReleaseError::cert("network over budget"));
    }

    // Suites never raise the claim. P1/P2 require bound confidential evidence.
    let claim_level = ClaimLevel::P0;

    Ok(CertReport {
        identity,
        passed: REQUIRED_SUITES.iter().map(|s| (*s).to_owned()).collect(),
        claim_level,
        package_hash,
    })
}

/// Import and validate a public checklist.
pub fn import_checklist(checklist: &CertChecklist) -> Result<(), ReleaseError> {
    checklist.validate()
}

/// Run the public suites on a mock HAL.
pub fn run_mock_suites(
    identity: PlatformIdentity,
    replay: ReplayEvidence<'_>,
    package_hash: Hash,
) -> Result<CertReport, ReleaseError> {
    let mut hal =
        MemoryPlatformHal::new(identity).map_err(|e| ReleaseError::cert(e.to_string()))?;
    run_suites(
        &mut hal,
        replay,
        package_hash,
        CertBudgets::HIGH,
        &CertChecklist::required(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{Epoch, Hash};
    use klotho_platform::{AdapterClass, GraphicsApi, PlatformTarget, ReplayEvidence};

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

    #[test]
    fn mock_gdk_suites_pass_at_p0() {
        let report = run_mock_suites(
            PlatformIdentity::gdk("public-mock"),
            replay(b"replay"),
            Hash::from_bytes([7; 32]),
        )
        .unwrap();
        assert_eq!(report.claim_level, ClaimLevel::P0);
        assert_eq!(report.passed.len(), REQUIRED_SUITES.len());
        assert!(!report.claim_level.may_claim_certified());
    }

    #[test]
    fn mock_prospero_suites_pass_at_p0() {
        let report = run_mock_suites(
            PlatformIdentity::prospero("public-mock"),
            replay(b"replay"),
            Hash::from_bytes([8; 32]),
        )
        .unwrap();
        assert_eq!(report.claim_level, ClaimLevel::P0);
        assert_eq!(report.identity.target, PlatformTarget::Prospero);
    }

    #[test]
    fn wgpu_console_identity_is_rejected() {
        let identity = PlatformIdentity {
            target: PlatformTarget::Gdk,
            graphics: GraphicsApi::DesktopWgpu,
            sdk_revision: "wgpu".into(),
            adapter_class: AdapterClass::PublicMock,
        };
        let err =
            run_mock_suites(identity, replay(b"replay"), Hash::from_bytes([1; 32])).unwrap_err();
        assert!(err.to_string().contains("backend") || err.to_string().contains("wgpu"));
    }

    #[test]
    fn present_over_budget_fails() {
        let mut hal = MemoryPlatformHal::new(PlatformIdentity::gdk("public-mock")).unwrap();
        hal.set_present_frame_us(40_000);
        let err = run_suites(
            &mut hal,
            replay(b"replay"),
            Hash::from_bytes([1; 32]),
            CertBudgets::HIGH,
            &CertChecklist::required(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("presentation"));
    }

    #[test]
    fn proprietary_without_p1_still_reports_p0() {
        let report = run_mock_suites(
            PlatformIdentity::gdk_proprietary("licensed-gdk"),
            replay(b"replay"),
            Hash::from_bytes([3; 32]),
        )
        .unwrap();
        assert_eq!(report.claim_level, ClaimLevel::P0);
    }
}
