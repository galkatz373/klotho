//! `KLTH` artifact headers. Validated before GPU / decoder upload (PR 12 / 14).

use klotho_core::{AabbMm, IVec3};
use klotho_prove::ArtifactKind;

use crate::error::CompileError;

/// Four-byte magic. ASCII `KLTH`.
pub const MAGIC: [u8; 4] = *b"KLTH";
/// Header version. Bump invalidates every blob id.
pub const VERSION: u8 = 1;
/// HLD §4: index count / 3 ≤ this.
pub const MAX_TRIS: u32 = 200_000;
/// HLD §4: rite `cap_steps` ≤ this.
pub const MAX_RITE_STEPS: u16 = 64;
/// PCM sample rate for v1 grains.
pub const GRAIN_HZ: u32 = 48_000;

/// Bytes of the fixed prefix: magic + version + kind + pad.
pub const PREFIX: usize = 8;

/// Write the 8-byte prefix.
pub(crate) fn write_prefix(buf: &mut Vec<u8>, kind: ArtifactKind) {
    buf.extend_from_slice(&MAGIC);
    buf.push(VERSION);
    buf.push(kind as u8);
    buf.push(0);
    buf.push(0);
}

fn peek(bytes: &[u8], kind: ArtifactKind) -> Result<&[u8], CompileError> {
    if peek_kind(bytes)? != kind {
        return Err(CompileError::Header("wrong kind".into()));
    }
    Ok(&bytes[PREFIX..])
}

/// Kind from a `KLTH` prefix. Does not validate the payload.
pub fn peek_kind(bytes: &[u8]) -> Result<ArtifactKind, CompileError> {
    if bytes.len() < PREFIX {
        return Err(CompileError::Header("truncated prefix".into()));
    }
    if bytes[..4] != MAGIC {
        return Err(CompileError::Header("bad magic".into()));
    }
    if bytes[4] != VERSION {
        return Err(CompileError::Header("bad version".into()));
    }
    ArtifactKind::from_u8(bytes[5]).ok_or_else(|| CompileError::Header("unknown kind".into()))
}

/// Validate a CAS blob by its `KLTH` kind. Mesh/grain/hull/rite/clip before use.
pub fn validate_blob(bytes: &[u8]) -> Result<(), CompileError> {
    match peek_kind(bytes)? {
        ArtifactKind::ClusteredMesh => {
            validate_mesh(bytes)?;
        }
        ArtifactKind::Hull => {
            validate_hull(bytes)?;
        }
        ArtifactKind::Grain => {
            validate_grain(bytes)?;
        }
        ArtifactKind::ClipSet => {
            validate_clipset(bytes)?;
        }
        ArtifactKind::ContactTrack => {
            crate::decode_contact_track(bytes)?;
        }
        ArtifactKind::RiteChunk => {
            validate_rite(bytes)?;
        }
        ArtifactKind::SkinnedMesh => {
            validate_skinned_mesh(bytes)?;
        }
        ArtifactKind::Texture | ArtifactKind::AffordanceGraph | ArtifactKind::Embedding => {}
        ArtifactKind::ProbeGrid => {
            validate_probe_grid(bytes)?;
        }
        ArtifactKind::Evidence => {
            return Err(CompileError::Header(
                "evidence bundle is not a cooked blob".into(),
            ));
        }
    }
    Ok(())
}

fn u32_le(b: &[u8], off: usize) -> Result<u32, CompileError> {
    let s = b
        .get(off..off + 4)
        .ok_or_else(|| CompileError::Header("truncated u32".into()))?;
    Ok(u32::from_le_bytes(s.try_into().expect("4 bytes")))
}

fn i32_le(b: &[u8], off: usize) -> Result<i32, CompileError> {
    let s = b
        .get(off..off + 4)
        .ok_or_else(|| CompileError::Header("truncated i32".into()))?;
    Ok(i32::from_le_bytes(s.try_into().expect("4 bytes")))
}

/// Parsed clustered-mesh header.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct MeshInfo {
    /// Vertex count.
    pub verts: u32,
    /// Index count (must be a multiple of 3).
    pub indices: u32,
}

impl MeshInfo {
    /// Triangle count.
    #[must_use]
    pub const fn tris(self) -> u32 {
        self.indices / 3
    }
}

/// Quantized clustered mesh after header validation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DecodedMesh {
    /// Counts.
    pub info: MeshInfo,
    /// `i16` millimetre verts. Presenters may promote to float.
    pub verts: Vec<[i16; 3]>,
    /// Triangle indices.
    pub indices: Vec<u32>,
}

/// Validate a clustered-mesh blob. Caps: `MAX_TRIS`, quantized `i16` verts.
pub fn validate_mesh(bytes: &[u8]) -> Result<MeshInfo, CompileError> {
    let rest = peek(bytes, ArtifactKind::ClusteredMesh)?;
    let verts = u32_le(rest, 0)?;
    let indices = u32_le(rest, 4)?;
    if indices % 3 != 0 {
        return Err(CompileError::Header("index count not multiple of 3".into()));
    }
    let tris = indices / 3;
    if tris > MAX_TRIS {
        return Err(CompileError::Header(format!("tris {tris} > {MAX_TRIS}")));
    }
    let vert_bytes = (verts as usize)
        .checked_mul(6)
        .ok_or_else(|| CompileError::Header("vert overflow".into()))?;
    let idx_bytes = (indices as usize)
        .checked_mul(4)
        .ok_or_else(|| CompileError::Header("index overflow".into()))?;
    let need = 8usize
        .checked_add(vert_bytes)
        .and_then(|n| n.checked_add(idx_bytes))
        .ok_or_else(|| CompileError::Header("size overflow".into()))?;
    if rest.len() != need {
        return Err(CompileError::Header("payload size mismatch".into()));
    }
    let idx_off = 8 + vert_bytes;
    for i in 0..indices as usize {
        let raw = &rest[idx_off + i * 4..idx_off + i * 4 + 4];
        let ix = u32::from_le_bytes(raw.try_into().expect("4"));
        if ix >= verts {
            return Err(CompileError::Header("index out of range".into()));
        }
    }
    Ok(MeshInfo { verts, indices })
}

/// Validate then copy verts/indices. Call this before GPU upload (PR 12).
pub fn decode_mesh(bytes: &[u8]) -> Result<DecodedMesh, CompileError> {
    let info = validate_mesh(bytes)?;
    let rest = &bytes[PREFIX..];
    let mut verts = Vec::with_capacity(info.verts as usize);
    let mut off = 8usize;
    for _ in 0..info.verts {
        let x = i16::from_le_bytes([rest[off], rest[off + 1]]);
        let y = i16::from_le_bytes([rest[off + 2], rest[off + 3]]);
        let z = i16::from_le_bytes([rest[off + 4], rest[off + 5]]);
        verts.push([x, y, z]);
        off += 6;
    }
    let mut indices = Vec::with_capacity(info.indices as usize);
    for _ in 0..info.indices {
        indices.push(u32::from_le_bytes([
            rest[off],
            rest[off + 1],
            rest[off + 2],
            rest[off + 3],
        ]));
        off += 4;
    }
    Ok(DecodedMesh {
        info,
        verts,
        indices,
    })
}

/// Validate a hull blob. Six `i32` millimetre extents.
pub fn validate_hull(bytes: &[u8]) -> Result<AabbMm, CompileError> {
    let rest = peek(bytes, ArtifactKind::Hull)?;
    if rest.len() != 24 {
        return Err(CompileError::Header("hull payload must be 24 bytes".into()));
    }
    Ok(AabbMm::new(
        IVec3 {
            x: i32_le(rest, 0)?,
            y: i32_le(rest, 4)?,
            z: i32_le(rest, 8)?,
        },
        IVec3 {
            x: i32_le(rest, 12)?,
            y: i32_le(rest, 16)?,
            z: i32_le(rest, 20)?,
        },
    ))
}

/// Parsed grain header.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct GrainInfo {
    /// Sample rate.
    pub hz: u32,
    /// Channel count (v1: 1).
    pub channels: u8,
    /// Frame count.
    pub frames: u32,
}

/// Validate a grain blob. PCM is `i16` LE mono at [`GRAIN_HZ`].
pub fn validate_grain(bytes: &[u8]) -> Result<GrainInfo, CompileError> {
    let rest = peek(bytes, ArtifactKind::Grain)?;
    if rest.len() < 12 {
        return Err(CompileError::Header("truncated grain header".into()));
    }
    let hz = u32_le(rest, 0)?;
    let channels = rest[4];
    let frames = u32_le(rest, 8)?;
    if hz != GRAIN_HZ {
        return Err(CompileError::Header("grain hz".into()));
    }
    if channels != 1 {
        return Err(CompileError::Header("grain channels".into()));
    }
    let pcm = (frames as usize)
        .checked_mul(2)
        .ok_or_else(|| CompileError::Header("pcm overflow".into()))?;
    if rest.len() != 12 + pcm {
        return Err(CompileError::Header("grain payload size".into()));
    }
    Ok(GrainInfo {
        hz,
        channels,
        frames,
    })
}

/// Mono PCM after header validation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DecodedGrain {
    /// Counts and rate.
    pub info: GrainInfo,
    /// Mono `i16` LE PCM at [`GRAIN_HZ`].
    pub pcm: Vec<i16>,
}

/// Validate then copy PCM. Call this before mix (PR 14).
pub fn decode_grain(bytes: &[u8]) -> Result<DecodedGrain, CompileError> {
    let info = validate_grain(bytes)?;
    let rest = &bytes[PREFIX..];
    let mut pcm = Vec::with_capacity(info.frames as usize);
    let mut off = 12usize;
    for _ in 0..info.frames {
        pcm.push(i16::from_le_bytes([rest[off], rest[off + 1]]));
        off += 2;
    }
    Ok(DecodedGrain { info, pcm })
}

/// Validate a rite-chunk blob (header only; ISA is `klotho-commit`).
pub fn validate_rite(bytes: &[u8]) -> Result<(), CompileError> {
    let rest = peek(bytes, ArtifactKind::RiteChunk)?;
    if rest.len() < 8 {
        return Err(CompileError::Header("truncated rite chunk".into()));
    }
    let cap_steps = u16::from_le_bytes([rest[2], rest[3]]);
    if cap_steps == 0 {
        return Err(CompileError::Header("cap_steps 0".into()));
    }
    if cap_steps > MAX_RITE_STEPS {
        return Err(CompileError::Header(format!(
            "cap_steps {cap_steps} > {MAX_RITE_STEPS}"
        )));
    }
    Ok(())
}

/// Bone cap on a SkinnedMesh blob / palette.
pub const MAX_SKIN_BONES: u32 = 256;
/// Vertex cap: `MAX_TRIS * 3` so a length prefix cannot allocate unbounded.
pub const MAX_SKIN_VERTS: u32 = MAX_TRIS.saturating_mul(3);
/// Quantized 4-influence weights must sum to this (`u16` x4).
pub const SKIN_WEIGHT_SUM: u32 = 65_535;

/// Parsed skinned-mesh header.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct SkinnedMeshInfo {
    /// Vertex count.
    pub verts: u32,
    /// Index count (must be a multiple of 3).
    pub indices: u32,
    /// Bone count. 0 = identity / rigid fallback.
    pub bones: u32,
}

/// Quantized skinned mesh after header validation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DecodedSkinnedMesh {
    /// Counts.
    pub info: SkinnedMeshInfo,
    /// `i16` millimetre verts. Presenters may promote to float.
    pub verts: Vec<[i16; 3]>,
    /// Four joint indices per vert.
    pub joints: Vec<[u8; 4]>,
    /// Four `u16` weights per vert. Each vertex sums to [`SKIN_WEIGHT_SUM`].
    pub weights: Vec<[u16; 4]>,
    /// Triangle indices.
    pub indices: Vec<u32>,
}

/// Encode a skinned mesh. Lengths of `joints` / `weights` must match `verts`.
pub fn encode_skinned_mesh(
    verts: &[[i16; 3]],
    joints: &[[u8; 4]],
    weights: &[[u16; 4]],
    indices: &[u32],
    bones: u32,
) -> Result<Vec<u8>, CompileError> {
    if joints.len() != verts.len() || weights.len() != verts.len() {
        return Err(CompileError::Header("weight count mismatch".into()));
    }
    if verts.len() > MAX_SKIN_VERTS as usize {
        return Err(CompileError::Header(format!(
            "verts {} > {MAX_SKIN_VERTS}",
            verts.len()
        )));
    }
    if indices.len() % 3 != 0 {
        return Err(CompileError::Header("index count not multiple of 3".into()));
    }
    let tris = (indices.len() / 3) as u32;
    if tris > MAX_TRIS {
        return Err(CompileError::Header(format!("tris {tris} > {MAX_TRIS}")));
    }
    if bones > MAX_SKIN_BONES {
        return Err(CompileError::Header(format!(
            "bones {bones} > {MAX_SKIN_BONES}"
        )));
    }
    let mut b = Vec::new();
    write_prefix(&mut b, ArtifactKind::SkinnedMesh);
    b.extend_from_slice(&(verts.len() as u32).to_le_bytes());
    b.extend_from_slice(&(indices.len() as u32).to_le_bytes());
    b.extend_from_slice(&bones.to_le_bytes());
    for v in verts {
        b.extend_from_slice(&v[0].to_le_bytes());
        b.extend_from_slice(&v[1].to_le_bytes());
        b.extend_from_slice(&v[2].to_le_bytes());
    }
    for j in joints {
        b.extend_from_slice(j);
    }
    for w in weights {
        for c in w {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    for i in indices {
        b.extend_from_slice(&i.to_le_bytes());
    }
    Ok(b)
}

/// Validate a skinned-mesh blob. Caps: [`MAX_TRIS`], [`MAX_SKIN_BONES`].
pub fn validate_skinned_mesh(bytes: &[u8]) -> Result<SkinnedMeshInfo, CompileError> {
    decode_skinned_mesh(bytes).map(|d| d.info)
}

/// Validate then copy verts/joints/weights/indices. Call before GPU upload.
pub fn decode_skinned_mesh(bytes: &[u8]) -> Result<DecodedSkinnedMesh, CompileError> {
    let rest = peek(bytes, ArtifactKind::SkinnedMesh)?;
    if rest.len() < 12 {
        return Err(CompileError::Header("truncated skinned header".into()));
    }
    let verts = u32_le(rest, 0)?;
    let indices = u32_le(rest, 4)?;
    let bones = u32_le(rest, 8)?;
    if indices % 3 != 0 {
        return Err(CompileError::Header("index count not multiple of 3".into()));
    }
    let tris = indices / 3;
    if tris > MAX_TRIS {
        return Err(CompileError::Header(format!("tris {tris} > {MAX_TRIS}")));
    }
    if bones > MAX_SKIN_BONES {
        return Err(CompileError::Header(format!(
            "bones {bones} > {MAX_SKIN_BONES}"
        )));
    }
    if verts > MAX_SKIN_VERTS {
        return Err(CompileError::Header(format!(
            "verts {verts} > {MAX_SKIN_VERTS}"
        )));
    }
    let nv = verts as usize;
    let ni = indices as usize;
    let vert_bytes = nv
        .checked_mul(6)
        .ok_or_else(|| CompileError::Header("vert overflow".into()))?;
    let joint_bytes = nv
        .checked_mul(4)
        .ok_or_else(|| CompileError::Header("joint overflow".into()))?;
    let weight_bytes = nv
        .checked_mul(8)
        .ok_or_else(|| CompileError::Header("weight overflow".into()))?;
    let idx_bytes = ni
        .checked_mul(4)
        .ok_or_else(|| CompileError::Header("index overflow".into()))?;
    let need = 12usize
        .checked_add(vert_bytes)
        .and_then(|n| n.checked_add(joint_bytes))
        .and_then(|n| n.checked_add(weight_bytes))
        .and_then(|n| n.checked_add(idx_bytes))
        .ok_or_else(|| CompileError::Header("size overflow".into()))?;
    if rest.len() != need {
        return Err(CompileError::Header("payload size mismatch".into()));
    }
    let mut off = 12usize;
    let mut out_verts = Vec::with_capacity(nv);
    for _ in 0..nv {
        let x = i16::from_le_bytes([rest[off], rest[off + 1]]);
        let y = i16::from_le_bytes([rest[off + 2], rest[off + 3]]);
        let z = i16::from_le_bytes([rest[off + 4], rest[off + 5]]);
        out_verts.push([x, y, z]);
        off += 6;
    }
    let mut out_joints = Vec::with_capacity(nv);
    for _ in 0..nv {
        let j = [rest[off], rest[off + 1], rest[off + 2], rest[off + 3]];
        for &b in &j {
            if u32::from(b) >= bones && !(bones == 0 && b == 0) {
                return Err(CompileError::Header("bone index out of range".into()));
            }
        }
        out_joints.push(j);
        off += 4;
    }
    let mut out_weights = Vec::with_capacity(nv);
    for _ in 0..nv {
        let w = [
            u16::from_le_bytes([rest[off], rest[off + 1]]),
            u16::from_le_bytes([rest[off + 2], rest[off + 3]]),
            u16::from_le_bytes([rest[off + 4], rest[off + 5]]),
            u16::from_le_bytes([rest[off + 6], rest[off + 7]]),
        ];
        let sum = u32::from(w[0]) + u32::from(w[1]) + u32::from(w[2]) + u32::from(w[3]);
        if sum != SKIN_WEIGHT_SUM {
            return Err(CompileError::Header("weight sum".into()));
        }
        out_weights.push(w);
        off += 8;
    }
    let mut out_idx = Vec::with_capacity(ni);
    for _ in 0..ni {
        let ix = u32::from_le_bytes([rest[off], rest[off + 1], rest[off + 2], rest[off + 3]]);
        if ix >= verts {
            return Err(CompileError::Header("index out of range".into()));
        }
        out_idx.push(ix);
        off += 4;
    }
    Ok(DecodedSkinnedMesh {
        info: SkinnedMeshInfo {
            verts,
            indices,
            bones,
        },
        verts: out_verts,
        joints: out_joints,
        weights: out_weights,
        indices: out_idx,
    })
}

/// v1 cap: clips in one ClipSet.
pub const MAX_CLIPS: u16 = 64;
/// v1 cap: samples per clip.
pub const MAX_CLIP_SAMPLES: u16 = 256;

/// Parsed ClipSet header.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ClipSetInfo {
    /// Clip count.
    pub clips: u16,
}

/// One decoded clip after header validation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DecodedClip {
    /// [`klotho_ir::Verb`] discriminant.
    pub verb: u8,
    /// Grounded selector.
    pub grounded: bool,
    /// Loop vs one-shot.
    pub looping: bool,
    /// Table index.
    pub id: u16,
    /// Per-tick root, millimetres, clip-local.
    pub samples: Vec<IVec3>,
}

/// Validate a ClipSet blob. Caps: [`MAX_CLIPS`], [`MAX_CLIP_SAMPLES`].
pub fn validate_clipset(bytes: &[u8]) -> Result<ClipSetInfo, CompileError> {
    decode_clipset(bytes).map(|c| ClipSetInfo {
        clips: u16::try_from(c.len()).unwrap_or(u16::MAX),
    })
}

/// Validate then copy clips. Call this before Motion upload / CAS bind.
pub fn decode_clipset(bytes: &[u8]) -> Result<Vec<DecodedClip>, CompileError> {
    let rest = peek(bytes, ArtifactKind::ClipSet)?;
    if rest.len() < 2 {
        return Err(CompileError::Header("truncated clipset".into()));
    }
    let n = u16::from_le_bytes([rest[0], rest[1]]);
    if n > MAX_CLIPS {
        return Err(CompileError::Header(format!("clips {n} > {MAX_CLIPS}")));
    }
    let mut off = 2usize;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let verb = *rest
            .get(off)
            .ok_or_else(|| CompileError::Header("truncated clip verb".into()))?;
        let grounded = *rest
            .get(off + 1)
            .ok_or_else(|| CompileError::Header("truncated clip grounded".into()))?;
        let looping = *rest
            .get(off + 2)
            .ok_or_else(|| CompileError::Header("truncated clip looping".into()))?;
        let id = u16::from_le_bytes(
            rest.get(off + 4..off + 6)
                .ok_or_else(|| CompileError::Header("truncated clip id".into()))?
                .try_into()
                .expect("2"),
        );
        let ns = u16::from_le_bytes(
            rest.get(off + 6..off + 8)
                .ok_or_else(|| CompileError::Header("truncated clip samples".into()))?
                .try_into()
                .expect("2"),
        );
        if ns > MAX_CLIP_SAMPLES {
            return Err(CompileError::Header(format!(
                "samples {ns} > {MAX_CLIP_SAMPLES}"
            )));
        }
        off += 8;
        let mut samples = Vec::with_capacity(ns as usize);
        for _ in 0..ns {
            let x = i32_le(rest, off)?;
            let y = i32_le(rest, off + 4)?;
            let z = i32_le(rest, off + 8)?;
            samples.push(IVec3 { x, y, z });
            off += 12;
        }
        out.push(DecodedClip {
            verb,
            grounded: grounded != 0,
            looping: looping != 0,
            id,
            samples,
        });
    }
    if off != rest.len() {
        return Err(CompileError::Header("clipset payload size".into()));
    }
    Ok(out)
}

/// Maximum probe cells in a baked volume (8×4×8 Era-2 cap).
pub const MAX_PROBE_CELLS: u32 = 8 * 4 * 8;

/// Parsed probe-grid header.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ProbeGridInfo {
    /// Origin, millimetres.
    pub origin: IVec3,
    /// Cell size, millimetres.
    pub spacing_mm: i32,
    /// Cell counts along X, Y, Z.
    pub dim: (u8, u8, u8),
}

/// Integer irradiance samples after header validation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DecodedProbeGrid {
    /// Counts and transform.
    pub info: ProbeGridInfo,
    /// RGB milli-irradiance, `len == cells * 3`.
    pub samples_milli: Vec<u16>,
}

fn probe_cell_count(dim: (u8, u8, u8)) -> Result<u32, CompileError> {
    let cells = u32::from(dim.0)
        .saturating_mul(u32::from(dim.1))
        .saturating_mul(u32::from(dim.2));
    if cells == 0 || cells > MAX_PROBE_CELLS {
        return Err(CompileError::Header("probe cell count".into()));
    }
    Ok(cells)
}

/// Validate a baked probe-grid blob.
pub fn validate_probe_grid(bytes: &[u8]) -> Result<ProbeGridInfo, CompileError> {
    let rest = peek(bytes, ArtifactKind::ProbeGrid)?;
    if rest.len() < 20 {
        return Err(CompileError::Header("truncated probe grid".into()));
    }
    let origin = IVec3 {
        x: i32_le(rest, 0)?,
        y: i32_le(rest, 4)?,
        z: i32_le(rest, 8)?,
    };
    let spacing_mm = i32_le(rest, 12)?;
    if spacing_mm <= 0 {
        return Err(CompileError::Header("probe spacing".into()));
    }
    let dim = (rest[16], rest[17], rest[18]);
    let cells = probe_cell_count(dim)?;
    let need = 20usize
        .checked_add((cells as usize).saturating_mul(6))
        .ok_or_else(|| CompileError::Header("probe size overflow".into()))?;
    if rest.len() != need {
        return Err(CompileError::Header("probe payload size".into()));
    }
    Ok(ProbeGridInfo {
        origin,
        spacing_mm,
        dim,
    })
}

/// Validate then copy milli-irradiance samples.
pub fn decode_probe_grid(bytes: &[u8]) -> Result<DecodedProbeGrid, CompileError> {
    let info = validate_probe_grid(bytes)?;
    let rest = &bytes[PREFIX..];
    let cells = probe_cell_count(info.dim)? as usize;
    let mut samples_milli = Vec::with_capacity(cells * 3);
    let mut off = 20usize;
    for _ in 0..cells * 3 {
        samples_milli.push(u16::from_le_bytes([rest[off], rest[off + 1]]));
        off += 2;
    }
    Ok(DecodedProbeGrid {
        info,
        samples_milli,
    })
}

/// Encode a baked probe grid. Samples must be `cells * 3` milli-RGB.
pub fn encode_probe_grid(
    origin: IVec3,
    spacing_mm: i32,
    dim: (u8, u8, u8),
    samples_milli: &[u16],
) -> Result<Vec<u8>, CompileError> {
    if spacing_mm <= 0 {
        return Err(CompileError::Header("probe spacing".into()));
    }
    let cells = probe_cell_count(dim)? as usize;
    if samples_milli.len() != cells * 3 {
        return Err(CompileError::Header("probe sample count".into()));
    }
    let mut b = Vec::with_capacity(PREFIX + 20 + cells * 6);
    write_prefix(&mut b, ArtifactKind::ProbeGrid);
    for v in [origin.x, origin.y, origin.z, spacing_mm] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.push(dim.0);
    b.push(dim.1);
    b.push(dim.2);
    b.push(0);
    for s in samples_milli {
        b.extend_from_slice(&s.to_le_bytes());
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_magic_is_header_error() {
        let e = validate_mesh(b"XXXX").unwrap_err();
        assert!(matches!(e, CompileError::Header(_)));
    }

    #[test]
    fn max_tris_is_hld() {
        assert_eq!(MAX_TRIS, 200_000);
        assert_eq!(MAX_RITE_STEPS, 64);
        assert_eq!(MAGIC, *b"KLTH");
    }

    #[test]
    fn probe_grid_round_trips() {
        let origin = IVec3 { x: 1, y: 2, z: 3 };
        let samples = vec![10u16, 20, 30, 40, 50, 60];
        let bytes = encode_probe_grid(origin, 2_000, (2, 1, 1), &samples).unwrap();
        assert_eq!(peek_kind(&bytes).unwrap(), ArtifactKind::ProbeGrid);
        validate_blob(&bytes).unwrap();
        let decoded = decode_probe_grid(&bytes).unwrap();
        assert_eq!(decoded.info.origin, origin);
        assert_eq!(decoded.samples_milli, samples);
    }

    #[test]
    fn rite_cap_steps_rejected() {
        let mut b = Vec::new();
        write_prefix(&mut b, ArtifactKind::RiteChunk);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&(MAX_RITE_STEPS.saturating_add(1)).to_le_bytes());
        b.extend_from_slice(&8u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        let e = validate_rite(&b).unwrap_err();
        assert!(
            matches!(e, CompileError::Header(ref s) if s.contains("cap_steps") && s.contains('>')),
            "{e}"
        );
        assert!(matches!(validate_blob(&b), Err(CompileError::Header(_))));
    }

    #[test]
    fn rite_cap_steps_zero_is_distinct() {
        let mut b = Vec::new();
        write_prefix(&mut b, ArtifactKind::RiteChunk);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&8u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        let e = validate_rite(&b).unwrap_err();
        assert!(
            matches!(e, CompileError::Header(ref s) if s == "cap_steps 0"),
            "{e}"
        );
    }

    #[test]
    fn oversize_tris_rejected_before_payload() {
        let mut b = Vec::new();
        write_prefix(&mut b, ArtifactKind::ClusteredMesh);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&(MAX_TRIS.saturating_add(1).saturating_mul(3)).to_le_bytes());
        let e = validate_mesh(&b).unwrap_err();
        assert!(matches!(e, CompileError::Header(s) if s.contains("tris")));
    }

    fn grain_blob(hz: u32, channels: u8, pcm: &[i16]) -> Vec<u8> {
        let mut b = Vec::new();
        write_prefix(&mut b, ArtifactKind::Grain);
        b.extend_from_slice(&hz.to_le_bytes());
        b.push(channels);
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        for s in pcm {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    #[test]
    fn decode_grain_copies_pcm_after_validate() {
        let pcm = [1i16, -2, 3, 0];
        let b = grain_blob(GRAIN_HZ, 1, &pcm);
        let d = decode_grain(&b).unwrap();
        assert_eq!(d.info.hz, GRAIN_HZ);
        assert_eq!(d.info.channels, 1);
        assert_eq!(d.info.frames, 4);
        assert_eq!(d.pcm, pcm);
    }

    #[test]
    fn decode_grain_rejects_bad_magic_hz_channels_truncated() {
        assert!(matches!(
            decode_grain(b"XXXX"),
            Err(CompileError::Header(_))
        ));
        let bad_hz = grain_blob(44_100, 1, &[1]);
        assert!(matches!(
            decode_grain(&bad_hz),
            Err(CompileError::Header(s)) if s.contains("hz")
        ));
        let bad_ch = grain_blob(GRAIN_HZ, 2, &[1, 2]);
        assert!(matches!(
            decode_grain(&bad_ch),
            Err(CompileError::Header(s)) if s.contains("channels")
        ));
        let mut trunc = grain_blob(GRAIN_HZ, 1, &[1, 2, 3]);
        trunc.pop();
        assert!(matches!(decode_grain(&trunc), Err(CompileError::Header(_))));
    }

    fn tri_skinned(bones: u32, joints: [u8; 4], weights: [u16; 4]) -> Vec<u8> {
        encode_skinned_mesh(
            &[[0, 0, 0], [100, 0, 0], [0, 100, 0]],
            &[joints, joints, joints],
            &[weights, weights, weights],
            &[0, 1, 2],
            bones,
        )
        .unwrap()
    }

    #[test]
    fn skinned_mesh_roundtrip() {
        let w = [SKIN_WEIGHT_SUM as u16, 0, 0, 0];
        let b = tri_skinned(2, [0, 1, 0, 0], w);
        assert_eq!(&b[..4], b"KLTH");
        assert_eq!(b[5], ArtifactKind::SkinnedMesh as u8);
        let info = validate_skinned_mesh(&b).unwrap();
        assert_eq!(info.verts, 3);
        assert_eq!(info.indices, 3);
        assert_eq!(info.bones, 2);
        let d = decode_skinned_mesh(&b).unwrap();
        assert_eq!(d.verts[1], [100, 0, 0]);
        assert_eq!(d.joints[0], [0, 1, 0, 0]);
        assert_eq!(d.weights[0], w);
        assert!(validate_blob(&b).is_ok());
    }

    #[test]
    fn skinned_bad_magic_is_header_error() {
        assert!(matches!(
            validate_skinned_mesh(b"XXXX"),
            Err(CompileError::Header(_))
        ));
    }

    #[test]
    fn skinned_bone_index_oob_rejected() {
        let w = [SKIN_WEIGHT_SUM as u16, 0, 0, 0];
        let e = encode_skinned_mesh(
            &[[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            &[[2, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]],
            &[w, w, w],
            &[0, 1, 2],
            2,
        )
        .unwrap();
        let err = validate_skinned_mesh(&e).unwrap_err();
        assert!(
            matches!(err, CompileError::Header(ref s) if s.contains("bone")),
            "{err}"
        );
    }

    #[test]
    fn skinned_weight_count_mismatch_rejected() {
        let e = encode_skinned_mesh(
            &[[0, 0, 0]],
            &[[0, 0, 0, 0], [0, 0, 0, 0]],
            &[[SKIN_WEIGHT_SUM as u16, 0, 0, 0]],
            &[0, 0, 0],
            1,
        )
        .unwrap_err();
        assert!(
            matches!(e, CompileError::Header(ref s) if s.contains("weight count mismatch")),
            "{e}"
        );
    }

    #[test]
    fn skinned_weight_sum_rejected() {
        let w = [1u16, 0, 0, 0];
        let bytes = encode_skinned_mesh(
            &[[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            &[[0; 4], [0; 4], [0; 4]],
            &[w, w, w],
            &[0, 1, 2],
            1,
        )
        .unwrap();
        let err = validate_skinned_mesh(&bytes).unwrap_err();
        assert!(
            matches!(err, CompileError::Header(ref s) if s.contains("weight sum")),
            "{err}"
        );
    }

    #[test]
    fn skinned_over_cap_rejected() {
        let mut b = Vec::new();
        write_prefix(&mut b, ArtifactKind::SkinnedMesh);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&(MAX_TRIS.saturating_add(1).saturating_mul(3)).to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        let e = validate_skinned_mesh(&b).unwrap_err();
        assert!(matches!(e, CompileError::Header(s) if s.contains("tris")));

        let mut b = Vec::new();
        write_prefix(&mut b, ArtifactKind::SkinnedMesh);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&(MAX_SKIN_BONES.saturating_add(1)).to_le_bytes());
        let e = validate_skinned_mesh(&b).unwrap_err();
        assert!(matches!(e, CompileError::Header(s) if s.contains("bones")));
    }

    #[test]
    fn skinned_truncated_and_index_oob() {
        let w = [SKIN_WEIGHT_SUM as u16, 0, 0, 0];
        let mut b = tri_skinned(1, [0; 4], w);
        b.pop();
        assert!(matches!(
            validate_skinned_mesh(&b),
            Err(CompileError::Header(_))
        ));
        let bytes = encode_skinned_mesh(
            &[[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            &[[0; 4], [0; 4], [0; 4]],
            &[w, w, w],
            &[0, 1, 3],
            1,
        )
        .unwrap();
        let err = validate_skinned_mesh(&bytes).unwrap_err();
        assert!(
            matches!(err, CompileError::Header(ref s) if s.contains("index out of range")),
            "{err}"
        );
    }

    #[test]
    fn skinned_bones_zero_allows_joint_zero() {
        let w = [SKIN_WEIGHT_SUM as u16, 0, 0, 0];
        let b = tri_skinned(0, [0; 4], w);
        assert!(validate_skinned_mesh(&b).is_ok());
        let bad = encode_skinned_mesh(
            &[[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            &[[1, 0, 0, 0], [0; 4], [0; 4]],
            &[w, w, w],
            &[0, 1, 2],
            0,
        )
        .unwrap();
        assert!(matches!(
            validate_skinned_mesh(&bad),
            Err(CompileError::Header(s)) if s.contains("bone")
        ));
    }
}
