//! Desktop platform services and the first-title release factory (KAI-22).
//!
//! Adapters may read Trace, saves, and package identity. They do not mutate
//! Projection, append Trace, or mint Agency. Achievement or telemetry failure
//! never changes gameplay. Cloud-save resolution selects a validated whole
//! save. Agent credentials cannot sign a release (K80). Console SKUs remain
//! KAI-23.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod achievement;
mod cloud;
mod crash;
mod error;
mod factory;
mod migrate;
mod privacy;
mod rating;
mod scan;
mod store;

pub use achievement::{
    AchievementDef, AchievementKind, AchievementQueue, AchievementSink, AchievementUnlock,
    MemoryAchievementSink,
};
pub use cloud::{
    CloudResolution, CloudSlot, CloudStore, ConflictChoice, SaveIdentity, resolve_cloud,
};
pub use crash::{CrashBundle, MappedCrash, SymbolStore, map_crash};
pub use error::ReleaseError;
pub use factory::{
    Approval, FactoryRequest, OperationsRunbook, Principal, REQUIRED_ROLES, ReleaseDashboard,
    ReleaseExtras, ReleaseRole, ReleaseSigningKey, SignedRelease, SignedSku, candidate_build,
    package_hash, verify_release,
};
pub use migrate::migrate_save;
pub use privacy::PrivacyManifest;
pub use rating::{BoardRecord, RatingBoard, RatingEvidence};
pub use scan::{ScanReport, scan_candidate};
pub use store::{DesktopStorefront, DlcPack, InstalledRelease, StagedRollout, StoreIdentity};

#[cfg(test)]
pub(crate) mod tests_support {
    use std::collections::BTreeMap;

    use klotho_compile::{
        AccessibilitySettings, CreditEntry, CreditsRoll, DESKTOP_SKUS, HudSpec, LocaleTable,
        REQUIRED_LOCALE_KEYS, ShipContent,
    };
    use klotho_ir::Name;
    use klotho_prove::{ReleaseRights, RightsRoute, hash_bytes};

    use crate::achievement::{AchievementDef, AchievementKind};
    use crate::factory::{Approval, OperationsRunbook, Principal, ReleaseExtras, ReleaseRole};
    use crate::privacy::PrivacyManifest;
    use crate::rating::{BoardRecord, RatingBoard, RatingEvidence};

    pub fn ship_content() -> ShipContent {
        let strings: BTreeMap<String, String> = REQUIRED_LOCALE_KEYS
            .iter()
            .map(|k| ((*k).to_owned(), format!("text-{k}")))
            .collect();
        ShipContent {
            locales: ["en", "ja", "es"]
                .iter()
                .map(|id| LocaleTable {
                    id: (*id).to_owned(),
                    strings: strings.clone(),
                })
                .collect(),
            credits: CreditsRoll {
                title: "Desktop Release".into(),
                entries: vec![CreditEntry {
                    name: "Klotho".into(),
                    role: "engine".into(),
                }],
            },
            accessibility: AccessibilitySettings {
                remap: true,
                hold_to_toggle: true,
                subtitles: true,
                closed_captions: false,
                text_scale_milli: 1_000,
                contrast: klotho_ir::ContrastMode::Default,
                reduce_motion: true,
                reduce_shake: true,
                screen_reader: false,
            },
            hud: HudSpec {
                slots: vec!["status".into(), "prompt".into(), "notice".into()],
                palette: "production".into(),
            },
            play_recording: "(id:\"critical-path\")".into(),
            notice: "Approved kitbash. No model weights.".into(),
        }
    }

    pub fn extras() -> ReleaseExtras {
        let h = hash_bytes(b"rights-v1");
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
        let mut symbols = BTreeMap::new();
        symbols.insert(0x10, "klotho_runtime::step".into());
        ReleaseExtras {
            sku_allowlist: DESKTOP_SKUS.iter().map(|sku| sku.id.to_owned()).collect(),
            achievements: vec![AchievementDef {
                id: Name::from("opened"),
                kind: AchievementKind::Learned { fact: 1 },
            }],
            ratings: RatingEvidence {
                boards,
                descriptors: vec!["fantasy violence".into()],
                credits: true,
                third_party_notices: true,
                accessibility: true,
            },
            privacy: PrivacyManifest {
                consent: true,
                crash_upload_consent: true,
                export_route: "export.klotho".into(),
                deletion_route: "delete.klotho".into(),
                retention_days: 30,
                regional: BTreeMap::from([("eu".into(), true)]),
                telemetry_aggregated: true,
            },
            rights: vec![ReleaseRights {
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
            }],
            runbook: OperationsRunbook {
                install: "Install the signed desktop package for the SKU.".into(),
                update: "Apply the next signed candidate then verify journeys.".into(),
                rollback: "Restore the previous signed candidate and save.".into(),
                crash: "Map the crash bundle through the symbol store and replay.".into(),
                support: "Collect the signed evidence bundle and privacy-gated logs.".into(),
            },
            symbols,
        }
    }

    pub fn human_approvals() -> Vec<Approval> {
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
                package_hash: klotho_core::Hash::ZERO,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use klotho_compile::{cook_doc, unpack_warp};
    use klotho_core::{Budget, Tick};
    use klotho_runtime::kernel_from_cooked;

    use super::*;
    use crate::tests_support::{extras, human_approvals, ship_content};

    fn scratch() -> std::path::PathBuf {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "klotho-release-{}-{nanos}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn desktop_candidates_install_and_complete_offline_journeys() {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
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
        verify_release(&release).unwrap();
        assert!(release.scan.clean);
        let store = DesktopStorefront::offline();
        let root = scratch();
        for sku in &release.skus {
            let dest = root.join(&sku.sku);
            let installed = store.install(&sku.package, &dest).unwrap();
            let warp = fs::read(dest.join("game.warp")).unwrap();
            let unpacked = unpack_warp(&warp).unwrap();
            let mut kernel = kernel_from_cooked(&unpacked).unwrap();
            kernel
                .step(Tick(1), Budget::HEARTH, &mut [])
                .expect("offline critical step");
            let snap = kernel.snapshot();
            let save = klotho_save::pause_save(&snap).unwrap();
            klotho_save::check_load(&save, snap.trace_prefix_hash, snap.canon_hash).unwrap();
            store.uninstall(installed).unwrap();
        }
        let _ = fs::remove_dir_all(&root);
    }
}
