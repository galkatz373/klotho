//! One-Place desktop package and installer lane (KAI-11).
//!
//! The ship graph is engine-only: a cooked `.warp`, keyed locales, credits,
//! accessibility, HUD spec, and a recorded critical-path play file. Studio,
//! model, and title-Rust paths are rejected by [`check_ship_allowlist`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cook::Cooked;
use crate::error::{CompileError, check_ship_allowlist};
use crate::warp::pack_warp;
use crate::{WholeTitleCook, encode_whole_title_plan};
use klotho_prove::hash_bytes;

/// Locale identifiers shipped with Mini-Tapestry.
pub const REQUIRED_LOCALES: [&str; 3] = ["en", "ja", "es"];

/// Keys every shipping locale table must contain.
pub const REQUIRED_LOCALE_KEYS: [&str; 8] = [
    "hud.stamina",
    "hud.prompt_use",
    "hud.prompt_attack",
    "menu.accessibility",
    "save.checkpoint",
    "credits.title",
    "credits.body",
    "play.complete",
];

/// First-title desktop SKUs. Console SKUs remain KAI-23.
pub const DESKTOP_SKUS: [DesktopSku; 3] = [
    DesktopSku {
        id: "win-d3d12-high",
        os: "Windows",
        graphics_api: "D3D12",
        resolution: "1920x1080",
        quality: "high",
        refresh_hz: 60,
    },
    DesktopSku {
        id: "linux-vulkan-high",
        os: "Linux",
        graphics_api: "Vulkan",
        resolution: "1920x1080",
        quality: "high",
        refresh_hz: 60,
    },
    DesktopSku {
        id: "mac-metal-high",
        os: "macOS",
        graphics_api: "Metal",
        resolution: "1920x1080",
        quality: "high",
        refresh_hz: 60,
    },
];

/// Desktop SKU identity copied into the package manifest.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSku {
    /// Stable SKU id (`win-d3d12-high`).
    pub id: &'static str,
    /// Family name (`Windows`, `Linux`, `macOS`).
    pub os: &'static str,
    /// Presentation API.
    pub graphics_api: &'static str,
    /// Locked resolution.
    pub resolution: &'static str,
    /// Quality tier.
    pub quality: &'static str,
    /// Refresh.
    pub refresh_hz: u16,
}

/// Keyed locale table. No executable text.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleTable {
    /// `en` / `ja` / `es`.
    pub id: String,
    /// Stable keys to shipping strings.
    pub strings: BTreeMap<String, String>,
}

/// One credits row.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditEntry {
    /// Person or house.
    pub name: String,
    /// Role.
    pub role: String,
}

/// End-roll credits.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditsRoll {
    /// Title card.
    pub title: String,
    /// Named contributors.
    pub entries: Vec<CreditEntry>,
}

/// First-title accessibility settings shipped with the increment.
pub type AccessibilitySettings = klotho_ir::A11yProfile;

/// Production HUD slots. Presentation only.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HudSpec {
    /// Required slots (`status`, `prompt`, `notice`).
    pub slots: Vec<String>,
    /// Palette name (`production`).
    pub palette: String,
}

/// Authoring-side payload packed next to the warp.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShipContent {
    /// Three shipping locales.
    pub locales: Vec<LocaleTable>,
    /// Credits roll.
    pub credits: CreditsRoll,
    /// Accessibility settings.
    pub accessibility: AccessibilitySettings,
    /// HUD descriptor.
    pub hud: HudSpec,
    /// Recorded human critical-path play bytes (journey RON).
    pub play_recording: String,
    /// Third-party / kitbash notice.
    pub notice: String,
}

/// Content-addressed desktop package. Paths are relative and allowlisted.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DesktopPackage {
    /// SKU packed.
    pub sku: String,
    /// Relative path → file bytes.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Record written by install. Uninstall only removes these paths.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    /// SKU id.
    pub sku: String,
    /// Relative path → blake3 hex.
    pub files: BTreeMap<String, String>,
}

const INSTALL_RECORD: &str = ".klotho-install.ron";
const PLACEHOLDERS: [&str; 6] = ["todo", "tbd", "fixme", "placeholder", "lorem ipsum", "xxxx"];

/// Pack `cooked` plus ship content for one desktop SKU.
pub fn pack_desktop(
    cooked: &Cooked,
    sku: &DesktopSku,
    content: &ShipContent,
) -> Result<DesktopPackage, CompileError> {
    validate_content(content)?;
    let mut files = BTreeMap::new();
    files.insert("game.warp".into(), pack_warp(cooked)?);
    files.insert(
        "sku.ron".into(),
        to_bytes(&SkuWire {
            id: sku.id,
            os: sku.os,
            graphics_api: sku.graphics_api,
            resolution: sku.resolution,
            quality: sku.quality,
            refresh_hz: sku.refresh_hz,
            claim_level: "P0",
        })?,
    );
    for locale in &content.locales {
        let path = format!("locales/{}.ron", locale.id);
        files.insert(path, to_bytes(locale)?);
    }
    files.insert("credits.ron".into(), to_bytes(&content.credits)?);
    files.insert(
        "accessibility.ron".into(),
        to_bytes(&content.accessibility)?,
    );
    files.insert("hud.ron".into(), to_bytes(&content.hud)?);
    files.insert(
        "play/critical-path.ron".into(),
        content.play_recording.as_bytes().to_vec(),
    );
    files.insert("NOTICE".into(), content.notice.as_bytes().to_vec());
    for (path, bytes) in &files {
        check_ship_allowlist(path)?;
        if path.ends_with(".rs") {
            return Err(CompileError::PackageAllowlist(path.clone()));
        }
        if let Ok(text) = std::str::from_utf8(bytes) {
            reject_placeholders(path, text)?;
        }
    }
    Ok(DesktopPackage {
        sku: sku.id.to_owned(),
        files,
    })
}

/// Pack the proven optimized cook plus its model-free runtime layout.
///
/// Debug source maps, pass diagnostics, transcripts, and stripped auxiliary
/// inputs are intentionally not serialized into the game package.
pub fn pack_optimized_desktop(
    cooked: &WholeTitleCook,
    sku: &DesktopSku,
    content: &ShipContent,
) -> Result<DesktopPackage, CompileError> {
    if !cooked
        .plan
        .skus
        .iter()
        .any(|planned| planned.sku.as_str() == sku.id)
    {
        return Err(CompileError::Optimization(format!(
            "SKU {} was not planned",
            sku.id
        )));
    }
    let mut package = pack_desktop(&cooked.optimized, sku, content)?;
    package.files.insert(
        "runtime/whole-title.kopt".into(),
        encode_whole_title_plan(&cooked.plan)?,
    );
    for (path, bytes) in &cooked.plan.runtime_files {
        check_ship_allowlist(path)?;
        package.files.insert(path.clone(), bytes.clone());
    }
    Ok(package)
}

/// Copy package files into `dest` and write an install record.
pub fn install_package(
    package: &DesktopPackage,
    dest: &Path,
) -> Result<InstallRecord, CompileError> {
    fs::create_dir_all(dest).map_err(|e| CompileError::Io(e.to_string()))?;
    let mut record = InstallRecord {
        sku: package.sku.clone(),
        files: BTreeMap::new(),
    };
    for (rel, bytes) in &package.files {
        check_ship_allowlist(rel)?;
        write_rel(dest, rel, bytes)?;
        record
            .files
            .insert(rel.clone(), hash_bytes(bytes).to_string());
    }
    let rec_bytes = to_bytes(&record)?;
    write_rel(dest, INSTALL_RECORD, &rec_bytes)?;
    Ok(record)
}

/// Restore missing or drifted files from `package`.
pub fn repair_package(
    package: &DesktopPackage,
    dest: &Path,
) -> Result<InstallRecord, CompileError> {
    install_package(package, dest)
}

/// Remove installed files listed in `record`.
pub fn uninstall_package(dest: &Path, record: &InstallRecord) -> Result<(), CompileError> {
    for rel in record.files.keys() {
        let path = dest.join(rel);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| CompileError::Io(e.to_string()))?;
        }
    }
    let marker = dest.join(INSTALL_RECORD);
    if marker.exists() {
        fs::remove_file(&marker).map_err(|e| CompileError::Io(e.to_string()))?;
    }
    remove_empty_dirs(dest);
    Ok(())
}

#[derive(Serialize)]
struct SkuWire {
    id: &'static str,
    os: &'static str,
    graphics_api: &'static str,
    resolution: &'static str,
    quality: &'static str,
    refresh_hz: u16,
    claim_level: &'static str,
}

fn validate_content(content: &ShipContent) -> Result<(), CompileError> {
    let ids: BTreeSet<_> = content.locales.iter().map(|l| l.id.as_str()).collect();
    if ids.len() != REQUIRED_LOCALES.len() || REQUIRED_LOCALES.iter().any(|id| !ids.contains(id)) {
        return Err(CompileError::Catalog(
            "ship content must include en, ja, and es".into(),
        ));
    }
    let expected: BTreeSet<_> = REQUIRED_LOCALE_KEYS.iter().copied().collect();
    for locale in &content.locales {
        let keys: BTreeSet<_> = locale.strings.keys().map(String::as_str).collect();
        if keys != expected {
            return Err(CompileError::Catalog(format!(
                "locale {} is missing required keys",
                locale.id
            )));
        }
        for (key, value) in &locale.strings {
            if value.trim().is_empty() {
                return Err(CompileError::Catalog(format!(
                    "locale {} has empty {key}",
                    locale.id
                )));
            }
            reject_placeholders(&format!("locales/{}.ron", locale.id), value)?;
        }
    }
    if content.credits.entries.is_empty() || content.credits.title.trim().is_empty() {
        return Err(CompileError::Catalog("credits roll is empty".into()));
    }
    if !content.accessibility.remap
        || !content.accessibility.subtitles
        || content.accessibility.validate().is_err()
    {
        return Err(CompileError::Catalog(
            "accessibility settings are incomplete".into(),
        ));
    }
    let slots: BTreeSet<_> = content.hud.slots.iter().map(String::as_str).collect();
    if !slots.contains("status") || !slots.contains("prompt") || !slots.contains("notice") {
        return Err(CompileError::Catalog(
            "HUD is missing a production slot".into(),
        ));
    }
    if content.hud.palette != "production" {
        return Err(CompileError::Catalog(
            "HUD palette must be production".into(),
        ));
    }
    if content.play_recording.trim().is_empty() || content.notice.trim().is_empty() {
        return Err(CompileError::Catalog(
            "play recording or NOTICE missing".into(),
        ));
    }
    reject_placeholders("NOTICE", &content.notice)?;
    reject_placeholders("credits", &content.credits.title)?;
    Ok(())
}

fn reject_placeholders(path: &str, text: &str) -> Result<(), CompileError> {
    let lower = text.to_ascii_lowercase();
    for needle in PLACEHOLDERS {
        if lower.contains(needle) {
            return Err(CompileError::Catalog(format!(
                "placeholder `{needle}` in {path}"
            )));
        }
    }
    Ok(())
}

fn to_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, CompileError> {
    let text = ron::ser::to_string(value).map_err(|e| CompileError::Catalog(e.to_string()))?;
    Ok(text.into_bytes())
}

fn write_rel(dest: &Path, rel: &str, bytes: &[u8]) -> Result<(), CompileError> {
    check_ship_allowlist(rel)?;
    let path = dest.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CompileError::Io(e.to_string()))?;
    }
    fs::write(&path, bytes).map_err(|e| CompileError::Io(e.to_string()))
}

fn remove_empty_dirs(root: &Path) {
    let _ = fs::remove_dir_all(root.join("locales"));
    let _ = fs::remove_dir_all(root.join("play"));
    if root
        .read_dir()
        .map(|mut d| d.next().is_none())
        .unwrap_or(false)
    {
        let _ = fs::remove_dir(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::cook_doc;

    fn content() -> ShipContent {
        let strings: BTreeMap<String, String> = REQUIRED_LOCALE_KEYS
            .iter()
            .map(|k| ((*k).to_owned(), format!("text-{k}")))
            .collect();
        ShipContent {
            locales: REQUIRED_LOCALES
                .iter()
                .map(|id| LocaleTable {
                    id: (*id).to_owned(),
                    strings: strings.clone(),
                })
                .collect(),
            credits: CreditsRoll {
                title: "Mini-Tapestry".into(),
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

    fn packed() -> DesktopPackage {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
        pack_desktop(&cooked, &DESKTOP_SKUS[0], &content()).unwrap()
    }

    #[test]
    fn three_desktop_skus_pack_without_studio_bytes() {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
        let content = content();
        for sku in &DESKTOP_SKUS {
            let package = pack_desktop(&cooked, sku, &content).unwrap();
            assert_eq!(package.sku, sku.id);
            assert!(package.files.contains_key("game.warp"));
            assert!(package.files.contains_key("locales/en.ron"));
            assert!(package.files.contains_key("locales/ja.ron"));
            assert!(package.files.contains_key("locales/es.ron"));
            assert!(package.files.contains_key("credits.ron"));
            assert!(package.files.contains_key("accessibility.ron"));
            assert!(package.files.contains_key("hud.ron"));
            assert!(package.files.contains_key("play/critical-path.ron"));
            for path in package.files.keys() {
                check_ship_allowlist(path).unwrap();
                assert!(!path.contains("studio"));
                assert!(!path.contains("models"));
                assert!(!path.ends_with(".rs"));
                assert!(!path.ends_with(".gguf"));
            }
        }
    }

    #[test]
    fn install_repair_uninstall_round_trips() {
        let package = packed();
        let dest =
            std::env::temp_dir().join(format!("klotho-pkg-{}-{}", std::process::id(), package.sku));
        let _ = fs::remove_dir_all(&dest);
        let record = install_package(&package, &dest).unwrap();
        assert!(dest.join("game.warp").is_file());
        fs::write(dest.join("game.warp"), b"corrupt").unwrap();
        repair_package(&package, &dest).unwrap();
        let repaired = fs::read(dest.join("game.warp")).unwrap();
        assert_eq!(repaired, package.files["game.warp"]);
        uninstall_package(&dest, &record).unwrap();
        assert!(!dest.join("game.warp").exists());
        assert!(!dest.join(INSTALL_RECORD).exists());
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn placeholder_locale_fails_closed() {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
        let mut content = content();
        content.locales[0]
            .strings
            .insert("hud.stamina".into(), "TODO bar".into());
        let err = pack_desktop(&cooked, &DESKTOP_SKUS[1], &content).unwrap_err();
        assert!(err.to_string().contains("placeholder"));
    }

    #[test]
    fn two_locales_fail_closed() {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
        let mut content = content();
        content.locales.pop();
        let err = pack_desktop(&cooked, &DESKTOP_SKUS[2], &content).unwrap_err();
        assert!(err.to_string().contains("en, ja, and es"));
    }

    #[test]
    fn optimized_package_has_plan_but_no_debug_or_model_inputs() {
        let mut request = crate::WholeTitleRequest::default();
        request.skus.push(crate::SkuPlanInput {
            sku: klotho_ir::Name::from(DESKTOP_SKUS[0].id),
            tier: klotho_ir::QualityTier::High,
            used_permutations: Vec::new(),
        });
        request
            .auxiliary_files
            .insert("models/author.gguf".into(), vec![1, 2, 3]);
        let whole = crate::cook_whole_title(&hearth_slice::hearth_doc(), &request, |_| {
            Ok(vec![crate::TraceRun {
                case: klotho_ir::Name::from("hearth-package"),
                deltas: vec![klotho_trace::TraceDelta::empty(klotho_core::Tick(0))],
                terminal_prefix: klotho_core::Hash::ZERO,
            }])
        })
        .unwrap();
        let package = pack_optimized_desktop(&whole, &DESKTOP_SKUS[0], &content()).unwrap();
        assert!(package.files.contains_key("runtime/whole-title.kopt"));
        assert!(package.files.keys().all(|path| !path.contains("models")));
        assert!(
            package
                .files
                .keys()
                .all(|path| !path.contains("source-map"))
        );
        let unpacked = crate::unpack_warp(&package.files["game.warp"]).unwrap();
        assert!(unpacked.optimized);
        assert_eq!(
            unpacked.canon.preds.len(),
            whole.optimized.canon.preds.len()
        );
    }
}
