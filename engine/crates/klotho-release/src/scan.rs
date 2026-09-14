//! Package, rights, privacy, and symbol-split scans.

use klotho_compile::{DesktopPackage, check_ship_allowlist};
use klotho_prove::ReleaseRights;

use crate::ReleaseError;
use crate::privacy::PrivacyManifest;
use crate::rating::RatingEvidence;

const PLACEHOLDERS: [&str; 6] = ["todo", "tbd", "fixme", "placeholder", "lorem ipsum", "xxxx"];

/// Result of scanning a candidate package plus its evidence.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ScanReport {
    /// True when every check passed.
    pub clean: bool,
}

/// Scan ship bytes, rights, ratings, and privacy. Symbols must be absent.
///
/// # Errors
///
/// Returns [`ReleaseError::Scan`] on the first closed failure.
pub fn scan_candidate(
    package: &DesktopPackage,
    rights: &[ReleaseRights],
    ratings: &RatingEvidence,
    privacy: &PrivacyManifest,
) -> Result<ScanReport, ReleaseError> {
    ratings.validate()?;
    privacy.validate()?;
    for right in rights {
        right
            .validate()
            .map_err(|e| ReleaseError::scan(e.to_string()))?;
    }
    if rights.is_empty() {
        return Err(ReleaseError::scan("release rights matrix is empty"));
    }
    for (path, bytes) in &package.files {
        check_ship_allowlist(path).map_err(|e| ReleaseError::scan(e.to_string()))?;
        if path.contains("symbol")
            || path.contains("source-map")
            || path.ends_with(".pdb")
            || path.ends_with(".gguf")
            || path.ends_with(".rs")
            || path.contains("studio/")
            || path.contains("models/")
        {
            return Err(ReleaseError::scan(format!("symbol/privacy split: {path}")));
        }
        if let Ok(text) = std::str::from_utf8(bytes) {
            reject_placeholders(path, text)?;
            reject_pii(path, text)?;
        }
    }
    Ok(ScanReport { clean: true })
}

fn reject_placeholders(path: &str, text: &str) -> Result<(), ReleaseError> {
    let lower = text.to_ascii_lowercase();
    for needle in PLACEHOLDERS {
        if lower.contains(needle) {
            return Err(ReleaseError::scan(format!(
                "placeholder `{needle}` in {path}"
            )));
        }
    }
    Ok(())
}

fn reject_pii(path: &str, text: &str) -> Result<(), ReleaseError> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("ssn") || lower.contains("password=") {
        return Err(ReleaseError::scan(format!("pii token in {path}")));
    }
    for token in text.split_whitespace() {
        if token.contains('@') && token.contains('.') {
            return Err(ReleaseError::scan(format!("email-like pii in {path}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use klotho_compile::DesktopPackage;
    use klotho_core::Hash;
    use klotho_prove::{ReleaseRights, RightsRoute, hash_bytes};

    use super::*;
    use crate::privacy::PrivacyManifest;
    use crate::rating::{BoardRecord, RatingBoard, RatingEvidence};

    fn rights() -> ReleaseRights {
        let h = hash_bytes(b"rights");
        ReleaseRights {
            route: RightsRoute::Commissioned,
            origin: h,
            terms: h,
            ownership: h,
            indemnity: h,
            source_permission: h,
            consent: h,
            restrictions: h,
            approved_by: "Legal Owner".into(),
            approval: h,
        }
    }

    fn ratings() -> RatingEvidence {
        let record = BoardRecord {
            completed: true,
            questionnaire_hash: hash_bytes(b"q"),
            capture_hash: hash_bytes(b"c"),
        };
        let mut boards = BTreeMap::new();
        for board in [
            RatingBoard::Esrb,
            RatingBoard::Pegi,
            RatingBoard::Usk,
            RatingBoard::Iarc,
        ] {
            boards.insert(board, record.clone());
        }
        RatingEvidence {
            boards,
            descriptors: vec!["fantasy violence".into()],
            credits: true,
            third_party_notices: true,
            accessibility: true,
        }
    }

    fn privacy() -> PrivacyManifest {
        PrivacyManifest {
            consent: true,
            crash_upload_consent: true,
            export_route: "export.klotho".into(),
            deletion_route: "delete.klotho".into(),
            retention_days: 30,
            regional: BTreeMap::new(),
            telemetry_aggregated: true,
        }
    }

    fn pkg(files: BTreeMap<String, Vec<u8>>) -> DesktopPackage {
        DesktopPackage {
            sku: "win-d3d12-high".into(),
            files,
        }
    }

    #[test]
    fn clean_package_passes() {
        let mut files = BTreeMap::new();
        files.insert("NOTICE".into(), b"Approved kitbash.".to_vec());
        scan_candidate(&pkg(files), &[rights()], &ratings(), &privacy()).unwrap();
    }

    #[test]
    fn symbols_and_email_fail() {
        let mut files = BTreeMap::new();
        files.insert("symbols/game.pdb".into(), b"debug".to_vec());
        assert!(scan_candidate(&pkg(files.clone()), &[rights()], &ratings(), &privacy()).is_err());
        files.clear();
        files.insert("NOTICE".into(), b"contact release@example.com".to_vec());
        assert!(scan_candidate(&pkg(files), &[rights()], &ratings(), &privacy()).is_err());
    }

    #[test]
    fn zero_hash_rights_fail() {
        let mut bad = rights();
        bad.approval = Hash::ZERO;
        let mut files = BTreeMap::new();
        files.insert("NOTICE".into(), b"ok".to_vec());
        assert!(scan_candidate(&pkg(files), &[bad], &ratings(), &privacy()).is_err());
    }
}
