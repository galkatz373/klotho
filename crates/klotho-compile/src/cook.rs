//! IntentDoc → Canon + CAS. Retrieval only; missing tag is a cook error.

use std::collections::BTreeMap;

use klotho_canon::{Canon, cook as cook_canon};
use klotho_core::Hash;
use klotho_ir::{IntentDoc, Name, SeedFact, to_ron};
use klotho_manifest::MaterialTag;
use klotho_prove::{
    Activity, Agent, ArtifactKind, Cas, ProvenanceDag, ProvenanceKind, blob_id_of, hash_bytes,
};

use crate::encode::{encode_grain, encode_hull, encode_mesh, encode_rite};
use crate::error::CompileError;
use crate::header::{validate_grain, validate_hull, validate_mesh, validate_rite};
use crate::kit::Kitbash;

/// Mixed into every cook hash. Bump invalidates CAS keys.
pub const COMPILER_VERSION: u32 = 1;

/// One seed locus bound to a kitbash tag.
#[derive(Clone, Debug)]
pub struct Binding {
    /// Seed name (`oak_door`).
    pub locus: Name,
    /// Kitbash tag (`door.oak.lockable`).
    pub tag: Name,
    /// Hull blob.
    pub hull: klotho_core::BlobId,
    /// Clustered-mesh blob.
    pub mesh: klotho_core::BlobId,
    /// Closed material.
    pub material: MaterialTag,
}

/// Cooked warp contents (`.warp` packing is PR 19).
#[derive(Debug)]
pub struct Cooked {
    /// Packed Canon.
    pub canon: Canon,
    /// Digest of the cook inputs (not a Trace prefix).
    pub cook_hash: Hash,
    /// Same value as [`Self::cook_hash`] — World takes a canon hash at boot.
    pub canon_hash: Hash,
    /// Content-addressed artifacts.
    pub cas: Cas,
    /// Provenance. Exportable (no `Unknown`).
    pub dag: ProvenanceDag,
    /// Seed locus → kitbash artifacts.
    pub bindings: Vec<Binding>,
    /// Grain tag → blob.
    pub grains: BTreeMap<String, klotho_core::BlobId>,
}

/// Cook `doc` against the workspace kitbash.
pub fn cook_doc(doc: &IntentDoc) -> Result<Cooked, CompileError> {
    cook_with(doc, &Kitbash::load_default()?)
}

/// Cook `doc` against an already-loaded library.
pub fn cook_with(doc: &IntentDoc, kit: &Kitbash) -> Result<Cooked, CompileError> {
    for t in &doc.style.kitbash_tags {
        if kit.get(t.as_str()).is_none() {
            return Err(CompileError::MissingTag(t.as_str().to_string()));
        }
    }
    let canon = cook_canon(doc).map_err(CompileError::canon)?;

    let mut cas = Cas::new();
    let mut dag = ProvenanceDag::new();
    let license = kit.license.clone();

    let kit_agent = dag
        .insert(
            ProvenanceKind::Agent {
                agent: Agent::Kitbash,
            },
            license.clone(),
            &[],
        )
        .map_err(CompileError::prove)?;
    let comp_agent = dag
        .insert(
            ProvenanceKind::Agent {
                agent: Agent::Compiler {
                    version: COMPILER_VERSION,
                },
            },
            license.clone(),
            &[],
        )
        .map_err(CompileError::prove)?;
    let intent_node = dag
        .insert(
            ProvenanceKind::Intent {
                doc_hash: doc.provenance.0,
            },
            license.clone(),
            &[],
        )
        .map_err(CompileError::prove)?;
    let cook_act = dag
        .insert(
            ProvenanceKind::Activity {
                activity: Activity::Cook,
            },
            license.clone(),
            &[kit_agent, comp_agent, intent_node],
        )
        .map_err(CompileError::prove)?;

    let mut mesh_ids = BTreeMap::new();
    let mut hull_ids = BTreeMap::new();
    let mut grain_ids = BTreeMap::new();
    let mut kit_blobs = Vec::new();

    for (tag, entry) in &kit.entries {
        let mesh = encode_mesh(&entry.mesh)?;
        validate_mesh(&mesh)?;
        let hull = encode_hull(entry.hull);
        validate_hull(&hull)?;
        let mid = cas.put(&mesh).map_err(CompileError::prove)?;
        let hid = cas.put(&hull).map_err(CompileError::prove)?;
        put_artifact(
            &mut dag,
            mid,
            ArtifactKind::ClusteredMesh,
            license.clone(),
            &[cook_act, kit_agent],
        )?;
        put_artifact(
            &mut dag,
            hid,
            ArtifactKind::Hull,
            license.clone(),
            &[cook_act, kit_agent],
        )?;
        mesh_ids.insert(tag.clone(), mid);
        hull_ids.insert(tag.clone(), hid);
        kit_blobs.push(mid);
        kit_blobs.push(hid);
        if let Some(g) = &entry.grain {
            if !kit.grains.contains_key(g) {
                return Err(CompileError::MissingTag(g.clone()));
            }
        }
    }
    for (tag, kind) in &kit.grains {
        let bytes = encode_grain(*kind);
        validate_grain(&bytes)?;
        let id = cas.put(&bytes).map_err(CompileError::prove)?;
        put_artifact(
            &mut dag,
            id,
            ArtifactKind::Grain,
            license.clone(),
            &[cook_act, kit_agent],
        )?;
        grain_ids.insert(tag.clone(), id);
        kit_blobs.push(id);
    }

    for rite in &canon.rites {
        let bytes = encode_rite(&rite.chunk)?;
        validate_rite(&bytes)?;
        let id = cas.put(&bytes).map_err(CompileError::prove)?;
        put_artifact(
            &mut dag,
            id,
            ArtifactKind::RiteChunk,
            license.clone(),
            &[cook_act, comp_agent],
        )?;
    }

    dag.exportable().map_err(CompileError::prove)?;
    dag.blobs_present(&cas).map_err(CompileError::prove)?;

    let mut bindings = Vec::new();
    for fact in &doc.seed {
        let SeedFact::Locus { name, .. } = fact else {
            continue;
        };
        let Some(tag) = kit.binds.get(name.as_str()) else {
            continue;
        };
        if kit.get(tag).is_none() {
            return Err(CompileError::MissingTag(tag.clone()));
        }
        bindings.push(Binding {
            locus: name.clone(),
            tag: Name::from(tag.as_str()),
            hull: *hull_ids.get(tag).expect("encoded"),
            mesh: *mesh_ids.get(tag).expect("encoded"),
            material: kit.get(tag).expect("tag").material,
        });
    }

    let cook_hash = cook_digest(doc, &kit_blobs);
    Ok(Cooked {
        canon,
        cook_hash,
        canon_hash: cook_hash,
        cas,
        dag,
        bindings,
        grains: grain_ids,
    })
}

fn put_artifact(
    dag: &mut ProvenanceDag,
    blob: klotho_core::BlobId,
    kind: ArtifactKind,
    license: klotho_prove::LicenseSpan,
    parents: &[klotho_prove::ProvenanceId],
) -> Result<(), CompileError> {
    dag.insert(ProvenanceKind::Artifact { blob, kind }, license, parents)
        .map_err(CompileError::prove)?;
    Ok(())
}

/// blake3 of canonical LE `(compiler version, canon RON, style, seed, kitbash ids)`.
fn cook_digest(doc: &IntentDoc, kit_blobs: &[klotho_core::BlobId]) -> Hash {
    let mut buf = Vec::new();
    buf.extend_from_slice(&COMPILER_VERSION.to_le_bytes());
    let canon = to_ron(&doc.canon_diffs).unwrap_or_default();
    let style = to_ron(&doc.style).unwrap_or_default();
    let seed = to_ron(&doc.seed).unwrap_or_default();
    put_bytes(&mut buf, canon.as_bytes());
    put_bytes(&mut buf, style.as_bytes());
    put_bytes(&mut buf, seed.as_bytes());
    let mut ids = kit_blobs.to_vec();
    ids.sort();
    ids.dedup();
    buf.extend_from_slice(&(ids.len() as u32).to_le_bytes());
    for id in ids {
        buf.extend_from_slice(id.as_bytes());
    }
    hash_bytes(&buf)
}

fn put_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);
}

/// Content id of bytes (tests / lockfile helpers).
#[must_use]
pub fn digest_of(bytes: &[u8]) -> Hash {
    hash_bytes(bytes)
}

/// Content id of a blob (same digest, distinct type).
#[must_use]
pub fn blob_of(bytes: &[u8]) -> klotho_core::BlobId {
    blob_id_of(bytes)
}

#[cfg(test)]
mod tests {
    use klotho_core::{Hash, LocusKind};
    use klotho_ir::{IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent};

    use super::*;

    fn empty_doc(tags: &[&str]) -> IntentDoc {
        IntentDoc {
            style: StyleIntent {
                notes: String::new(),
                palettes: Vec::new(),
                kitbash_tags: tags.iter().map(|t| Name::from(*t)).collect(),
            },
            canon_diffs: Vec::new(),
            seed: vec![SeedFact::Locus {
                name: Name::from("oak_door"),
                kind: LocusKind::Relic,
            }],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        }
    }

    #[test]
    fn missing_tag_is_cook_error() {
        let doc = empty_doc(&["no.such.tag"]);
        let e = cook_doc(&doc).unwrap_err();
        assert!(matches!(e, CompileError::MissingTag(t) if t == "no.such.tag"));
    }

    #[test]
    fn hearth_tags_retrieve_and_hash_is_stable() {
        let mut doc = empty_doc(&Kitbash::HEARTH_TAGS);
        doc.style.notes = "chunky readable silhouettes".into();
        doc.style.palettes = vec![
            Name::from("stone"),
            Name::from("metal"),
            Name::from("organic"),
        ];
        let a = cook_doc(&doc).unwrap();
        let b = cook_doc(&doc).unwrap();
        assert_eq!(a.cook_hash, b.cook_hash);
        assert_eq!(a.cas.len(), b.cas.len());
        assert!(a.dag.exportable().is_ok());
        assert!(
            a.bindings
                .iter()
                .any(|b| b.locus.as_str() == "oak_door" && b.tag.as_str() == "door.oak.lockable")
        );
        assert!(a.canon.rites.is_empty());
        // 12 meshes + 12 hulls + 3 grains.
        assert!(a.cas.len() >= 27);
        assert_eq!(a.grains.len(), 3);
    }

    #[test]
    fn hearth_doc_cooks_against_kitbash() {
        let doc = hearth_slice::hearth_doc();
        let cooked = cook_doc(&doc).unwrap();
        assert!(cooked.canon.rite_id("lockpick").is_some());
        assert!(
            cooked
                .bindings
                .iter()
                .any(|b| b.locus.as_str() == "fathers_hammer")
        );
        assert_eq!(cooked.cook_hash, cook_doc(&doc).unwrap().cook_hash);
    }
}
