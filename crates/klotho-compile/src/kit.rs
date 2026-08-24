//! Closed kitbash library: tagged entries, lockfile hashes, licenses.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use klotho_core::AabbMm;
use klotho_manifest::MaterialTag;
use klotho_prove::{Hash, LicenseSpan, hash_bytes};
use serde::Deserialize;

use crate::encode::hull_for;
use crate::error::CompileError;

/// On-disk catalog (RON).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Catalog")]
struct CatalogFile {
    holder: String,
    entries: Vec<EntryFile>,
    grains: Vec<GrainFile>,
    binds: Vec<(String, String)>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Entry")]
struct EntryFile {
    tag: String,
    material: Mat,
    hull_hx: i32,
    hull_hy: i32,
    hull_hz: i32,
    mesh: MeshRecipe,
    grain: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Grain")]
struct GrainFile {
    tag: String,
    kind: GrainKind,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Deserialize)]
enum Mat {
    Organic,
    Metal,
    Stone,
    Cloth,
    Emissive,
    Water,
}

impl Mat {
    fn tag(self) -> MaterialTag {
        match self {
            Self::Organic => MaterialTag::Organic,
            Self::Metal => MaterialTag::Metal,
            Self::Stone => MaterialTag::Stone,
            Self::Cloth => MaterialTag::Cloth,
            Self::Emissive => MaterialTag::Emissive,
            Self::Water => MaterialTag::Water,
        }
    }
}

/// Integer mesh recipe. Cylinder always uses the 8-seg milli table.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Deserialize)]
pub(crate) enum MeshRecipe {
    /// Axis-aligned box, millimetres, standing on y=0.
    Box {
        /// Half-extent X.
        hx: i32,
        /// Height Y.
        hy: i32,
        /// Half-extent Z.
        hz: i32,
    },
    /// Vertical cylinder. `segs` is recorded but v1 encodes 8.
    Cylinder {
        /// Radius, millimetres.
        radius: i32,
        /// Height Y.
        hy: i32,
        /// Authoring segment count (ignored; 8-seg table is the hashed path).
        segs: u16,
    },
}

/// Integer PCM recipe.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Deserialize)]
pub(crate) enum GrainKind {
    /// Door / wood knock.
    Knock,
    /// Fire crackle.
    Crackle,
    /// Shop ambience loop.
    Bed,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Lockfile")]
struct LockFile {
    files: Vec<(String, String)>,
}

/// One tagged kitbash prop.
#[derive(Clone, Debug)]
pub struct KitEntry {
    /// Retrieval tag (`door.oak.lockable`).
    pub tag: String,
    /// Closed material.
    pub material: MaterialTag,
    /// Integer hull.
    pub hull: AabbMm,
    pub(crate) mesh: MeshRecipe,
    /// Optional grain tag.
    pub grain: Option<String>,
}

/// Reviewed, hashed, licensed kitbash.
#[derive(Clone, Debug)]
pub struct Kitbash {
    /// Rights holder recorded on every blob.
    pub holder: String,
    /// blake3 of `LICENSE`.
    pub contract_hash: Hash,
    /// Tag → entry, ordered.
    pub entries: BTreeMap<String, KitEntry>,
    pub(crate) grains: BTreeMap<String, GrainKind>,
    /// Seed locus name → tag.
    pub binds: BTreeMap<String, String>,
    /// Commissioned span used on every blob.
    pub license: LicenseSpan,
}

impl Kitbash {
    /// Load `data/kitbash` next to the workspace (via this crate's manifest dir).
    pub fn load_default() -> Result<Self, CompileError> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/kitbash");
        Self::load(&root)
    }

    /// Load a kitbash root containing `catalog.ron`, `LICENSE`, `lock.ron`.
    pub fn load(root: &Path) -> Result<Self, CompileError> {
        let lock_src = read(root, "lock.ron")?;
        let lock: LockFile =
            ron::from_str(&lock_src).map_err(|e| CompileError::Catalog(format!("lock: {e}")))?;
        if lock.files.is_empty() {
            return Err(CompileError::Catalog("empty lockfile".into()));
        }
        let mut locked = BTreeMap::new();
        for (name, hex) in lock.files {
            let bytes = read_bytes(root, &name)?;
            let actual = hash_bytes(&bytes);
            let actual_s = actual.to_string();
            if actual_s != hex {
                return Err(CompileError::LockMismatch {
                    file: name,
                    expected: hex,
                    actual: actual_s,
                });
            }
            locked.insert(name, bytes);
        }
        let license_bytes = locked
            .get("LICENSE")
            .ok_or_else(|| CompileError::MissingLockFile("LICENSE".into()))?;
        let catalog_src = locked
            .get("catalog.ron")
            .ok_or_else(|| CompileError::MissingLockFile("catalog.ron".into()))?;
        let catalog: CatalogFile = ron::from_str(
            std::str::from_utf8(catalog_src)
                .map_err(|_| CompileError::Catalog("catalog is not utf-8".into()))?,
        )
        .map_err(|e| CompileError::Catalog(e.to_string()))?;
        let contract_hash = hash_bytes(license_bytes);
        let license = LicenseSpan::commissioned(catalog.holder.clone(), contract_hash)
            .map_err(CompileError::prove)?;
        let mut entries = BTreeMap::new();
        for e in catalog.entries {
            if e.tag.is_empty() {
                return Err(CompileError::Catalog("empty tag".into()));
            }
            let hull = hull_for(e.hull_hx, e.hull_hy, e.hull_hz);
            entries.insert(
                e.tag.clone(),
                KitEntry {
                    tag: e.tag,
                    material: e.material.tag(),
                    hull,
                    mesh: e.mesh,
                    grain: e.grain,
                },
            );
        }
        let mut grains = BTreeMap::new();
        for g in catalog.grains {
            grains.insert(g.tag, g.kind);
        }
        let mut binds = BTreeMap::new();
        for (locus, tag) in catalog.binds {
            binds.insert(locus, tag);
        }
        Ok(Self {
            holder: catalog.holder,
            contract_hash,
            entries,
            grains,
            binds,
            license,
        })
    }

    /// Retrieve by tag.
    #[must_use]
    pub fn get(&self, tag: &str) -> Option<&KitEntry> {
        self.entries.get(tag)
    }

    /// Required Appendix A tags (HLD §Place and style).
    pub const HEARTH_TAGS: [&'static str; 12] = [
        "place.hearth.interior",
        "door.oak.lockable",
        "barrel.oak.portable.flammable",
        "npc.human.biped",
        "relic.hammer",
        "relic.key",
        "relic.lockpick",
        "relic.bucket",
        "relic.ingot",
        "prop.anvil",
        "prop.forge",
        "prop.stool",
    ];
}

fn read(root: &Path, name: &str) -> Result<String, CompileError> {
    let p = root.join(name);
    fs::read_to_string(&p).map_err(|e| CompileError::Io(format!("{}: {e}", p.display())))
}

fn read_bytes(root: &Path, name: &str) -> Result<Vec<u8>, CompileError> {
    let p = root.join(name);
    fs::read(&p).map_err(|_| CompileError::MissingLockFile(name.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_library_has_hearth_tags() {
        let k = Kitbash::load_default().unwrap();
        for t in Kitbash::HEARTH_TAGS {
            assert!(k.get(t).is_some(), "missing {t}");
        }
        assert!(k.license.is_exportable());
        assert_eq!(k.holder, "Klotho");
        assert_eq!(
            k.binds.get("oak_door").map(String::as_str),
            Some("door.oak.lockable")
        );
    }

    #[test]
    fn lockfile_inputs_are_hashed() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/kitbash");
        for f in ["LICENSE", "catalog.ron"] {
            let bytes = fs::read(root.join(f)).unwrap();
            assert_eq!(hash_bytes(&bytes).to_string().len(), 64, "{f}");
        }
    }

    #[test]
    fn lock_mismatch_refuses_unknown_bytes() {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/kitbash");
        let tmp = std::env::temp_dir().join(format!("klotho-kit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        for f in ["LICENSE", "catalog.ron", "lock.ron"] {
            fs::copy(src.join(f), tmp.join(f)).unwrap();
        }
        fs::write(tmp.join("catalog.ron"), b"not-the-locked-bytes\n").unwrap();
        let e = Kitbash::load(&tmp).unwrap_err();
        assert!(matches!(e, CompileError::LockMismatch { file, .. } if file == "catalog.ron"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
