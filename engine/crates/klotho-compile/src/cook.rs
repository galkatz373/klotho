//! IntentDoc → Canon + CAS. Retrieval only; missing tag is a cook error.

use std::collections::BTreeMap;

use klotho_canon::{Canon, cook as cook_canon};
use klotho_core::Hash;
use klotho_ir::{IntentDoc, IntentModule, IntentProject, Name, SeedFact, to_ron};
use klotho_manifest::MaterialTag;
use klotho_prove::{
    Activity, Agent, ArtifactKind, Cas, LicenseSpan, ProveError, ProvenanceDag, ProvenanceKind,
    blob_id_of, hash_bytes,
};

use crate::encode::{
    encode_clipset, encode_grain, encode_hull, encode_mesh, encode_rite, hearth_biped_clips,
};
use crate::error::CompileError;
use crate::header::{
    validate_clipset, validate_grain, validate_hull, validate_mesh, validate_rite,
    validate_skinned_mesh,
};
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

/// Cooked warp contents: Canon, seed Intent, CAS, provenance.
#[derive(Debug)]
pub struct Cooked {
    /// Authoring document this cook came from (seed facts + diffs).
    pub doc: IntentDoc,
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
    /// ClipSet tag → blob (`biped` is the v1 Hearth table).
    pub clips: BTreeMap<String, klotho_core::BlobId>,
}

/// Quantized glTF blobs for [`cook_with_dcc`]. Compile does not parse glTF.
///
/// Bindings use [`MaterialTag::Organic`]: glTF extras do not carry a material
/// tag in v1 (closed default).
#[derive(Clone, Debug)]
pub struct DccArtifact {
    /// `extras.klotho.affordance`.
    pub tag: String,
    /// SPDX / commissioned span. [`LicenseSpan::Unknown`] fails export.
    pub license: LicenseSpan,
    /// KLTH ClusteredMesh bytes.
    pub mesh: Vec<u8>,
    /// KLTH Hull bytes.
    pub hull: Vec<u8>,
    /// KLTH SkinnedMesh bytes when the source had `JOINTS_0`.
    pub skinned: Option<Vec<u8>>,
    /// KLTH ClipSet bytes when animations were present.
    pub clips: Option<Vec<u8>>,
    /// blake3 of the glTF bytes, external buffers, and license sidecar.
    pub source_hash: Hash,
}

/// Cook `doc` against the workspace kitbash.
pub fn cook_doc(doc: &IntentDoc) -> Result<Cooked, CompileError> {
    cook_with(doc, &Kitbash::load_default()?)
}

/// Flatten a locked project, then cook the resulting [`IntentDoc`].
pub fn cook_project(
    project: &IntentProject,
    modules: &[IntentModule],
) -> Result<Cooked, CompileError> {
    let flat = project
        .flatten(modules)
        .map_err(|e| CompileError::Flatten(e.to_string()))?;
    cook_doc(&flat.doc)
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

    let mut clip_ids = BTreeMap::new();
    {
        let bytes = encode_clipset(&hearth_biped_clips())?;
        validate_clipset(&bytes)?;
        let id = cas.put(&bytes).map_err(CompileError::prove)?;
        put_artifact(
            &mut dag,
            id,
            ArtifactKind::ClipSet,
            license.clone(),
            &[cook_act, kit_agent],
        )?;
        clip_ids.insert("biped".into(), id);
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
        doc: doc.clone(),
        canon,
        cook_hash,
        canon_hash: cook_hash,
        cas,
        dag,
        bindings,
        grains: grain_ids,
        clips: clip_ids,
    })
}

/// Cook `doc` against already-quantized DCC artifacts. Does not load kitbash.
///
/// Every `style.kitbash_tags` entry must exist in `imports`. Bindings are
/// seed loci whose names match a tag, then unmatched style tags (locus =
/// tag). Material is [`MaterialTag::Organic`].
pub fn cook_with_dcc(doc: &IntentDoc, imports: &[DccArtifact]) -> Result<Cooked, CompileError> {
    let mut by_tag = BTreeMap::new();
    for a in imports {
        if a.tag.is_empty() {
            return Err(CompileError::MissingTag(String::new()));
        }
        if !a.license.is_exportable() {
            return Err(CompileError::prove(ProveError::UnknownLicense));
        }
        if by_tag.insert(a.tag.clone(), a).is_some() {
            return Err(CompileError::Gltf(format!("duplicate tag {}", a.tag)));
        }
    }
    for t in &doc.style.kitbash_tags {
        if !by_tag.contains_key(t.as_str()) {
            return Err(CompileError::MissingTag(t.as_str().to_string()));
        }
    }

    let canon = cook_canon(doc).map_err(CompileError::canon)?;
    if !canon.rites.is_empty() && imports.is_empty() {
        return Err(CompileError::Prove("rite cook needs a DCC license".into()));
    }

    let mut cas = Cas::new();
    let mut dag = ProvenanceDag::new();

    let Some(first) = imports.first() else {
        let cook_hash = cook_digest(doc, &[]);
        return Ok(Cooked {
            doc: doc.clone(),
            canon,
            cook_hash,
            canon_hash: cook_hash,
            cas,
            dag,
            bindings: Vec::new(),
            grains: BTreeMap::new(),
            clips: BTreeMap::new(),
        });
    };
    let license = first.license.clone();

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

    let mut src_nodes = Vec::new();
    for a in imports {
        let src = dag
            .insert(
                ProvenanceKind::Intent {
                    doc_hash: a.source_hash,
                },
                a.license.clone(),
                &[],
            )
            .map_err(CompileError::prove)?;
        src_nodes.push(src);
    }

    let mut cook_parents = vec![comp_agent, intent_node];
    cook_parents.extend_from_slice(&src_nodes);
    let cook_act = dag
        .insert(
            ProvenanceKind::Activity {
                activity: Activity::Cook,
            },
            license.clone(),
            &cook_parents,
        )
        .map_err(CompileError::prove)?;

    let mut mesh_ids = BTreeMap::new();
    let mut hull_ids = BTreeMap::new();
    let mut clip_ids = BTreeMap::new();
    let mut dcc_blobs = Vec::new();
    let mut semantic_blobs = Vec::new();

    for (i, a) in imports.iter().enumerate() {
        let src = src_nodes[i];
        let parents = [cook_act, comp_agent, src];

        validate_mesh(&a.mesh)?;
        validate_hull(&a.hull)?;
        let mid = cas.put(&a.mesh).map_err(CompileError::prove)?;
        let hid = cas.put(&a.hull).map_err(CompileError::prove)?;
        put_artifact(
            &mut dag,
            mid,
            ArtifactKind::ClusteredMesh,
            a.license.clone(),
            &parents,
        )?;
        put_artifact(
            &mut dag,
            hid,
            ArtifactKind::Hull,
            a.license.clone(),
            &parents,
        )?;
        mesh_ids.insert(a.tag.clone(), mid);
        hull_ids.insert(a.tag.clone(), hid);
        dcc_blobs.push(mid);
        dcc_blobs.push(hid);
        semantic_blobs.push(hid);

        if let Some(sk) = &a.skinned {
            validate_skinned_mesh(sk)?;
            let id = cas.put(sk).map_err(CompileError::prove)?;
            put_artifact(
                &mut dag,
                id,
                ArtifactKind::SkinnedMesh,
                a.license.clone(),
                &parents,
            )?;
            dcc_blobs.push(id);
        }
        if let Some(cl) = &a.clips {
            validate_clipset(cl)?;
            let id = cas.put(cl).map_err(CompileError::prove)?;
            put_artifact(
                &mut dag,
                id,
                ArtifactKind::ClipSet,
                a.license.clone(),
                &parents,
            )?;
            clip_ids.insert(a.tag.clone(), id);
            dcc_blobs.push(id);
        }
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
    let mut bound = BTreeMap::new();
    for fact in &doc.seed {
        let SeedFact::Locus { name, .. } = fact else {
            continue;
        };
        let Some(art) = by_tag.get(name.as_str()) else {
            continue;
        };
        bindings.push(Binding {
            locus: name.clone(),
            tag: Name::from(art.tag.as_str()),
            hull: *hull_ids.get(&art.tag).expect("encoded"),
            mesh: *mesh_ids.get(&art.tag).expect("encoded"),
            material: MaterialTag::Organic,
        });
        bound.insert(name.as_str().to_string(), ());
    }
    for t in &doc.style.kitbash_tags {
        if bound.contains_key(t.as_str()) {
            continue;
        }
        let art = by_tag.get(t.as_str()).expect("style tag checked");
        bindings.push(Binding {
            locus: t.clone(),
            tag: Name::from(art.tag.as_str()),
            hull: *hull_ids.get(&art.tag).expect("encoded"),
            mesh: *mesh_ids.get(&art.tag).expect("encoded"),
            material: MaterialTag::Organic,
        });
        bound.insert(t.as_str().to_string(), ());
    }

    let cook_hash = cook_digest(doc, &dcc_blobs);
    // Visual meshes/skinning/clips hot-swap without changing authoritative
    // ancestry. Only explicitly imported semantic hulls participate in the
    // World/Trace canon hash (K72); the complete artifact set remains in the
    // cook hash for reproducible packaging.
    let canon_hash = cook_digest(doc, &semantic_blobs);
    Ok(Cooked {
        doc: doc.clone(),
        canon,
        cook_hash,
        canon_hash,
        cas,
        dag,
        bindings,
        grains: BTreeMap::new(),
        clips: clip_ids,
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
pub(crate) fn cook_digest(doc: &IntentDoc, kit_blobs: &[klotho_core::BlobId]) -> Hash {
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
        // 12 meshes + 12 hulls + 3 grains + 1 biped ClipSet.
        assert!(a.cas.len() >= 28);
        assert_eq!(a.grains.len(), 3);
        assert_eq!(a.clips.len(), 1);
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

    #[test]
    fn migrated_hearth_flattens_to_same_cook() {
        let doc = hearth_slice::hearth_doc();
        let bundle = klotho_ir::migrate_doc(
            klotho_ir::Name::from("hearth"),
            klotho_ir::Name::from("main"),
            doc.clone(),
        )
        .unwrap();
        let flat = bundle.project.flatten(&bundle.modules).unwrap();
        assert_eq!(flat.doc, doc);
        let direct = cook_doc(&doc).unwrap();
        let via_project = cook_project(&bundle.project, &bundle.modules).unwrap();
        assert_eq!(direct.cook_hash, via_project.cook_hash);
        assert_eq!(direct.canon.laws.len(), via_project.canon.laws.len());
        assert_eq!(direct.canon.rites.len(), via_project.canon.rites.len());
        assert_eq!(direct.canon.pin_names, via_project.canon.pin_names);
    }

    #[test]
    fn unexpanded_pattern_fails_cook_project() {
        let doc = empty_doc(&[]);
        let mut bundle = klotho_ir::migrate_doc(
            klotho_ir::Name::from("spin"),
            klotho_ir::Name::from("main"),
            doc,
        )
        .unwrap();
        let instance = klotho_ir::PatternInstance {
            anchor: bundle.modules[0].anchor.child(b"pattern:gate"),
            module: bundle.modules[0].anchor,
            instance: Name::from("gate"),
            pattern: Name::from("traversal.door_key"),
            version: 1,
            args: Vec::new(),
        };
        bundle.modules[0]
            .object_anchors
            .push(klotho_ir::ObjectAnchor {
                kind: klotho_ir::AnchorKind::Pattern,
                name: Name::from("gate"),
                anchor: instance.anchor,
            });
        bundle.modules[0].patterns.push(instance);
        let err = cook_project(&bundle.project, &bundle.modules).unwrap_err();
        match err {
            CompileError::Flatten(s) => assert!(s.contains("UnexpandedPattern"), "{s}"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn cook_doc_does_not_need_dcc() {
        let cooked = cook_doc(&empty_doc(&Kitbash::HEARTH_TAGS)).unwrap();
        assert!(cooked.dag.exportable().is_ok());
        assert!(cooked.cas.len() >= 28);
    }

    fn dcc_tri(tag: &str, license: LicenseSpan) -> DccArtifact {
        let mesh =
            crate::encode_mesh_i16(&[[0, 0, 0], [100, 0, 0], [0, 100, 0]], &[0, 1, 2]).unwrap();
        let hull = crate::encode_hull(klotho_core::AabbMm::new(
            klotho_core::IVec3 { x: 0, y: 0, z: 0 },
            klotho_core::IVec3 {
                x: 100,
                y: 100,
                z: 0,
            },
        ));
        DccArtifact {
            tag: tag.into(),
            license,
            mesh,
            hull,
            skinned: None,
            clips: None,
            source_hash: Hash::ZERO,
        }
    }

    #[test]
    fn dcc_style_tag_missing_is_cook_error() {
        let lic = LicenseSpan::spdx("CC0-1.0", "test").unwrap();
        let art = dcc_tri("prop.cube.portable", lic);
        let e = cook_with_dcc(&empty_doc(&["no.such.tag"]), &[art]).unwrap_err();
        assert!(matches!(e, CompileError::MissingTag(t) if t == "no.such.tag"));
    }

    #[test]
    fn dcc_unknown_license_fails_export() {
        let art = dcc_tri("prop.cube.portable", LicenseSpan::Unknown);
        let e = cook_with_dcc(&empty_doc(&["prop.cube.portable"]), &[art]).unwrap_err();
        assert!(matches!(e, CompileError::Prove(s) if s.contains("UnknownLicense")));
    }

    #[test]
    fn dcc_tagged_ingest_is_exportable() {
        let lic = LicenseSpan::spdx("CC0-1.0", "Klotho fixtures").unwrap();
        let art = dcc_tri("prop.cube.portable", lic);
        let mut doc = empty_doc(&["prop.cube.portable"]);
        doc.seed = vec![SeedFact::Locus {
            name: Name::from("prop.cube.portable"),
            kind: LocusKind::Relic,
        }];
        let cooked = cook_with_dcc(&doc, &[art]).unwrap();
        assert!(cooked.dag.exportable().is_ok());
        assert_eq!(cooked.bindings.len(), 1);
        assert_eq!(cooked.bindings[0].tag.as_str(), "prop.cube.portable");
        assert_eq!(cooked.bindings[0].material, MaterialTag::Organic);
        assert!(cooked.grains.is_empty());
    }

    #[test]
    fn dcc_style_tags_bind_when_seed_name_differs() {
        let lic = LicenseSpan::spdx("CC0-1.0", "Klotho fixtures").unwrap();
        let art = dcc_tri("prop.cube.portable", lic);
        // empty_doc seeds oak_door, which is not the DCC tag.
        let cooked = cook_with_dcc(&empty_doc(&["prop.cube.portable"]), &[art]).unwrap();
        assert_eq!(cooked.bindings.len(), 1);
        assert_eq!(cooked.bindings[0].locus.as_str(), "prop.cube.portable");
        assert_eq!(cooked.bindings[0].tag.as_str(), "prop.cube.portable");
        assert_eq!(cooked.bindings[0].material, MaterialTag::Organic);
    }

    #[test]
    fn visual_only_dcc_rebake_preserves_authoritative_hash() {
        let lic = LicenseSpan::spdx("CC0-1.0", "Klotho fixtures").unwrap();
        let a = dcc_tri("prop.cube.portable", lic);
        let mut b = a.clone();
        b.mesh = crate::encode_mesh_i16(&[[0, 0, 0], [90, 0, 0], [0, 90, 0]], &[0, 1, 2]).unwrap();
        let doc = empty_doc(&["prop.cube.portable"]);
        let before = cook_with_dcc(&doc, &[a]).unwrap();
        let after = cook_with_dcc(&doc, &[b]).unwrap();
        assert_ne!(before.cook_hash, after.cook_hash);
        assert_eq!(before.canon_hash, after.canon_hash);
    }
}
