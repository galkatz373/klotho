//! Public console device-farm protocol (KAI-23).
//!
//! The public farm is mock devices only. A farm row that declares P1 without
//! bound confidential evidence is refused.

use klotho_core::Hash;
use klotho_platform::{AdapterClass, PlatformIdentity, PlatformTarget, ReplayEvidence};
use serde::{Deserialize, Serialize};

use crate::ReleaseError;
use crate::cert::{CertReport, run_mock_suites};
use crate::claim::ClaimLevel;

/// One farm device.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FarmDevice {
    /// Stable device id.
    pub id: String,
    /// Target.
    pub target: PlatformTarget,
    /// SDK revision recorded on the identity.
    pub sdk_revision: String,
    /// Adapter class.
    pub adapter_class: AdapterClass,
}

impl FarmDevice {
    /// Identity minted for this device.
    pub fn identity(&self) -> Result<PlatformIdentity, ReleaseError> {
        if self.id.trim().is_empty() || self.sdk_revision.trim().is_empty() {
            return Err(ReleaseError::cert("farm device is incomplete"));
        }
        if !self.target.is_console() {
            return Err(ReleaseError::cert("farm device is not a console target"));
        }
        let identity = match (self.target, self.adapter_class) {
            (PlatformTarget::Gdk, AdapterClass::PublicMock) => {
                PlatformIdentity::gdk(&self.sdk_revision)
            }
            (PlatformTarget::Prospero, AdapterClass::PublicMock) => {
                PlatformIdentity::prospero(&self.sdk_revision)
            }
            (PlatformTarget::Gdk, AdapterClass::Proprietary) => {
                PlatformIdentity::gdk_proprietary(&self.sdk_revision)
            }
            (PlatformTarget::Prospero, AdapterClass::Proprietary) => {
                PlatformIdentity::prospero_proprietary(&self.sdk_revision)
            }
            (PlatformTarget::Desktop, _) => {
                return Err(ReleaseError::cert("farm device is not a console target"));
            }
        };
        Ok(identity)
    }
}

/// Public or confidential device farm.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceFarm {
    /// Farm id.
    pub id: String,
    /// Devices.
    pub devices: Vec<FarmDevice>,
}

impl DeviceFarm {
    /// Fail closed on empty farms or mixed desktop rows.
    pub fn validate(&self) -> Result<(), ReleaseError> {
        if self.id.trim().is_empty() {
            return Err(ReleaseError::cert("farm id is empty"));
        }
        if self.devices.is_empty() {
            return Err(ReleaseError::cert("farm has no devices"));
        }
        for device in &self.devices {
            device.identity()?;
        }
        Ok(())
    }
}

/// Run every farm device through the public suites.
pub fn run_farm(
    farm: &DeviceFarm,
    replay: ReplayEvidence<'_>,
    package_hash: Hash,
) -> Result<Vec<CertReport>, ReleaseError> {
    farm.validate()?;
    let mut reports = Vec::new();
    for device in &farm.devices {
        if device.adapter_class == AdapterClass::Proprietary {
            return Err(ReleaseError::cert(format!(
                "farm device {} is proprietary; public farm cannot mint P1",
                device.id
            )));
        }
        let report = run_mock_suites(device.identity()?, replay, package_hash)?;
        if report.claim_level != ClaimLevel::P0 {
            return Err(ReleaseError::cert("public farm produced a claim above P0"));
        }
        reports.push(report);
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{Epoch, Hash};
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

    #[test]
    fn public_farm_stays_p0() {
        let farm = DeviceFarm {
            id: "kai-console-farm-public".into(),
            devices: vec![
                FarmDevice {
                    id: "gdk-mock-0".into(),
                    target: PlatformTarget::Gdk,
                    sdk_revision: "public-mock".into(),
                    adapter_class: AdapterClass::PublicMock,
                },
                FarmDevice {
                    id: "prospero-mock-0".into(),
                    target: PlatformTarget::Prospero,
                    sdk_revision: "public-mock".into(),
                    adapter_class: AdapterClass::PublicMock,
                },
            ],
        };
        let reports = run_farm(&farm, replay(b"replay"), Hash::from_bytes([3; 32])).unwrap();
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|r| r.claim_level == ClaimLevel::P0));
    }

    #[test]
    fn proprietary_farm_row_cannot_run_in_public() {
        let farm = DeviceFarm {
            id: "secret-lab".into(),
            devices: vec![FarmDevice {
                id: "gdk-devkit-0".into(),
                target: PlatformTarget::Gdk,
                sdk_revision: "licensed-gdk".into(),
                adapter_class: AdapterClass::Proprietary,
            }],
        };
        let err = run_farm(&farm, replay(b"replay"), Hash::from_bytes([3; 32])).unwrap_err();
        assert!(err.to_string().contains("cannot mint P1"));
    }
}
