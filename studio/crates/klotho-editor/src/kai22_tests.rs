//! KAI-22: desktop platform services and release factory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use klotho_compile::{
    AccessibilitySettings, CreditEntry, CreditsRoll, DESKTOP_SKUS, HudSpec, LocaleTable,
    REQUIRED_LOCALE_KEYS, ShipContent, cook_doc,
};
use klotho_core::{Hash, LocusKind};
use klotho_ir::{IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent, from_ron};
use klotho_prove::ReleaseRights;
use klotho_release::{
    AchievementDef, Approval, DesktopStorefront, FactoryRequest, OperationsRunbook, Principal,
    PrivacyManifest, REQUIRED_ROLES, RatingEvidence, ReleaseExtras, ReleaseRole, ReleaseSigningKey,
    candidate_build, verify_release,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactorySpec {
    skus: Vec<String>,
    claim_level: String,
    offline: bool,
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/desktop-release")
}

fn read<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    from_ron(&fs::read_to_string(fixture().join(name)).unwrap()).unwrap()
}

fn no_rust_below(path: &Path) -> bool {
    fs::read_dir(path).unwrap().all(|entry| {
        let path = entry.unwrap().path();
        if path.is_dir() {
            no_rust_below(&path)
        } else {
            path.extension().and_then(|ext| ext.to_str()) != Some("rs")
        }
    })
}

fn no_placeholders(path: &Path) {
    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            no_placeholders(&entry.unwrap().path());
        }
        return;
    }
    if path.file_name().and_then(|name| name.to_str()) == Some("README.md") {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let lower = text.to_ascii_lowercase();
    for needle in ["todo", "tbd", "fixme", "placeholder", "lorem ipsum", "xxxx"] {
        assert!(
            !lower.contains(needle),
            "{} contains {needle}",
            path.display()
        );
    }
}

fn ship_content() -> ShipContent {
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
        play_recording: fs::read_to_string(fixture().join("journeys/offline-critical.ron"))
            .unwrap(),
        notice: "Approved kitbash. No model weights.".into(),
    }
}

fn extras() -> ReleaseExtras {
    let spec: FactorySpec = read("factory.ron");
    let mut symbols = BTreeMap::new();
    symbols.insert(0x10, "klotho_runtime::step".into());
    ReleaseExtras {
        sku_allowlist: spec.skus.into_iter().collect(),
        achievements: read("achievements.ron"),
        ratings: read("ratings.ron"),
        privacy: read("privacy.ron"),
        rights: read("rights.ron"),
        runbook: read("runbook.ron"),
        symbols,
    }
}

fn tiny_doc() -> IntentDoc {
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs: Vec::new(),
        seed: vec![SeedFact::Locus {
            name: Name::from("player"),
            kind: LocusKind::Actor,
        }],
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

#[test]
fn fixture_is_data_only_and_complete() {
    no_placeholders(&fixture());
    assert!(no_rust_below(&fixture()));
    let spec: FactorySpec = read("factory.ron");
    assert_eq!(spec.claim_level, "P0");
    assert!(spec.offline);
    assert_eq!(spec.skus.len(), 3);
    for sku in &DESKTOP_SKUS {
        assert!(spec.skus.iter().any(|id| id == sku.id));
    }
    let ratings: RatingEvidence = read("ratings.ron");
    ratings.validate().unwrap();
    let privacy: PrivacyManifest = read("privacy.ron");
    privacy.validate().unwrap();
    let rights: Vec<ReleaseRights> = read("rights.ron");
    assert!(!rights.is_empty());
    rights[0].validate().unwrap();
    let runbook: OperationsRunbook = read("runbook.ron");
    assert!(!runbook.install.is_empty());
    let achievements: Vec<AchievementDef> = read("achievements.ron");
    assert!(!achievements.is_empty());
    let approvals: Vec<Approval> = read("approvals.ron");
    assert_eq!(approvals.len(), REQUIRED_ROLES.len());
    for role in REQUIRED_ROLES {
        assert!(
            approvals
                .iter()
                .any(|row| row.role == role && matches!(row.principal, Principal::Human(_)))
        );
    }
}

#[test]
fn factory_builds_signed_offline_candidates_from_fixture() {
    let cooked = cook_doc(&tiny_doc()).unwrap();
    let content = ship_content();
    let extras = extras();
    let approvals: Vec<Approval> = read("approvals.ron");
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
    assert_eq!(release.skus.len(), 3);
    assert!(release.dashboard.signed);
    assert_eq!(release.dashboard.claim_level, "P0");
    let store = DesktopStorefront::offline();
    let dest = std::env::temp_dir().join(format!(
        "klotho-kai22-{}-{}",
        std::process::id(),
        release.skus[0].sku
    ));
    let _ = fs::remove_dir_all(&dest);
    let installed = store.install(&release.skus[0].package, &dest).unwrap();
    assert!(dest.join("game.warp").is_file());
    assert!(dest.join("ratings.ron").is_file());
    assert!(!dest.join("symbols").exists());
    store.uninstall(installed).unwrap();
    let _ = fs::remove_dir_all(&dest);
}

#[test]
fn agent_cannot_hold_signing_authority() {
    let cooked = cook_doc(&tiny_doc()).unwrap();
    let content = ship_content();
    let extras = extras();
    let mut approvals: Vec<Approval> = read("approvals.ron");
    let signing_row = approvals
        .iter_mut()
        .find(|row| row.role == ReleaseRole::Signing)
        .unwrap();
    signing_row.principal = Principal::Agent(Name::from("authoring-agent"));
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
