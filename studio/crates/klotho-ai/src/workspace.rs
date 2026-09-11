//! Content-addressed snapshot workspace. Never writes the live project tree.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use klotho_author::{AnchoredSeedFact, bundle_content_hash, dependents_of};
use klotho_core::Hash;
use klotho_ir::{
    AnchorId, IntentModule, IntentProject, Name, NameAlias, ObjectAnchor, ProjectBundle, SeedFact,
    Tombstone, from_ron, to_ron,
};
use klotho_prove::hash_bytes;

use crate::error::AiError;
use crate::ids::{AssetRequestId, Cell, FieldId, ReferenceId};

const SNAP_DOMAIN: &[u8] = b"klotho-ai-snap-v1";

/// Isolated project plus sidecar collections that are not yet IR fields.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringSnapshot {
    /// Project manifest.
    pub project: IntentProject,
    /// Module bodies.
    pub modules: Vec<IntentModule>,
    /// Asset candidates keyed by locus.
    pub assets: BTreeMap<AnchorId, BTreeSet<AssetRequestId>>,
    /// Reference edges keyed by target.
    pub references: BTreeMap<AnchorId, BTreeSet<ReferenceId>>,
}

impl AuthoringSnapshot {
    /// Wrap a loaded bundle.
    #[must_use]
    pub fn from_bundle(bundle: ProjectBundle) -> Self {
        Self {
            project: bundle.project,
            modules: bundle.modules,
            assets: BTreeMap::new(),
            references: BTreeMap::new(),
        }
    }

    /// Project plus modules.
    #[must_use]
    pub fn bundle(&self) -> ProjectBundle {
        ProjectBundle {
            project: self.project.clone(),
            modules: self.modules.clone(),
        }
    }

    /// Content hash of this snapshot.
    pub fn content_hash(&self) -> Result<Hash, AiError> {
        let text = to_ron(self).map_err(|e| AiError::Ser(e.to_string()))?;
        let mut buf = Vec::with_capacity(SNAP_DOMAIN.len() + 4 + text.len());
        buf.extend_from_slice(SNAP_DOMAIN);
        buf.extend_from_slice(&(text.len() as u32).to_le_bytes());
        buf.extend_from_slice(text.as_bytes());
        Ok(hash_bytes(&buf))
    }

    /// Bundle lock hash (modules only).
    pub fn project_hash(&self) -> Result<Hash, AiError> {
        Ok(bundle_content_hash(&self.bundle())?)
    }

    /// Live object, if any.
    #[must_use]
    pub fn lookup(&self, id: AnchorId) -> Option<&ObjectAnchor> {
        self.modules
            .iter()
            .find_map(|m| m.object_anchors.iter().find(|o| o.anchor == id))
    }

    /// Module, if any.
    #[must_use]
    pub fn module(&self, id: AnchorId) -> Option<&IntentModule> {
        self.modules.iter().find(|m| m.anchor == id)
    }

    /// Module that owns `id`.
    #[must_use]
    pub fn owning_module(&self, id: AnchorId) -> Option<AnchorId> {
        self.modules
            .iter()
            .find(|m| m.anchor == id || m.object_anchors.iter().any(|o| o.anchor == id))
            .map(|m| m.anchor)
    }

    /// Dependent anchors of `target`.
    #[must_use]
    pub fn dependents(&self, target: AnchorId) -> Vec<AnchorId> {
        dependents_of(&self.bundle(), target)
    }

    /// True when `id` is a live object or module.
    #[must_use]
    pub fn exists(&self, id: AnchorId) -> bool {
        self.modules.iter().any(|m| m.anchor == id)
            || self
                .modules
                .iter()
                .any(|m| m.object_anchors.iter().any(|o| o.anchor == id))
    }

    /// True when `id` is tombstoned.
    #[must_use]
    pub fn tombstoned(&self, id: AnchorId) -> bool {
        self.modules
            .iter()
            .any(|m| m.tombstones.iter().any(|t| t.anchor == id))
    }

    /// True when `name` is live, aliased, or tombstoned in `module`.
    #[must_use]
    pub fn name_taken(&self, module: AnchorId, name: &Name) -> bool {
        let Some(m) = self.modules.iter().find(|m| m.anchor == module) else {
            return true;
        };
        m.object_anchors.iter().any(|o| o.name == *name)
            || m.aliases.iter().any(|a| a.name == *name)
            || m.tombstones.iter().any(|t| t.name == *name)
    }

    /// Hash of a cell's current value, or the absent sentinel.
    #[must_use]
    pub fn cell_hash(&self, cell: Cell) -> Hash {
        hash_bytes(&cell_bytes(self, cell))
    }

    /// Occupied identities (modules, objects, tombstones).
    #[must_use]
    pub fn occupied(&self) -> BTreeSet<AnchorId> {
        let mut ids = BTreeSet::new();
        for module in &self.modules {
            ids.insert(module.anchor);
            for object in &module.object_anchors {
                ids.insert(object.anchor);
            }
            for tomb in &module.tombstones {
                ids.insert(tomb.anchor);
            }
        }
        ids
    }

    /// Tombstone row, if any.
    #[must_use]
    pub fn tombstone(&self, id: AnchorId) -> Option<&Tombstone> {
        self.modules
            .iter()
            .find_map(|m| m.tombstones.iter().find(|t| t.anchor == id))
    }

    /// Alias rows targeting `id`.
    #[must_use]
    pub fn aliases(&self, id: AnchorId) -> Vec<&NameAlias> {
        self.modules
            .iter()
            .flat_map(|m| m.aliases.iter())
            .filter(|a| a.target == id)
            .collect()
    }

    /// Qty facts on `id`.
    #[must_use]
    pub fn qty(&self, id: AnchorId, res: &Name) -> Option<i32> {
        let name = self.lookup(id)?.name.clone();
        self.modules.iter().find_map(|m| {
            m.body.seed.iter().find_map(|f| match f {
                SeedFact::Qty { of, res: r, value } if *of == name && r == res => Some(*value),
                _ => None,
            })
        })
    }

    /// Rel facts involving `id`.
    #[must_use]
    pub fn rels(&self, id: AnchorId) -> Vec<AnchoredSeedFact> {
        let Some(object) = self.lookup(id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for module in &self.modules {
            for fact in &module.body.seed {
                if let SeedFact::Rel { a, rel, b } = fact {
                    if a == &object.name || b == &object.name {
                        let aa = name_to_anchor(module, a);
                        let bb = name_to_anchor(module, b);
                        if let (Some(aa), Some(bb)) = (aa, bb) {
                            out.push(AnchoredSeedFact::Rel {
                                a: aa,
                                rel: *rel,
                                b: bb,
                            });
                        }
                    }
                }
            }
        }
        out
    }
}

fn name_to_anchor(module: &IntentModule, name: &Name) -> Option<AnchorId> {
    module
        .object_anchors
        .iter()
        .find(|o| o.name == *name)
        .map(|o| o.anchor)
}

fn cell_bytes(snap: &AuthoringSnapshot, cell: Cell) -> Vec<u8> {
    let mut buf = Vec::from(&cell.anchor.0[..]);
    buf.push(cell.field as u8);
    match cell.field {
        FieldId::Name => {
            if let Some(o) = snap.lookup(cell.anchor) {
                buf.extend_from_slice(o.name.as_str().as_bytes());
            }
        }
        FieldId::Qty => {
            if let Some(o) = snap.lookup(cell.anchor) {
                for module in &snap.modules {
                    for fact in &module.body.seed {
                        if let SeedFact::Qty { of, res, value } = fact {
                            if of == &o.name {
                                buf.extend_from_slice(res.as_str().as_bytes());
                                buf.extend_from_slice(&value.to_le_bytes());
                            }
                        }
                    }
                }
            }
        }
        FieldId::Pose => {
            if let Some(o) = snap.lookup(cell.anchor) {
                for module in &snap.modules {
                    for fact in &module.body.seed {
                        if let SeedFact::Pose { of, pose } = fact {
                            if of == &o.name {
                                buf.extend_from_slice(&pose.x.0.to_le_bytes());
                                buf.extend_from_slice(&pose.y.0.to_le_bytes());
                                buf.extend_from_slice(&pose.z.0.to_le_bytes());
                            }
                        }
                    }
                }
            }
        }
        FieldId::CanonDiff => {
            if let Some(m) = snap.modules.iter().find(|m| m.anchor == cell.anchor) {
                if let Ok(text) = to_ron(&m.body.canon_diffs) {
                    buf.extend_from_slice(text.as_bytes());
                }
            }
        }
        FieldId::Tombstone => {
            if snap.tombstoned(cell.anchor) {
                buf.push(1);
            }
        }
        FieldId::AssetBinding => {
            if let Some(set) = snap.assets.get(&cell.anchor) {
                for id in set {
                    buf.extend_from_slice(&id.0);
                }
            }
        }
        FieldId::Reference => {
            if let Some(set) = snap.references.get(&cell.anchor) {
                for id in set {
                    buf.extend_from_slice(&id.0);
                }
            }
        }
        _ => {
            if snap.exists(cell.anchor) {
                buf.push(1);
            }
        }
    }
    buf
}

/// CAS directory under `root`.
#[derive(Clone, Debug)]
pub struct ContentWorkspace {
    root: PathBuf,
}

impl ContentWorkspace {
    /// Create `root` if needed.
    pub fn open(root: &Path) -> Result<Self, AiError> {
        fs::create_dir_all(root).map_err(|e| AiError::Io(e.to_string()))?;
        fs::create_dir_all(root.join("cas")).map_err(|e| AiError::Io(e.to_string()))?;
        fs::create_dir_all(root.join("tx")).map_err(|e| AiError::Io(e.to_string()))?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Workspace root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Persist `snap` under its content hash. Existing hashes are left untouched.
    pub fn put(&self, snap: &AuthoringSnapshot) -> Result<Hash, AiError> {
        let hash = snap.content_hash()?;
        let dest = self.cas_dir(hash);
        if dest.exists() {
            return Ok(hash);
        }
        let tmp = self.root.join("cas").join(format!("{hash}.tmp"));
        if tmp.exists() {
            fs::remove_dir_all(&tmp).map_err(|e| AiError::Io(e.to_string()))?;
        }
        fs::create_dir_all(&tmp).map_err(|e| AiError::Io(e.to_string()))?;
        let text = to_ron(snap).map_err(|e| AiError::Ser(e.to_string()))?;
        fs::write(tmp.join("snapshot.ron"), text).map_err(|e| AiError::Io(e.to_string()))?;
        match fs::rename(&tmp, &dest) {
            Ok(()) => Ok(hash),
            Err(_) if dest.exists() => {
                let _ = fs::remove_dir_all(&tmp);
                Ok(hash)
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&tmp);
                Err(AiError::Io(e.to_string()))
            }
        }
    }

    /// Load a snapshot by hash.
    pub fn get(&self, hash: Hash) -> Result<AuthoringSnapshot, AiError> {
        let path = self.cas_dir(hash).join("snapshot.ron");
        let text = fs::read_to_string(&path)
            .map_err(|e| AiError::Io(format!("{}: {e}", path.display())))?;
        from_ron(&text).map_err(|e| AiError::Ser(e.to_string()))
    }

    /// Delete only `tx/{id}`; CAS blobs stay (content-addressed).
    pub fn delete_tx(&self, name: &str) -> Result<(), AiError> {
        let path = self.root.join("tx").join(name);
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|e| AiError::Io(e.to_string()))?;
        }
        Ok(())
    }

    fn cas_dir(&self, hash: Hash) -> PathBuf {
        self.root.join("cas").join(hash.to_string())
    }
}
