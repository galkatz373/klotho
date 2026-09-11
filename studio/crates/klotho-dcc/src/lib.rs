//! Cook-time glTF 2.0 importer.
//!
//! `extras.klotho.affordance` is required on each contributing mesh/node.
//! Positions are meters, quantized to `i16` millimetres (round-to-nearest,
//! FMA-free). Clip translation is sampled every 50 ms, then stored as integer
//! millimetre root deltas. Hashed CAS bytes are little-endian integers (K20).
//! Missing affordance tag is a cook error. Kitbash retrieval is unchanged.
//!
//! ```json
//! "extras": {
//!   "klotho": {
//!     "affordance": "prop.cube.portable",
//!     "license": { "spdx": "CC0-1.0", "copyright": "Klotho fixtures" },
//!     "verb": "Move",
//!     "grounded": true,
//!     "looping": true
//!   }
//! }
//! ```
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod import;

pub use error::DccError;
pub use import::{import_gltf, import_gltf_bytes};

use klotho_compile::DccArtifact;
use klotho_core::Hash;
use klotho_prove::LicenseSpan;

/// One tagged glTF mesh cooked to KLTH blobs.
#[derive(Clone, Debug)]
pub struct GltfImport {
    /// `extras.klotho.affordance`.
    pub tag: String,
    /// SPDX span from extras or sidecar. Unknown is refused at import.
    pub license: LicenseSpan,
    /// KLTH ClusteredMesh.
    pub mesh: Vec<u8>,
    /// KLTH Hull of the quantized verts.
    pub hull: Vec<u8>,
    /// KLTH SkinnedMesh when `JOINTS_0` / `WEIGHTS_0` are present.
    pub skinned: Option<Vec<u8>>,
    /// KLTH ClipSet when a translation animation targets the node.
    pub clips: Option<Vec<u8>>,
    /// blake3 of the glTF JSON, external buffers, and license sidecar.
    pub source_hash: Hash,
}

impl From<GltfImport> for DccArtifact {
    fn from(g: GltfImport) -> Self {
        Self {
            tag: g.tag,
            license: g.license,
            mesh: g.mesh,
            hull: g.hull,
            skinned: g.skinned,
            clips: g.clips,
            source_hash: g.source_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use klotho_compile::{
        CompileError, cook_doc, cook_with_dcc, decode_clipset, decode_mesh, encode_mesh_i16,
        validate_clipset, validate_hull, validate_mesh,
    };
    use klotho_core::{Hash, IVec3, LocusKind};
    use klotho_ir::{IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent, Verb};
    use klotho_prove::hash_bytes;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/dcc")
            .join(name)
    }

    fn cube_verts() -> Vec<[i16; 3]> {
        vec![
            [-125, 0, -125],
            [125, 0, -125],
            [125, 0, 125],
            [-125, 0, 125],
            [-125, 250, -125],
            [125, 250, -125],
            [125, 250, 125],
            [-125, 250, 125],
        ]
    }

    #[test]
    fn untagged_is_cook_error() {
        let e = import_gltf(&fixture("untagged.gltf")).unwrap_err();
        assert!(matches!(e, DccError::MissingTag(_)), "{e}");
    }

    #[test]
    fn cube_imports_mesh_hull_and_pinned_hash() {
        let a = import_gltf(&fixture("cube.gltf")).unwrap();
        let json = fs::read(fixture("cube.gltf")).unwrap();
        let b = import_gltf_bytes(&json, None).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].mesh, b[0].mesh);
        assert_eq!(a[0].hull, b[0].hull);
        assert_eq!(a[0].tag, "prop.cube.portable");
        assert!(a[0].license.is_exportable());
        assert!(a[0].skinned.is_none());
        assert!(a[0].clips.is_none());

        let info = validate_mesh(&a[0].mesh).unwrap();
        assert_eq!(info.verts, 8);
        assert_eq!(info.tris(), 12);
        validate_hull(&a[0].hull).unwrap();
        let decoded = decode_mesh(&a[0].mesh).unwrap();
        assert_eq!(decoded.verts, cube_verts());

        let expected = encode_mesh_i16(&decoded.verts, &decoded.indices).unwrap();
        assert_eq!(a[0].mesh, expected);
        // CI is three-OS; hashed bytes are little-endian i16/u32 only.
        assert_eq!(
            hash_bytes(&a[0].mesh).to_string(),
            "2947beed724d7dd202920d7ea53778ded77814e28577c2a79b994204842f22e1"
        );
        assert_eq!(
            hash_bytes(&a[0].hull).to_string(),
            "f5fbfc3c88e4d948b1059f930608b347ff3f939bb98de71fabf0b9b20b57def3"
        );
    }

    #[test]
    fn missing_license_fails_import() {
        let e = import_gltf(&fixture("nolicense.gltf")).unwrap_err();
        assert!(matches!(e, DccError::License(_)), "{e}");
    }

    #[test]
    fn license_sidecar_bytes_participate_in_source_hash() {
        let dir = std::env::temp_dir().join(format!("klotho-dcc-sidecar-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cube.gltf");
        fs::copy(fixture("cube.gltf"), &path).unwrap();
        let sidecar = dir.join("cube.gltf.license.json");
        fs::write(&sidecar, br#"{"spdx":"CC0-1.0","copyright":"first"}"#).unwrap();
        let first = import_gltf(&path).unwrap()[0].source_hash;
        fs::write(&sidecar, br#"{"spdx":"CC0-1.0","copyright":"second"}"#).unwrap();
        let second = import_gltf(&path).unwrap()[0].source_hash;
        assert_ne!(first, second);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn walk_produces_validating_clipset() {
        let v = import_gltf(&fixture("walk.gltf")).unwrap();
        assert_eq!(v[0].tag, "npc.human.biped");
        let clips = v[0].clips.as_ref().expect("clip");
        validate_clipset(clips).unwrap();
        let decoded = decode_clipset(clips).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].verb, Verb::Move.as_u8());
        assert!(decoded[0].grounded);
        assert!(decoded[0].looping);
        assert_eq!(decoded[0].samples, vec![IVec3 { x: 0, y: 0, z: 20 }]);
    }

    #[test]
    fn cube_cook_with_dcc_is_exportable() {
        let imports: Vec<DccArtifact> = import_gltf(&fixture("cube.gltf"))
            .unwrap()
            .into_iter()
            .map(Into::into)
            .collect();
        let doc = IntentDoc {
            style: StyleIntent {
                notes: String::new(),
                palettes: Vec::new(),
                kitbash_tags: vec![Name::from("prop.cube.portable")],
            },
            canon_diffs: Vec::new(),
            seed: vec![SeedFact::Locus {
                name: Name::from("prop.cube.portable"),
                kind: LocusKind::Relic,
            }],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        };
        let cooked = cook_with_dcc(&doc, &imports).unwrap();
        assert!(cooked.dag.exportable().is_ok());
        assert_eq!(cooked.bindings.len(), 1);
        assert!(cooked.grains.is_empty());
    }

    #[test]
    fn dcc_style_tag_missing_is_missing_tag() {
        let imports: Vec<DccArtifact> = import_gltf(&fixture("cube.gltf"))
            .unwrap()
            .into_iter()
            .map(Into::into)
            .collect();
        let doc = IntentDoc {
            style: StyleIntent {
                notes: String::new(),
                palettes: Vec::new(),
                kitbash_tags: vec![Name::from("no.such.tag")],
            },
            canon_diffs: Vec::new(),
            seed: Vec::new(),
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        };
        let e = cook_with_dcc(&doc, &imports).unwrap_err();
        assert!(matches!(e, CompileError::MissingTag(t) if t == "no.such.tag"));
    }

    #[test]
    fn cook_doc_does_not_need_dcc() {
        let doc = IntentDoc {
            style: StyleIntent {
                notes: String::new(),
                palettes: Vec::new(),
                kitbash_tags: vec![Name::from("door.oak.lockable")],
            },
            canon_diffs: Vec::new(),
            seed: Vec::new(),
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        };
        let cooked = cook_doc(&doc).unwrap();
        assert!(cooked.dag.exportable().is_ok());
    }

    #[test]
    fn quantize_overflow_is_cook_error() {
        let json = br#"{
  "asset": {"version": "2.0"},
  "scenes": [{"nodes": [0]}],
  "nodes": [{
    "mesh": 0,
    "extras": {"klotho": {
      "affordance": "prop.huge",
      "license": {"spdx": "CC0-1.0", "copyright": "t"}
    }}
  }],
  "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
  "accessors": [
    {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
     "min": [0,0,0], "max": [40,0,0]},
    {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
  ],
  "bufferViews": [
    {"buffer": 0, "byteOffset": 0, "byteLength": 36},
    {"buffer": 0, "byteOffset": 36, "byteLength": 6}
  ],
  "buffers": [{"uri": "data:application/octet-stream;base64,AAAgQgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgD8AAAAAAAABAAIA", "byteLength": 42}]
}"#;
        // 40 m → 40000 mm does not fit i16.
        let e = import_gltf_bytes(json, None).unwrap_err();
        assert!(matches!(e, DccError::QuantizeOverflow), "{e}");
    }

    const TRI_EXTRAS: &str = r#""extras": {"klotho": {"affordance": "prop.tri", "license": {"spdx": "CC0-1.0", "copyright": "t"}}}"#;

    #[test]
    fn accessor_overrun_is_cook_error() {
        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, {TRI_EXTRAS}}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1}}]}}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 24}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 6}}
  ],
  "buffers": [{{"uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAzczMPQAAAAAAAAAAAAAAAM3MzD0AAAAAAAABAAIA", "byteLength": 42}}]
}}"#
        );
        let e = import_gltf_bytes(json.as_bytes(), None).unwrap_err();
        assert!(
            matches!(e, DccError::Gltf(ref s) if s.contains("overrun")),
            "{e}"
        );
    }

    #[test]
    fn multi_primitive_oob_index_is_cook_error() {
        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, {TRI_EXTRAS}}}],
  "meshes": [{{"primitives": [
    {{"attributes": {{"POSITION": 0}}, "indices": 1}},
    {{"attributes": {{"POSITION": 2}}, "indices": 3}}
  ]}}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}},
    {{"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": 72, "byteLength": 6}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": 78, "byteLength": 6}}
  ],
  "buffers": [{{"uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAzczMPQAAAAAAAAAAAAAAAM3MzD0AAAAAzcxMPgAAAAAAAAAAmpmZPgAAAAAAAAAAzcxMPs3MzD0AAAAAAAABAAMAAAABAAIA", "byteLength": 84}}]
}}"#
        );
        let e = import_gltf_bytes(json.as_bytes(), None).unwrap_err();
        assert!(
            matches!(e, DccError::Gltf(ref s) if s.contains("index out of range")),
            "{e}"
        );
    }

    #[test]
    fn clip_oversize_is_cook_error() {
        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, {TRI_EXTRAS}}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1}}]}}],
  "animations": [{{
    "extras": {{"klotho": {{"verb": "Move"}}}},
    "channels": [{{"sampler": 0, "target": {{"node": 0, "path": "translation"}}}}],
    "samplers": [{{"input": 2, "interpolation": "LINEAR", "output": 3}}]
  }}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}},
    {{"bufferView": 2, "componentType": 5126, "count": 2, "type": "SCALAR"}},
    {{"bufferView": 3, "componentType": 5126, "count": 2, "type": "VEC3"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 6}},
    {{"buffer": 0, "byteOffset": 42, "byteLength": 8}},
    {{"buffer": 0, "byteOffset": 50, "byteLength": 24}}
  ],
  "buffers": [{{"uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAzczMPQAAAAAAAAAAAAAAAM3MzD0AAAAAAAABAAIAAAAAAAAAUEEAAAAAAAAAAAAAAAAAAAAAAAAAAArXozw=", "byteLength": 74}}]
}}"#
        );
        let e = import_gltf_bytes(json.as_bytes(), None).unwrap_err();
        assert!(
            matches!(e, DccError::Gltf(ref s) if s.contains("clip samples")),
            "{e}"
        );
    }

    #[test]
    fn absolute_buffer_uri_is_cook_error() {
        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, {TRI_EXTRAS}}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1}}]}}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 6}}
  ],
  "buffers": [{{"uri": "/etc/passwd", "byteLength": 42}}]
}}"#
        );
        let e = import_gltf_bytes(json.as_bytes(), None).unwrap_err();
        assert!(
            matches!(e, DccError::Gltf(ref s) if s.contains("rejected buffer uri")),
            "{e}"
        );
    }

    #[test]
    fn step_exact_keyframe_uses_that_sample() {
        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, {TRI_EXTRAS}}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1}}]}}],
  "animations": [{{
    "extras": {{"klotho": {{"verb": "Move"}}}},
    "channels": [{{"sampler": 0, "target": {{"node": 0, "path": "translation"}}}}],
    "samplers": [{{"input": 2, "interpolation": "STEP", "output": 3}}]
  }}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}},
    {{"bufferView": 2, "componentType": 5126, "count": 3, "type": "SCALAR"}},
    {{"bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 6}},
    {{"buffer": 0, "byteOffset": 42, "byteLength": 12}},
    {{"buffer": 0, "byteOffset": 54, "byteLength": 36}}
  ],
  "buffers": [{{"uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAzczMPQAAAAAAAAAAAAAAAM3MzD0AAAAAAAABAAIAAAAAAM3MTD3NzMw9AAAAAAAAAAAAAAAAAAAAAAAAAAAK16M8AAAAAAAAAAAK1yM9", "byteLength": 90}}]
}}"#
        );
        let v = import_gltf_bytes(json.as_bytes(), None).unwrap();
        let clips = decode_clipset(v[0].clips.as_ref().unwrap()).unwrap();
        assert_eq!(
            clips[0].samples,
            vec![IVec3 { x: 0, y: 0, z: 20 }, IVec3 { x: 0, y: 0, z: 20 }]
        );
    }

    #[test]
    fn joint_index_out_of_range_is_cook_error() {
        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, "skin": 0, {TRI_EXTRAS}}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0, "JOINTS_0": 2, "WEIGHTS_0": 3}}, "indices": 1}}]}}],
  "skins": [{{"joints": [0]}}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}},
    {{"bufferView": 2, "componentType": 5121, "count": 3, "type": "VEC4"}},
    {{"bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC4"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": 96, "byteLength": 6}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 12}},
    {{"buffer": 0, "byteOffset": 48, "byteLength": 48}}
  ],
  "buffers": [{{"uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAzczMPQAAAAAAAAAAAAAAAM3MzD0AAAAAAQAAAAEAAAABAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAAAAAAAAAAAAAAAIA/AAAAAAAAAAAAAAAAAAABAAIA", "byteLength": 102}}]
}}"#
        );
        let e = import_gltf_bytes(json.as_bytes(), None).unwrap_err();
        assert!(
            matches!(e, DccError::Gltf(ref s) if s.contains("bone index")),
            "{e}"
        );
    }

    #[test]
    fn primitive_extras_tag_and_license_export() {
        let json = br#"{
  "asset": {"version": "2.0"},
  "scenes": [{"nodes": [0]}],
  "nodes": [{"mesh": 0}],
  "meshes": [{"primitives": [{
    "attributes": {"POSITION": 0},
    "indices": 1,
    "extras": {"klotho": {
      "affordance": "prop.prim.only",
      "license": {"spdx": "CC0-1.0", "copyright": "t"}
    }}
  }]}],
  "accessors": [
    {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
    {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
  ],
  "bufferViews": [
    {"buffer": 0, "byteOffset": 0, "byteLength": 36},
    {"buffer": 0, "byteOffset": 36, "byteLength": 6}
  ],
  "buffers": [{"uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAzczMPQAAAAAAAAAAAAAAAM3MzD0AAAAAAAABAAIA", "byteLength": 42}]
}"#;
        let v = import_gltf_bytes(json, None).unwrap();
        assert_eq!(v[0].tag, "prop.prim.only");
        assert!(v[0].license.is_exportable());
        validate_mesh(&v[0].mesh).unwrap();
    }

    #[test]
    fn clip_delta_overflow_is_quantize_error() {
        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, {TRI_EXTRAS}}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1}}]}}],
  "animations": [{{
    "extras": {{"klotho": {{"verb": "Move"}}}},
    "channels": [{{"sampler": 0, "target": {{"node": 0, "path": "translation"}}}}],
    "samplers": [{{"input": 2, "interpolation": "LINEAR", "output": 3}}]
  }}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}},
    {{"bufferView": 2, "componentType": 5126, "count": 2, "type": "SCALAR"}},
    {{"bufferView": 3, "componentType": 5126, "count": 2, "type": "VEC3"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 6}},
    {{"buffer": 0, "byteOffset": 42, "byteLength": 8}},
    {{"buffer": 0, "byteOffset": 50, "byteLength": 24}}
  ],
  "buffers": [{{"uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAzczMPQAAAAAAAAAAAAAAAM3MzD0AAAAAAAABAAIAAAAAAM3MTD0AJPRJAAAAAAAAAAAAJPTJAAAAAAAAAAA=", "byteLength": 74}}]
}}"#
        );
        let e = import_gltf_bytes(json.as_bytes(), None).unwrap_err();
        assert!(matches!(e, DccError::QuantizeOverflow), "{e}");
    }
}
