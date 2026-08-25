//! Quantized little-endian blobs. No host floats in hashed bytes.

use klotho_canon::RiteChunk;
use klotho_core::{AabbMm, IVec3};
use klotho_ir::to_ron;
use klotho_prove::ArtifactKind;

use crate::error::CompileError;
use crate::header::{DecodedClip, GRAIN_HZ, MAX_CLIP_SAMPLES, MAX_CLIPS, write_prefix};
use crate::kit::{GrainKind, MeshRecipe};

/// Quantize a millimetre extent to `i16`. Overflow is a cook error, not wrap.
fn q(v: i32) -> Result<i16, CompileError> {
    i16::try_from(v).map_err(|_| CompileError::QuantizeOverflow)
}

/// Hull blob: prefix + six `i32` LE millimetres.
pub(crate) fn encode_hull(aabb: AabbMm) -> Vec<u8> {
    let mut b = Vec::with_capacity(8 + 24);
    write_prefix(&mut b, ArtifactKind::Hull);
    for v in [
        aabb.min.x, aabb.min.y, aabb.min.z, aabb.max.x, aabb.max.y, aabb.max.z,
    ] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// Local AABB for a kitbash box/cylinder standing on y=0.
pub(crate) fn hull_for(hx: i32, hy: i32, hz: i32) -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -hx,
            y: 0,
            z: -hz,
        },
        IVec3 {
            x: hx,
            y: hy,
            z: hz,
        },
    )
}

/// Clustered mesh: `i16` millimetre verts, `u32` indices, LE.
pub(crate) fn encode_mesh(recipe: &MeshRecipe) -> Result<Vec<u8>, CompileError> {
    let (verts, indices) = match *recipe {
        MeshRecipe::Box { hx, hy, hz } => box_mesh(hx, hy, hz)?,
        MeshRecipe::Cylinder {
            radius,
            hy,
            segs: _,
        } => cylinder_mesh(radius, hy)?,
    };
    let mut b = Vec::new();
    write_prefix(&mut b, ArtifactKind::ClusteredMesh);
    b.extend_from_slice(&(verts.len() as u32).to_le_bytes());
    b.extend_from_slice(&(indices.len() as u32).to_le_bytes());
    for v in &verts {
        b.extend_from_slice(&v[0].to_le_bytes());
        b.extend_from_slice(&v[1].to_le_bytes());
        b.extend_from_slice(&v[2].to_le_bytes());
    }
    for i in indices {
        b.extend_from_slice(&i.to_le_bytes());
    }
    Ok(b)
}

fn box_mesh(hx: i32, hy: i32, hz: i32) -> Result<(Vec<[i16; 3]>, Vec<u32>), CompileError> {
    let hx = q(hx)?;
    let hy = q(hy)?;
    let hz = q(hz)?;
    let verts = vec![
        [-hx, 0, -hz],
        [hx, 0, -hz],
        [hx, 0, hz],
        [-hx, 0, hz],
        [-hx, hy, -hz],
        [hx, hy, -hz],
        [hx, hy, hz],
        [-hx, hy, hz],
    ];
    let indices = vec![
        4, 5, 6, 4, 6, 7, // +Y
        0, 3, 2, 0, 2, 1, // -Y
        3, 7, 6, 3, 6, 2, // +Z
        1, 5, 4, 1, 4, 0, // -Z
        1, 2, 6, 1, 6, 5, // +X
        0, 4, 7, 0, 7, 3, // -X
    ];
    Ok((verts, indices))
}

/// 8-seg cylinder. Cos/sin milli-table — no `f32` on the hashed path.
const COS8: [i32; 8] = [1000, 707, 0, -707, -1000, -707, 0, 707];
const SIN8: [i32; 8] = [0, 707, 1000, 707, 0, -707, -1000, -707];

fn cylinder_mesh(radius: i32, hy: i32) -> Result<(Vec<[i16; 3]>, Vec<u32>), CompileError> {
    let hy = q(hy)?;
    let mut verts = Vec::with_capacity(18);
    verts.push([0, 0, 0]);
    for i in 0..8 {
        verts.push([q(radius * COS8[i] / 1000)?, 0, q(radius * SIN8[i] / 1000)?]);
    }
    verts.push([0, hy, 0]);
    for i in 0..8 {
        verts.push([q(radius * COS8[i] / 1000)?, hy, q(radius * SIN8[i] / 1000)?]);
    }
    let mut indices = Vec::new();
    for i in 0..8u32 {
        let b0 = 1 + i;
        let b1 = 1 + (i + 1) % 8;
        let t0 = 10 + i;
        let t1 = 10 + (i + 1) % 8;
        indices.extend_from_slice(&[b0, b1, t1, b0, t1, t0]);
        indices.extend_from_slice(&[0, b1, b0]);
        indices.extend_from_slice(&[9, t0, t1]);
    }
    Ok((verts, indices))
}

/// Integer PCM grain. No host floats.
pub(crate) fn encode_grain(kind: GrainKind) -> Vec<u8> {
    let pcm: Vec<i16> = match kind {
        GrainKind::Knock => {
            let n = 4800usize;
            (0..n)
                .map(|i| {
                    let env = ((n - i) as i32 * 12_000) / n as i32;
                    if i % 2 == 0 {
                        env as i16
                    } else {
                        -(env as i16)
                    }
                })
                .collect()
        }
        GrainKind::Crackle => {
            let n = 2400usize;
            let mut s = 0x9E37_79B9u32;
            (0..n)
                .map(|i| {
                    s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let nse = (s >> 16) as i16;
                    let env = ((n - i) as i32 * 8_000) / n as i32;
                    ((nse as i32 * env) / 32_768) as i16
                })
                .collect()
        }
        GrainKind::Bed => {
            let period = 436i32;
            (0..GRAIN_HZ as usize)
                .map(|i| {
                    let v = ((i as i32 % period) - period / 2) * 20;
                    v.clamp(-4_000, 4_000) as i16
                })
                .collect()
        }
    };
    let mut b = Vec::with_capacity(8 + 12 + pcm.len() * 2);
    write_prefix(&mut b, ArtifactKind::Grain);
    b.extend_from_slice(&GRAIN_HZ.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&[0, 0, 0]);
    b.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    for s in pcm {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// Rite bytecode blob. Ops are canonical RON so the ISA stays in `klotho-ir`.
pub(crate) fn encode_rite(chunk: &RiteChunk) -> Result<Vec<u8>, CompileError> {
    let mut b = Vec::new();
    write_prefix(&mut b, ArtifactKind::RiteChunk);
    b.extend_from_slice(&chunk.entry.to_le_bytes());
    b.extend_from_slice(&chunk.cap_steps.to_le_bytes());
    b.extend_from_slice(&chunk.cap_ticks.to_le_bytes());
    b.extend_from_slice(&(chunk.instrs.len() as u16).to_le_bytes());
    for instr in &chunk.instrs {
        b.extend_from_slice(&instr.pc.to_le_bytes());
        let ron = to_ron(&instr.op).map_err(|e| CompileError::Catalog(e.to_string()))?;
        let bytes = ron.as_bytes();
        b.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        b.extend_from_slice(bytes);
    }
    Ok(b)
}

/// ClipSet blob: prefix + `u16` count + packed clips. Integer millimetre samples.
pub(crate) fn encode_clipset(clips: &[DecodedClip]) -> Result<Vec<u8>, CompileError> {
    if clips.len() > MAX_CLIPS as usize {
        return Err(CompileError::Header(format!(
            "clips {} > {MAX_CLIPS}",
            clips.len()
        )));
    }
    let mut b = Vec::new();
    write_prefix(&mut b, ArtifactKind::ClipSet);
    b.extend_from_slice(&(clips.len() as u16).to_le_bytes());
    for c in clips {
        if c.samples.len() > MAX_CLIP_SAMPLES as usize {
            return Err(CompileError::Header(format!(
                "samples {} > {MAX_CLIP_SAMPLES}",
                c.samples.len()
            )));
        }
        b.push(c.verb);
        b.push(u8::from(c.grounded));
        b.push(u8::from(c.looping));
        b.push(0);
        b.extend_from_slice(&c.id.to_le_bytes());
        b.extend_from_slice(&(c.samples.len() as u16).to_le_bytes());
        for s in &c.samples {
            b.extend_from_slice(&s.x.to_le_bytes());
            b.extend_from_slice(&s.y.to_le_bytes());
            b.extend_from_slice(&s.z.to_le_bytes());
        }
    }
    Ok(b)
}

/// Hearth biped T-pose idle + +Z walk. Same numbers as `klotho-motion::ClipSet::hearth`.
pub(crate) fn hearth_biped_clips() -> Vec<DecodedClip> {
    use klotho_ir::Verb;
    vec![
        DecodedClip {
            verb: Verb::Look.as_u8(),
            grounded: true,
            looping: true,
            id: 0,
            samples: vec![IVec3::ZERO],
        },
        DecodedClip {
            verb: Verb::Move.as_u8(),
            grounded: true,
            looping: true,
            id: 1,
            samples: vec![IVec3 { x: 0, y: 0, z: 20 }],
        },
        DecodedClip {
            verb: Verb::Use.as_u8(),
            grounded: true,
            looping: false,
            id: 2,
            samples: vec![IVec3::ZERO],
        },
    ]
}

#[cfg(test)]
mod tests {
    use klotho_prove::hash_bytes;

    use super::*;
    use crate::header::{
        decode_clipset, decode_grain, decode_mesh, validate_clipset, validate_grain, validate_hull,
        validate_mesh,
    };

    #[test]
    fn box_mesh_is_i16_le_and_validates() {
        let bytes = encode_mesh(&MeshRecipe::Box {
            hx: 100,
            hy: 200,
            hz: 50,
        })
        .unwrap();
        assert_eq!(&bytes[..4], b"KLTH");
        assert_eq!(bytes[4], 1);
        assert_eq!(bytes[5], ArtifactKind::ClusteredMesh as u8);
        let info = validate_mesh(&bytes).unwrap();
        assert_eq!(info.verts, 8);
        assert_eq!(info.tris(), 12);
        // First vert x = -100i16 LE at offset 16.
        assert_eq!(&bytes[16..18], &(-100i16).to_le_bytes());
        let decoded = decode_mesh(&bytes).unwrap();
        assert_eq!(decoded.verts[0], [-100, 0, -50]);
        assert_eq!(decoded.indices.len(), 36);
        assert_eq!(
            hash_bytes(&bytes).to_string(),
            "4e7855dd1e522cf6b80781aa577a6a0c6a0cc0fe0e94d4e39509e649073fa4c7"
        );
    }

    #[test]
    fn hull_and_grain_validate() {
        let h = encode_hull(hull_for(400, 2000, 50));
        let aabb = validate_hull(&h).unwrap();
        assert_eq!(aabb.min.x, -400);
        assert_eq!(aabb.max.y, 2000);
        let g = encode_grain(GrainKind::Knock);
        let info = validate_grain(&g).unwrap();
        assert_eq!(info.hz, GRAIN_HZ);
        assert_eq!(info.frames, 4800);
        assert_eq!(info.channels, 1);
        let decoded = decode_grain(&g).unwrap();
        assert_eq!(decoded.pcm.len(), 4800);
        assert_eq!(decoded.pcm[0], 12_000);
    }

    #[test]
    fn cylinder_has_no_host_float() {
        let bytes = encode_mesh(&MeshRecipe::Cylinder {
            radius: 300,
            hy: 900,
            segs: 8,
        })
        .unwrap();
        let info = validate_mesh(&bytes).unwrap();
        assert_eq!(info.verts, 18);
    }

    #[test]
    fn quantize_overflow_is_cook_error() {
        let e = encode_mesh(&MeshRecipe::Box {
            hx: 40_000,
            hy: 1,
            hz: 1,
        })
        .unwrap_err();
        assert_eq!(e, CompileError::QuantizeOverflow);
    }

    #[test]
    fn clipset_roundtrip_and_caps() {
        let bytes = encode_clipset(&hearth_biped_clips()).unwrap();
        assert_eq!(&bytes[..4], b"KLTH");
        assert_eq!(bytes[5], ArtifactKind::ClipSet as u8);
        let info = validate_clipset(&bytes).unwrap();
        assert_eq!(info.clips, 3);
        let decoded = decode_clipset(&bytes).unwrap();
        assert_eq!(decoded[1].samples[0].z, 20);
        let mut over = hearth_biped_clips();
        over.extend(std::iter::repeat_n(
            DecodedClip {
                verb: 0,
                grounded: true,
                looping: true,
                id: 9,
                samples: vec![IVec3::ZERO],
            },
            MAX_CLIPS as usize,
        ));
        let e = encode_clipset(&over).unwrap_err();
        assert!(matches!(e, CompileError::Header(s) if s.contains("clips")));
    }
}
