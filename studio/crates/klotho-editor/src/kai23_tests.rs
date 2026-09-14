//! KAI-23: certified console SKU public boundary.

use std::fs;
use std::path::{Path, PathBuf};

use klotho_core::Hash;
use klotho_ir::from_ron;
use klotho_prove::hash_bytes;
use klotho_release::{
    AccessWorkspace, AdapterClass, Approval, CONSOLE_SKUS, CertChecklist, ClaimLevel,
    ConsoleFactoryRequest, DeviceFarm, EvidenceDraft, EvidenceRecord, GraphicsApi, PlatformTarget,
    REQUIRED_ROLES, REQUIRED_SUITES, ReleaseSigningKey, ReplayEvidence, achieved_level,
    console_candidate_build, forbid_overclaim, import_checklist, import_redacted, public_farm_gate,
    public_workspace_layout, submit_console_sku,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentitySpec {
    skus: Vec<String>,
    sdk_revision: String,
    claim_level: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedactedSpec {
    level: ClaimLevel,
    target: PlatformTarget,
    date: String,
    package_hash: Hash,
    owner: String,
    expiry: String,
    passed: bool,
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/console-sku")
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
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

fn replay<'a>(bytes: &'a [u8]) -> ReplayEvidence<'a> {
    ReplayEvidence {
        label: "kernel",
        canon_hash: Hash::from_bytes([1; 32]),
        epoch: klotho_core::Epoch(1),
        expected_trace_prefix_hash: Hash::from_bytes([2; 32]),
        observed_trace_prefix_hash: Hash::from_bytes([2; 32]),
        replay: bytes,
    }
}

#[test]
fn fixture_is_data_only_and_complete() {
    no_placeholders(&fixture());
    assert!(no_rust_below(&fixture()));
    let spec: IdentitySpec = read("identities.ron");
    assert_eq!(spec.claim_level, "P0");
    assert_eq!(spec.sdk_revision, "public-mock");
    assert_eq!(spec.skus.len(), CONSOLE_SKUS.len());
    for sku in &CONSOLE_SKUS {
        assert!(spec.skus.iter().any(|id| id == sku.id));
        assert!(sku.target.is_console());
        assert!(matches!(
            sku.graphics,
            GraphicsApi::GdkD3d12 | GraphicsApi::ProsperoGnmAgc
        ));
    }
    let checklist: CertChecklist = read("checklist.ron");
    import_checklist(&checklist).unwrap();
    assert_eq!(checklist.items.len(), REQUIRED_SUITES.len());
    let farm: DeviceFarm = read("farm.ron");
    farm.validate().unwrap();
    assert_eq!(farm.devices.len(), 2);
    assert!(
        farm.devices
            .iter()
            .all(|d| d.adapter_class == AdapterClass::PublicMock)
    );
    let approvals: Vec<Approval> = read("approvals.ron");
    assert_eq!(approvals.len(), REQUIRED_ROLES.len());
}

#[test]
fn public_factory_and_farm_stay_p0() {
    let spec: IdentitySpec = read("identities.ron");
    let approvals: Vec<Approval> = read("approvals.ron");
    let farm: DeviceFarm = read("farm.ron");
    let ws = AccessWorkspace::open(
        std::env::temp_dir().join(format!("klotho-kai23-{}", std::process::id())),
    );
    let signing = ReleaseSigningKey::from_bytes([11; 32]);
    let release = console_candidate_build(ConsoleFactoryRequest {
        warp: b"warp-bytes",
        sku_ids: &spec.skus,
        sdk_revision: &spec.sdk_revision,
        approvals: &approvals,
        signing: &signing,
        p1: None,
        holder: None,
        workspace: &ws,
        replay: replay(b"replay"),
    })
    .unwrap();
    assert_eq!(release.dashboard.claim_level, ClaimLevel::P0);
    assert_eq!(release.dashboard.public_statement, "P0 public boundary");
    assert!(!release.dashboard.claim_level.may_claim_certified());
    for sku in &release.skus {
        let submission = submit_console_sku(sku, None).unwrap();
        assert_eq!(submission.claim_level, ClaimLevel::P0);
        forbid_overclaim(&submission.public_statement, submission.claim_level).unwrap();
    }
    let claim = public_farm_gate(&farm, replay(b"replay"), Hash::from_bytes([3; 32])).unwrap();
    assert_eq!(claim, ClaimLevel::P0);
}

#[test]
fn redacted_evidence_is_not_reproduction() {
    let spec: RedactedSpec = read("evidence/redacted.ron");
    let record = EvidenceRecord::compose(
        EvidenceDraft {
            level: spec.level,
            target: spec.target,
            date: spec.date,
            package_hash: spec.package_hash,
            owner: spec.owner,
            expiry: spec.expiry,
            passed: spec.passed,
        },
        &[],
    )
    .unwrap();
    let imported = import_redacted(record.clone()).unwrap();
    assert_eq!(imported.level, ClaimLevel::P0);
    let ws = AccessWorkspace::gdk(repo());
    let err = ws.resolve(&imported).unwrap_err();
    assert!(err.to_string().contains("not public reproduction"));
    assert_eq!(
        achieved_level(AdapterClass::PublicMock, None, None, &ws),
        ClaimLevel::P0
    );
    assert_eq!(hash_bytes(&[]), imported.redacted_commitment);
}

#[test]
fn overclaim_and_certified_language_fail_closed() {
    let err = forbid_overclaim("certified console SKU", ClaimLevel::P0).unwrap_err();
    assert!(err.to_string().contains("forbidden"));
    let err = forbid_overclaim("console-ready", ClaimLevel::P1).unwrap_err();
    assert!(err.to_string().contains("forbidden"));
}

#[test]
fn public_workspace_layout_exists() {
    public_workspace_layout(&repo()).unwrap();
}
