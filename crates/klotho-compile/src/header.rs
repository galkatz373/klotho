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
    if bytes.len() < PREFIX {
        return Err(CompileError::Header("truncated prefix".into()));
    }
    if bytes[..4] != MAGIC {
        return Err(CompileError::Header("bad magic".into()));
    }
    if bytes[4] != VERSION {
        return Err(CompileError::Header("bad version".into()));
    }
    if bytes[5] != kind as u8 {
        return Err(CompileError::Header("wrong kind".into()));
    }
    Ok(&bytes[PREFIX..])
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
    Ok(())
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
        assert_eq!(MAGIC, *b"KLTH");
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
}
