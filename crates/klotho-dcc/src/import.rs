//! glTF 2.0 JSON + buffers → quantized KLTH blobs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use klotho_compile::{
    DecodedClip, MAX_CLIP_SAMPLES, SKIN_WEIGHT_SUM, encode_clipset, encode_hull, encode_mesh_i16,
    encode_skinned_mesh, validate_skinned_mesh,
};
use klotho_core::{AabbMm, Hash, IVec3};
use klotho_ir::Verb;
use klotho_prove::{LicenseSpan, hash_bytes};
use serde::Deserialize;
use serde_json::Value;

use crate::GltfImport;
use crate::error::DccError;

const MODE_TRIANGLES: u32 = 4;
const COMP_U8: u32 = 5121;
const COMP_U16: u32 = 5123;
const COMP_U32: u32 = 5125;
const COMP_F32: u32 = 5126;
const TICK_MS: i32 = 50;
const IDENT: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

#[derive(Deserialize)]
struct GltfDoc {
    asset: Asset,
    #[serde(default)]
    scene: Option<u32>,
    #[serde(default)]
    scenes: Vec<Scene>,
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    meshes: Vec<Mesh>,
    #[serde(default)]
    accessors: Vec<Accessor>,
    #[serde(default, rename = "bufferViews")]
    buffer_views: Vec<BufferView>,
    #[serde(default)]
    buffers: Vec<GltfBuffer>,
    #[serde(default)]
    animations: Vec<Animation>,
    #[serde(default)]
    skins: Vec<Skin>,
}

#[derive(Deserialize)]
struct Asset {
    version: String,
}

#[derive(Deserialize)]
struct Scene {
    #[serde(default)]
    nodes: Vec<u32>,
}

#[derive(Deserialize)]
struct Node {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mesh: Option<u32>,
    #[serde(default)]
    skin: Option<u32>,
    #[serde(default)]
    children: Vec<u32>,
    #[serde(default)]
    translation: Option<[f32; 3]>,
    #[serde(default)]
    rotation: Option<[f32; 4]>,
    #[serde(default)]
    scale: Option<[f32; 3]>,
    #[serde(default)]
    matrix: Option<[f32; 16]>,
    #[serde(default)]
    extras: Option<Value>,
}

#[derive(Deserialize)]
struct Mesh {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    primitives: Vec<Primitive>,
    #[serde(default)]
    extras: Option<Value>,
}

#[derive(Deserialize)]
struct Primitive {
    #[serde(default)]
    attributes: BTreeMap<String, u32>,
    #[serde(default)]
    indices: Option<u32>,
    #[serde(default)]
    mode: Option<u32>,
    #[serde(default)]
    extras: Option<Value>,
}

#[derive(Deserialize)]
struct Accessor {
    #[serde(default, rename = "bufferView")]
    buffer_view: Option<u32>,
    #[serde(default, rename = "byteOffset")]
    byte_offset: u32,
    #[serde(rename = "componentType")]
    component_type: u32,
    count: u32,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    sparse: Option<Value>,
}

#[derive(Deserialize)]
struct BufferView {
    buffer: u32,
    #[serde(default, rename = "byteOffset")]
    byte_offset: u32,
    #[serde(rename = "byteLength")]
    byte_length: u32,
    #[serde(default, rename = "byteStride")]
    byte_stride: Option<u32>,
}

#[derive(Deserialize)]
struct GltfBuffer {
    #[serde(rename = "byteLength")]
    byte_length: u32,
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Deserialize)]
struct Animation {
    #[serde(default)]
    channels: Vec<AnimChannel>,
    #[serde(default)]
    samplers: Vec<AnimSampler>,
    #[serde(default)]
    extras: Option<Value>,
}

#[derive(Deserialize)]
struct AnimChannel {
    sampler: u32,
    target: AnimTarget,
}

#[derive(Deserialize)]
struct AnimTarget {
    #[serde(default)]
    node: Option<u32>,
    path: String,
}

#[derive(Deserialize)]
struct AnimSampler {
    input: u32,
    output: u32,
    #[serde(default)]
    interpolation: Option<String>,
}

#[derive(Deserialize)]
struct Skin {
    #[serde(default)]
    joints: Vec<u32>,
}

#[derive(Clone, Default)]
struct KlothoMeta {
    affordance: Option<String>,
    license: Option<LicenseSpan>,
    verb: Option<String>,
    grounded: Option<bool>,
    looping: Option<bool>,
}

/// Cook `path` as glTF 2.0 JSON. Relative buffer URIs resolve next to the file.
pub fn import_gltf(path: &Path) -> Result<Vec<GltfImport>, DccError> {
    let json = fs::read(path).map_err(|e| DccError::Io(format!("{}: {e}", path.display())))?;
    let sidecar = read_sidecar(path)?;
    import_inner(&json, None, path.parent(), sidecar)
}

/// Cook glTF JSON bytes. `bin` is buffer 0 when the JSON has no `uri`.
pub fn import_gltf_bytes(json: &[u8], bin: Option<&[u8]>) -> Result<Vec<GltfImport>, DccError> {
    import_inner(json, bin, None, None)
}

fn import_inner(
    json: &[u8],
    bin: Option<&[u8]>,
    base: Option<&Path>,
    sidecar: Option<LicenseSpan>,
) -> Result<Vec<GltfImport>, DccError> {
    let doc: GltfDoc =
        serde_json::from_slice(json).map_err(|e| DccError::Gltf(format!("json: {e}")))?;
    if doc.asset.version != "2.0" {
        return Err(DccError::Gltf(format!(
            "glTF version {} (need 2.0)",
            doc.asset.version
        )));
    }
    let (buffers, source_hash) = load_buffers(&doc, json, bin, base)?;
    let ctx = Ctx {
        doc,
        buffers,
        sidecar,
        source_hash,
    };
    ctx.cook()
}

struct Ctx {
    doc: GltfDoc,
    buffers: Vec<Vec<u8>>,
    sidecar: Option<LicenseSpan>,
    source_hash: Hash,
}

impl Ctx {
    fn cook(&self) -> Result<Vec<GltfImport>, DccError> {
        let nodes = self.contributing_nodes()?;
        if nodes.is_empty() {
            return Err(DccError::Gltf("no geometry".into()));
        }
        let mut out = Vec::new();
        let mut tags = BTreeMap::new();
        for (idx, world) in nodes {
            let imp = self.cook_node(idx, world)?;
            if tags.insert(imp.tag.clone(), idx).is_some() {
                return Err(DccError::Gltf(format!("duplicate tag {}", imp.tag)));
            }
            out.push(imp);
        }
        Ok(out)
    }

    fn contributing_nodes(&self) -> Result<Vec<(u32, [f32; 16])>, DccError> {
        let mut out = Vec::new();
        let mut seen = BTreeMap::new();
        let roots: Vec<u32> = if let Some(si) = self.doc.scene {
            self.doc
                .scenes
                .get(si as usize)
                .ok_or_else(|| DccError::Gltf(format!("scene {si}")))?
                .nodes
                .clone()
        } else if let Some(scene) = self.doc.scenes.first() {
            scene.nodes.clone()
        } else {
            (0..self.doc.nodes.len() as u32).collect()
        };
        for r in roots {
            self.walk_node(r, IDENT, &mut seen, &mut out)?;
        }
        Ok(out)
    }

    fn walk_node(
        &self,
        idx: u32,
        parent: [f32; 16],
        seen: &mut BTreeMap<u32, ()>,
        out: &mut Vec<(u32, [f32; 16])>,
    ) -> Result<(), DccError> {
        if seen.insert(idx, ()).is_some() {
            return Ok(());
        }
        let node = self
            .doc
            .nodes
            .get(idx as usize)
            .ok_or_else(|| DccError::Gltf(format!("node {idx}")))?;
        let world = mul4(parent, local_matrix(node));
        if node.mesh.is_some() {
            out.push((idx, world));
        }
        for &c in &node.children {
            self.walk_node(c, world, seen, out)?;
        }
        Ok(())
    }

    fn cook_node(&self, idx: u32, world: [f32; 16]) -> Result<GltfImport, DccError> {
        let node = &self.doc.nodes[idx as usize];
        let mesh_i = node
            .mesh
            .ok_or_else(|| DccError::Gltf(format!("node {idx} has no mesh")))?;
        let mesh = self
            .doc
            .meshes
            .get(mesh_i as usize)
            .ok_or_else(|| DccError::Gltf(format!("mesh {mesh_i}")))?;
        let node_meta = parse_klotho(node.extras.as_ref())?;
        let mesh_meta = parse_klotho(mesh.extras.as_ref())?;
        let meta = overlay(node_meta, mesh_meta);
        let label = node
            .name
            .clone()
            .or_else(|| mesh.name.clone())
            .unwrap_or_else(|| format!("node {idx}"));

        let mut verts: Vec<[i16; 3]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut joints: Vec<[u8; 4]> = Vec::new();
        let mut weights: Vec<[u16; 4]> = Vec::new();
        let mut want_skin = false;
        let mut exported_tag: Option<String> = None;
        let mut exported_license: Option<LicenseSpan> = None;

        for prim in &mesh.primitives {
            let prim_only = parse_klotho(prim.extras.as_ref())?;
            let resolved = overlay(meta.clone(), prim_only);
            let tag = resolved
                .affordance
                .clone()
                .ok_or_else(|| DccError::MissingTag(label.clone()))?;
            if let Some(prev) = &exported_tag {
                if prev != &tag {
                    return Err(DccError::Gltf(format!("conflicting affordance on {label}")));
                }
            } else {
                exported_tag = Some(tag);
            }
            if let Some(lic) = resolved.license.clone() {
                if let Some(prev) = &exported_license {
                    if prev != &lic {
                        return Err(DccError::Gltf(format!("conflicting license on {label}")));
                    }
                } else {
                    exported_license = Some(lic);
                }
            }
            let mode = prim.mode.unwrap_or(MODE_TRIANGLES);
            if mode != MODE_TRIANGLES {
                return Err(DccError::Gltf("only TRIANGLES primitives".into()));
            }
            let pos_i = *prim
                .attributes
                .get("POSITION")
                .ok_or_else(|| DccError::Gltf(format!("{label}: missing POSITION")))?;
            let pos = self.read_vec3_f32(pos_i)?;
            let has_j = prim.attributes.contains_key("JOINTS_0");
            let has_w = prim.attributes.contains_key("WEIGHTS_0");
            if has_j != has_w {
                return Err(DccError::Gltf(format!(
                    "{label}: JOINTS_0/WEIGHTS_0 must both be present"
                )));
            }
            if verts.is_empty() {
                want_skin = has_j;
            } else if want_skin != has_j {
                return Err(DccError::Gltf(format!("{label}: mixed skinned primitives")));
            }
            let base = verts.len() as u32;
            for p in &pos {
                let w = transform_point(world, *p);
                let mm = [
                    meters_to_mm(w[0])?,
                    meters_to_mm(w[1])?,
                    meters_to_mm(w[2])?,
                ];
                verts.push([mm_i16(mm[0])?, mm_i16(mm[1])?, mm_i16(mm[2])?]);
            }
            if has_j {
                let j_i = *prim
                    .attributes
                    .get("JOINTS_0")
                    .ok_or_else(|| DccError::Gltf(format!("{label}: JOINTS_0")))?;
                let w_i = *prim
                    .attributes
                    .get("WEIGHTS_0")
                    .ok_or_else(|| DccError::Gltf(format!("{label}: WEIGHTS_0")))?;
                let js = self.read_joints(j_i)?;
                let ws = self.read_weights(w_i)?;
                if js.len() != pos.len() || ws.len() != pos.len() {
                    return Err(DccError::Gltf(format!("{label}: skin length")));
                }
                joints.extend_from_slice(&js);
                weights.extend_from_slice(&ws);
            }
            let ix = if let Some(ii) = prim.indices {
                self.read_indices(ii)?
            } else {
                if pos.len() % 3 != 0 {
                    return Err(DccError::Gltf(format!(
                        "{label}: non-indexed count not multiple of 3"
                    )));
                }
                (0..pos.len() as u32).collect()
            };
            for i in ix {
                if (i as usize) >= pos.len() {
                    return Err(DccError::Gltf(format!("{label}: index out of range")));
                }
                indices.push(
                    i.checked_add(base)
                        .ok_or_else(|| DccError::Gltf(format!("{label}: index overflow")))?,
                );
            }
        }

        let tag = exported_tag.ok_or_else(|| DccError::MissingTag(label.clone()))?;
        let license = exported_license
            .or_else(|| self.sidecar.clone())
            .ok_or_else(|| {
                DccError::License(format!("{label}: missing SPDX extras and sidecar"))
            })?;
        if !license.is_exportable() {
            return Err(DccError::License(format!(
                "{label}: license not exportable"
            )));
        }

        let mesh_bytes = encode_mesh_i16(&verts, &indices)?;
        let hull = encode_hull(hull_of(&verts));
        let skinned = if want_skin {
            let bones = if let Some(si) = node.skin {
                let skin = self
                    .doc
                    .skins
                    .get(si as usize)
                    .ok_or_else(|| DccError::Gltf(format!("skin {si}")))?;
                if skin.joints.is_empty() {
                    return Err(DccError::Gltf(format!("{label}: empty skin.joints")));
                }
                skin.joints.len() as u32
            } else {
                return Err(DccError::Gltf(format!(
                    "{label}: JOINTS_0 without node.skin"
                )));
            };
            for j in &joints {
                for &b in j {
                    if u32::from(b) >= bones && !(bones == 0 && b == 0) {
                        return Err(DccError::Gltf(format!("{label}: bone index out of range")));
                    }
                }
            }
            let bytes = encode_skinned_mesh(&verts, &joints, &weights, &indices, bones)?;
            validate_skinned_mesh(&bytes)?;
            Some(bytes)
        } else {
            None
        };
        let clips = self.cook_clips(idx, &meta, &label)?;

        Ok(GltfImport {
            tag,
            license,
            mesh: mesh_bytes,
            hull,
            skinned,
            clips,
            source_hash: self.source_hash,
        })
    }

    fn cook_clips(
        &self,
        node_idx: u32,
        node_meta: &KlothoMeta,
        label: &str,
    ) -> Result<Option<Vec<u8>>, DccError> {
        let mut clips = Vec::new();
        for (ci, anim) in self.doc.animations.iter().enumerate() {
            let mut trans = None;
            let mut targeted = false;
            for ch in &anim.channels {
                if ch.target.node != Some(node_idx) {
                    continue;
                }
                targeted = true;
                if ch.target.path == "translation" {
                    trans = Some(ch);
                }
            }
            if !targeted {
                continue;
            }
            let Some(ch) = trans else {
                return Err(DccError::Gltf(format!(
                    "{label}: animation has no translation channel"
                )));
            };
            let sampler = anim
                .samplers
                .get(ch.sampler as usize)
                .ok_or_else(|| DccError::Gltf(format!("{label}: sampler {}", ch.sampler)))?;
            let interp = sampler.interpolation.as_deref().unwrap_or("LINEAR");
            if interp != "LINEAR" && interp != "STEP" {
                return Err(DccError::Gltf(format!(
                    "{label}: unsupported interpolation {interp}"
                )));
            }
            let times = self.read_f32_scalar(sampler.input)?;
            let values = self.read_vec3_f32(sampler.output)?;
            let anim_meta = overlay(node_meta.clone(), parse_klotho(anim.extras.as_ref())?);
            let verb = anim_meta.verb.as_deref().ok_or_else(|| {
                DccError::Gltf(format!("{label}: animation missing extras.klotho.verb"))
            })?;
            clips.push(DecodedClip {
                verb: parse_verb(verb)?,
                grounded: anim_meta.grounded.unwrap_or(true),
                looping: anim_meta.looping.unwrap_or(true),
                id: u16::try_from(ci).map_err(|_| DccError::Gltf("clip id".into()))?,
                samples: sample_root_deltas(&times, &values, interp)?,
            });
        }
        if clips.is_empty() {
            return Ok(None);
        }
        Ok(Some(encode_clipset(&clips)?))
    }

    fn acc(&self, i: u32) -> Result<&Accessor, DccError> {
        self.doc
            .accessors
            .get(i as usize)
            .ok_or_else(|| DccError::Gltf(format!("accessor {i}")))
    }

    fn elem_ptr(&self, acc: &Accessor, elem: u32) -> Result<(&[u8], usize), DccError> {
        if acc.sparse.is_some() {
            return Err(DccError::Gltf("sparse accessors".into()));
        }
        let bv_i = acc
            .buffer_view
            .ok_or_else(|| DccError::Gltf("accessor missing bufferView".into()))?;
        let bv = self
            .doc
            .buffer_views
            .get(bv_i as usize)
            .ok_or_else(|| DccError::Gltf(format!("bufferView {bv_i}")))?;
        let buf = self
            .buffers
            .get(bv.buffer as usize)
            .ok_or_else(|| DccError::Gltf(format!("buffer {}", bv.buffer)))?;
        let view_end = (bv.byte_offset as usize)
            .checked_add(bv.byte_length as usize)
            .ok_or_else(|| DccError::Gltf("bufferView size overflow".into()))?;
        if view_end > buf.len() {
            return Err(DccError::Gltf("bufferView truncated".into()));
        }
        let comp = component_size(acc.component_type)?;
        let ncomp = type_count(&acc.ty)?;
        let packed = comp * ncomp;
        let stride = bv.byte_stride.unwrap_or(packed as u32) as usize;
        if stride < packed {
            return Err(DccError::Gltf("byteStride too small".into()));
        }
        let start = (bv.byte_offset as usize)
            .checked_add(acc.byte_offset as usize)
            .and_then(|s| s.checked_add(elem as usize * stride))
            .ok_or_else(|| DccError::Gltf("accessor offset overflow".into()))?;
        let end = start
            .checked_add(packed)
            .ok_or_else(|| DccError::Gltf("accessor size overflow".into()))?;
        if end > view_end {
            return Err(DccError::Gltf("accessor overrun".into()));
        }
        let slice = buf
            .get(start..end)
            .ok_or_else(|| DccError::Gltf("accessor truncated".into()))?;
        Ok((slice, packed))
    }

    fn read_vec3_f32(&self, i: u32) -> Result<Vec<[f32; 3]>, DccError> {
        let acc = self.acc(i)?;
        if acc.ty != "VEC3" || acc.component_type != COMP_F32 {
            return Err(DccError::Gltf("expected FLOAT VEC3".into()));
        }
        let mut out = Vec::with_capacity(acc.count as usize);
        for e in 0..acc.count {
            let (s, _) = self.elem_ptr(acc, e)?;
            out.push([f32_le(s, 0), f32_le(s, 4), f32_le(s, 8)]);
        }
        Ok(out)
    }

    fn read_f32_scalar(&self, i: u32) -> Result<Vec<f32>, DccError> {
        let acc = self.acc(i)?;
        if acc.ty != "SCALAR" || acc.component_type != COMP_F32 {
            return Err(DccError::Gltf("expected FLOAT SCALAR".into()));
        }
        let mut out = Vec::with_capacity(acc.count as usize);
        for e in 0..acc.count {
            let (s, _) = self.elem_ptr(acc, e)?;
            out.push(f32_le(s, 0));
        }
        Ok(out)
    }

    fn read_indices(&self, i: u32) -> Result<Vec<u32>, DccError> {
        let acc = self.acc(i)?;
        if acc.ty != "SCALAR" {
            return Err(DccError::Gltf("indices must be SCALAR".into()));
        }
        let mut out = Vec::with_capacity(acc.count as usize);
        for e in 0..acc.count {
            let (s, _) = self.elem_ptr(acc, e)?;
            let v = match acc.component_type {
                COMP_U8 => u32::from(s[0]),
                COMP_U16 => u32::from(u16::from_le_bytes([s[0], s[1]])),
                COMP_U32 => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                _ => return Err(DccError::Gltf("index componentType".into())),
            };
            out.push(v);
        }
        Ok(out)
    }

    fn read_joints(&self, i: u32) -> Result<Vec<[u8; 4]>, DccError> {
        let acc = self.acc(i)?;
        if acc.ty != "VEC4" {
            return Err(DccError::Gltf("JOINTS_0 must be VEC4".into()));
        }
        let mut out = Vec::with_capacity(acc.count as usize);
        for e in 0..acc.count {
            let (s, _) = self.elem_ptr(acc, e)?;
            let j = match acc.component_type {
                COMP_U8 => [s[0], s[1], s[2], s[3]],
                COMP_U16 => {
                    let mut j = [0u8; 4];
                    for k in 0..4 {
                        let v = u16::from_le_bytes([s[k * 2], s[k * 2 + 1]]);
                        j[k] = u8::try_from(v)
                            .map_err(|_| DccError::Gltf("joint index > 255".into()))?;
                    }
                    j
                }
                _ => return Err(DccError::Gltf("JOINTS_0 componentType".into())),
            };
            out.push(j);
        }
        Ok(out)
    }

    fn read_weights(&self, i: u32) -> Result<Vec<[u16; 4]>, DccError> {
        let acc = self.acc(i)?;
        if acc.ty != "VEC4" {
            return Err(DccError::Gltf("WEIGHTS_0 must be VEC4".into()));
        }
        let mut out = Vec::with_capacity(acc.count as usize);
        for e in 0..acc.count {
            let (s, _) = self.elem_ptr(acc, e)?;
            let w = match acc.component_type {
                COMP_F32 => [f32_le(s, 0), f32_le(s, 4), f32_le(s, 8), f32_le(s, 12)],
                COMP_U8 => [
                    f32::from(s[0]) / 255.0,
                    f32::from(s[1]) / 255.0,
                    f32::from(s[2]) / 255.0,
                    f32::from(s[3]) / 255.0,
                ],
                _ => return Err(DccError::Gltf("WEIGHTS_0 componentType".into())),
            };
            out.push(quantize_weights(w)?);
        }
        Ok(out)
    }
}

fn load_buffers(
    doc: &GltfDoc,
    json: &[u8],
    bin: Option<&[u8]>,
    base: Option<&Path>,
) -> Result<(Vec<Vec<u8>>, Hash), DccError> {
    let mut buffers = Vec::new();
    let mut external = Vec::new();
    if let Some(b) = bin {
        external.extend_from_slice(b);
    }
    for (i, b) in doc.buffers.iter().enumerate() {
        let data = match b.uri.as_deref() {
            None => bin
                .ok_or_else(|| DccError::Gltf(format!("buffer {i} missing uri and bin")))?
                .to_vec(),
            Some(uri) if uri.starts_with("data:") => decode_data_uri(uri)?,
            Some(uri) => {
                reject_buffer_uri(uri)?;
                let dir = base.ok_or_else(|| {
                    DccError::Gltf(format!("relative buffer uri {uri} without path"))
                })?;
                let p = resolve_buffer_path(dir, uri)?;
                let bytes =
                    fs::read(&p).map_err(|e| DccError::Io(format!("{}: {e}", p.display())))?;
                external.extend_from_slice(&bytes);
                bytes
            }
        };
        if data.len() < b.byte_length as usize {
            return Err(DccError::Gltf(format!("buffer {i} truncated")));
        }
        let mut data = data;
        data.truncate(b.byte_length as usize);
        buffers.push(data);
    }
    let source_hash = if external.is_empty() {
        hash_bytes(json)
    } else {
        let mut v = Vec::with_capacity(json.len() + external.len());
        v.extend_from_slice(json);
        v.extend_from_slice(&external);
        hash_bytes(&v)
    };
    Ok((buffers, source_hash))
}

fn reject_buffer_uri(uri: &str) -> Result<(), DccError> {
    let p = Path::new(uri);
    if uri.contains("..")
        || uri.contains(':')
        || uri.starts_with('/')
        || uri.starts_with('\\')
        || p.is_absolute()
    {
        return Err(DccError::Gltf(format!("rejected buffer uri {uri}")));
    }
    Ok(())
}

fn resolve_buffer_path(dir: &Path, uri: &str) -> Result<PathBuf, DccError> {
    reject_buffer_uri(uri)?;
    let p = dir.join(uri);
    if !p.starts_with(dir) {
        return Err(DccError::Gltf(format!("rejected buffer uri {uri}")));
    }
    Ok(p)
}

fn read_sidecar(path: &Path) -> Result<Option<LicenseSpan>, DccError> {
    let mut name = path.as_os_str().to_os_string();
    name.push(".license.json");
    let p = Path::new(&name);
    if !p.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(p).map_err(|e| DccError::Io(format!("{}: {e}", p.display())))?;
    let v: Value =
        serde_json::from_slice(&bytes).map_err(|e| DccError::License(format!("sidecar: {e}")))?;
    Ok(Some(parse_license_value(&v)?))
}

fn parse_klotho(extras: Option<&Value>) -> Result<KlothoMeta, DccError> {
    let Some(k) = extras.and_then(|e| e.get("klotho")) else {
        return Ok(KlothoMeta::default());
    };
    let affordance = k
        .get("affordance")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let license = match k.get("license") {
        Some(v) => Some(parse_license_value(v)?),
        None => None,
    };
    let verb = k
        .get("verb")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let grounded = k.get("grounded").and_then(Value::as_bool);
    let looping = k.get("looping").and_then(Value::as_bool);
    Ok(KlothoMeta {
        affordance,
        license,
        verb,
        grounded,
        looping,
    })
}

fn parse_license_value(v: &Value) -> Result<LicenseSpan, DccError> {
    let spdx = v.get("spdx").and_then(Value::as_str).unwrap_or("");
    if spdx.is_empty() {
        return Err(DccError::License("license.spdx required".into()));
    }
    let copyright = v.get("copyright").and_then(Value::as_str).unwrap_or("");
    LicenseSpan::spdx(spdx, copyright).map_err(|e| DccError::License(e.to_string()))
}

fn overlay(base: KlothoMeta, over: KlothoMeta) -> KlothoMeta {
    KlothoMeta {
        affordance: over.affordance.or(base.affordance),
        license: over.license.or(base.license),
        verb: over.verb.or(base.verb),
        grounded: over.grounded.or(base.grounded),
        looping: over.looping.or(base.looping),
    }
}

fn parse_verb(s: &str) -> Result<u8, DccError> {
    let v = match s {
        "Look" => Verb::Look,
        "Move" => Verb::Move,
        "Use" => Verb::Use,
        "Carry" => Verb::Carry,
        "Drop" => Verb::Drop,
        "Pay" => Verb::Pay,
        "Fire" => Verb::Fire,
        "Time" => Verb::Time,
        "Open" => Verb::Open,
        "Talk" => Verb::Talk,
        "Investigate" => Verb::Investigate,
        "Steer" => Verb::Steer,
        "Reload" => Verb::Reload,
        _ => return Err(DccError::Gltf(format!("unknown verb {s}"))),
    };
    Ok(v.as_u8())
}

fn meters_to_mm(m: f32) -> Result<i32, DccError> {
    let mm = f64::from(m) * 1000.0;
    let mm = mm.round();
    if !mm.is_finite() {
        return Err(DccError::Gltf("non-finite position".into()));
    }
    if mm < f64::from(i32::MIN) || mm > f64::from(i32::MAX) {
        return Err(DccError::QuantizeOverflow);
    }
    Ok(mm as i32)
}

fn mm_i16(mm: i32) -> Result<i16, DccError> {
    i16::try_from(mm).map_err(|_| DccError::QuantizeOverflow)
}

fn hull_of(verts: &[[i16; 3]]) -> AabbMm {
    let mut min = IVec3 {
        x: i32::from(verts[0][0]),
        y: i32::from(verts[0][1]),
        z: i32::from(verts[0][2]),
    };
    let mut max = min;
    for v in verts {
        let p = IVec3 {
            x: i32::from(v[0]),
            y: i32::from(v[1]),
            z: i32::from(v[2]),
        };
        min = min.min(p);
        max = max.max(p);
    }
    AabbMm::new(min, max)
}

fn sample_root_deltas(
    times: &[f32],
    values: &[[f32; 3]],
    interp: &str,
) -> Result<Vec<IVec3>, DccError> {
    if times.len() != values.len() || times.is_empty() {
        return Err(DccError::Gltf("animation sampler length".into()));
    }
    let duration = times[times.len() - 1];
    let duration_ms = (f64::from(duration) * 1000.0).round();
    if !duration_ms.is_finite() {
        return Err(DccError::Gltf("non-finite animation time".into()));
    }
    if duration_ms < f64::from(i32::MIN) || duration_ms > f64::from(i32::MAX) {
        return Err(DccError::Gltf("animation duration".into()));
    }
    let duration_ms = duration_ms as i32;
    if duration_ms <= 0 || times.len() == 1 {
        return Ok(vec![IVec3::ZERO]);
    }
    let ticks = duration_ms / TICK_MS;
    if ticks > i32::from(MAX_CLIP_SAMPLES) {
        return Err(DccError::Gltf(format!(
            "clip samples {ticks} > {MAX_CLIP_SAMPLES}"
        )));
    }
    let n = ticks.max(1) as usize;
    let mut prev = quantize_mm(sample_at(times, values, 0.0, interp)?)?;
    let mut deltas = Vec::with_capacity(n);
    for i in 1..=n {
        let t_ms = i as i32 * TICK_MS;
        let t = ((t_ms as f64) / 1000.0) as f32;
        let t = t.min(duration);
        let cur = quantize_mm(sample_at(times, values, t, interp)?)?;
        let x = cur
            .x
            .checked_sub(prev.x)
            .ok_or(DccError::QuantizeOverflow)?;
        let y = cur
            .y
            .checked_sub(prev.y)
            .ok_or(DccError::QuantizeOverflow)?;
        let z = cur
            .z
            .checked_sub(prev.z)
            .ok_or(DccError::QuantizeOverflow)?;
        deltas.push(IVec3 { x, y, z });
        prev = cur;
    }
    Ok(deltas)
}

fn quantize_mm(p: [f32; 3]) -> Result<IVec3, DccError> {
    Ok(IVec3 {
        x: meters_to_mm(p[0])?,
        y: meters_to_mm(p[1])?,
        z: meters_to_mm(p[2])?,
    })
}

fn sample_at(
    times: &[f32],
    values: &[[f32; 3]],
    t: f32,
    interp: &str,
) -> Result<[f32; 3], DccError> {
    if t <= times[0] {
        return Ok(values[0]);
    }
    let last = times.len() - 1;
    if t >= times[last] {
        return Ok(values[last]);
    }
    let mut i = 0;
    while i + 1 < times.len() && times[i + 1] <= t {
        i += 1;
    }
    if interp == "STEP" {
        return Ok(values[i]);
    }
    let t0 = times[i];
    let t1 = times[i + 1];
    let dt = t1 - t0;
    if dt <= 0.0 {
        return Ok(values[i]);
    }
    let u = (t - t0) / dt;
    Ok([
        values[i][0] + (values[i + 1][0] - values[i][0]) * u,
        values[i][1] + (values[i + 1][1] - values[i][1]) * u,
        values[i][2] + (values[i + 1][2] - values[i][2]) * u,
    ])
}

fn quantize_weights(w: [f32; 4]) -> Result<[u16; 4], DccError> {
    if w.iter().any(|x| !x.is_finite() || *x < 0.0) {
        return Err(DccError::Gltf("WEIGHTS_0 malformed".into()));
    }
    let sum = f64::from(w[0]) + f64::from(w[1]) + f64::from(w[2]) + f64::from(w[3]);
    if sum <= 0.0 {
        return Err(DccError::Gltf("WEIGHTS_0 sum".into()));
    }
    let n = [
        f64::from(w[0]) / sum,
        f64::from(w[1]) / sum,
        f64::from(w[2]) / sum,
        f64::from(w[3]) / sum,
    ];
    let mut u = [
        (n[0] * f64::from(SKIN_WEIGHT_SUM)).round() as u32,
        (n[1] * f64::from(SKIN_WEIGHT_SUM)).round() as u32,
        (n[2] * f64::from(SKIN_WEIGHT_SUM)).round() as u32,
        (n[3] * f64::from(SKIN_WEIGHT_SUM)).round() as u32,
    ];
    let s: u32 = u.iter().sum();
    if s == 0 {
        u[0] = SKIN_WEIGHT_SUM;
    } else if s != SKIN_WEIGHT_SUM {
        let mut max_i = 0usize;
        for i in 1..4 {
            if u[i] > u[max_i] {
                max_i = i;
            }
        }
        if s > SKIN_WEIGHT_SUM {
            u[max_i] = u[max_i].saturating_sub(s - SKIN_WEIGHT_SUM);
        } else {
            u[max_i] += SKIN_WEIGHT_SUM - s;
        }
    }
    Ok([u[0] as u16, u[1] as u16, u[2] as u16, u[3] as u16])
}

fn local_matrix(n: &Node) -> [f32; 16] {
    if let Some(m) = n.matrix {
        return m;
    }
    let t = n.translation.unwrap_or([0.0; 3]);
    let r = n.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let s = n.scale.unwrap_or([1.0; 3]);
    let rot_id = r == [0.0, 0.0, 0.0, 1.0];
    let scl_id = s == [1.0; 3];
    if rot_id && scl_id {
        let mut m = IDENT;
        m[12] = t[0];
        m[13] = t[1];
        m[14] = t[2];
        return m;
    }
    trs_mat(t, r, s)
}

fn trs_mat(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> [f32; 16] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;
    let r00 = 1.0 - 2.0 * (yy + zz);
    let r01 = 2.0 * (xy - wz);
    let r02 = 2.0 * (xz + wy);
    let r10 = 2.0 * (xy + wz);
    let r11 = 1.0 - 2.0 * (xx + zz);
    let r12 = 2.0 * (yz - wx);
    let r20 = 2.0 * (xz - wy);
    let r21 = 2.0 * (yz + wx);
    let r22 = 1.0 - 2.0 * (xx + yy);
    [
        r00 * s[0],
        r10 * s[0],
        r20 * s[0],
        0.0,
        r01 * s[1],
        r11 * s[1],
        r21 * s[1],
        0.0,
        r02 * s[2],
        r12 * s[2],
        r22 * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

fn mul4(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut r = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            r[col * 4 + row] = a[row] * b[col * 4]
                + a[4 + row] * b[col * 4 + 1]
                + a[8 + row] * b[col * 4 + 2]
                + a[12 + row] * b[col * 4 + 3];
        }
    }
    r
}

fn transform_point(m: [f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

fn component_size(ty: u32) -> Result<usize, DccError> {
    match ty {
        COMP_U8 => Ok(1),
        COMP_U16 => Ok(2),
        COMP_U32 | COMP_F32 => Ok(4),
        5120 | 5122 => Err(DccError::Gltf("signed accessor".into())),
        _ => Err(DccError::Gltf(format!("componentType {ty}"))),
    }
}

fn type_count(ty: &str) -> Result<usize, DccError> {
    match ty {
        "SCALAR" => Ok(1),
        "VEC2" => Ok(2),
        "VEC3" => Ok(3),
        "VEC4" => Ok(4),
        "MAT4" => Ok(16),
        _ => Err(DccError::Gltf(format!("accessor type {ty}"))),
    }
}

fn f32_le(s: &[u8], off: usize) -> f32 {
    f32::from_le_bytes([s[off], s[off + 1], s[off + 2], s[off + 3]])
}

fn decode_data_uri(uri: &str) -> Result<Vec<u8>, DccError> {
    let Some((_, b64)) = uri.split_once("base64,") else {
        return Err(DccError::Gltf("data uri must be base64".into()));
    };
    decode_base64(b64)
}

fn decode_base64(s: &str) -> Result<Vec<u8>, DccError> {
    fn val(c: u8) -> Result<u8, DccError> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(DccError::Gltf("invalid base64".into())),
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.len() % 4 != 0 {
        return Err(DccError::Gltf("invalid base64 length".into()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks_exact(4) {
        let pad = u8::from(chunk[2] == b'=') + u8::from(chunk[3] == b'=');
        let a = val(chunk[0])?;
        let b = val(chunk[1])?;
        let c = if chunk[2] == b'=' { 0 } else { val(chunk[2])? };
        let d = if chunk[3] == b'=' { 0 } else { val(chunk[3])? };
        let n = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}
