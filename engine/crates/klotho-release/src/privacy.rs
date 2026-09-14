//! Privacy, consent, retention, and regional feature configuration.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ReleaseError;

/// First-title privacy manifest packed beside the warp, never as telemetry code.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyManifest {
    /// Player has a recorded consent choice.
    pub consent: bool,
    /// Crash-upload consent, independent of play telemetry.
    pub crash_upload_consent: bool,
    /// Data-export routing identifier.
    pub export_route: String,
    /// Deletion routing identifier.
    pub deletion_route: String,
    /// Retention in days. Zero is refused.
    pub retention_days: u32,
    /// Regional feature flags (`eu`, `jp`, …).
    pub regional: BTreeMap<String, bool>,
    /// Telemetry is aggregated/redacted before authoring suggestions.
    pub telemetry_aggregated: bool,
}

impl PrivacyManifest {
    /// Fail closed on missing consent, routes, retention, or aggregation.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Scan`] when a required field is empty or false.
    pub fn validate(&self) -> Result<(), ReleaseError> {
        if !self.consent {
            return Err(ReleaseError::scan("privacy consent missing"));
        }
        if self.export_route.trim().is_empty() {
            return Err(ReleaseError::scan("privacy export route missing"));
        }
        if self.deletion_route.trim().is_empty() {
            return Err(ReleaseError::scan("privacy deletion route missing"));
        }
        if self.retention_days == 0 {
            return Err(ReleaseError::scan("privacy retention is zero"));
        }
        if !self.telemetry_aggregated {
            return Err(ReleaseError::scan("telemetry must be aggregated"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete() -> PrivacyManifest {
        PrivacyManifest {
            consent: true,
            crash_upload_consent: false,
            export_route: "export.klotho".into(),
            deletion_route: "delete.klotho".into(),
            retention_days: 30,
            regional: BTreeMap::from([("eu".into(), true)]),
            telemetry_aggregated: true,
        }
    }

    #[test]
    fn complete_manifest_validates() {
        complete().validate().unwrap();
    }

    #[test]
    fn missing_consent_or_retention_fails() {
        let mut m = complete();
        m.consent = false;
        assert!(m.validate().is_err());
        m = complete();
        m.retention_days = 0;
        assert!(m.validate().is_err());
    }
}
