//! `.warp` container: little-endian sections. Not `KLTH` (CAS blobs use that).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use klotho_canon::cook as cook_canon;
use klotho_core::{BlobId, Hash};
use klotho_ir::{IntentDoc, Name, SeedFact, from_ron, to_ron};
use klotho_manifest::MaterialTag;
use klotho_prove::{
    Activity, Agent, ArtifactKind, Cas, LicenseSpan, MAX_BLOB_BYTES, MAX_BLOBS, ProveError,
    ProvenanceDag, ProvenanceId, ProvenanceKind, ProvenanceNode, blob_id_of,
};

use crate::Binding;
use crate::cook::{COMPILER_VERSION, Cooked, cook_digest, optimized_cook_digest};
use crate::error::CompileError;
use crate::header::{peek_kind, validate_blob};

/// Container magic. Distinct from CAS `KLTH` so a blob is not a warp.
pub const WARP_MAGIC: [u8; 4] = *b"KWRP";
/// Container version. Bump invalidates every file.
pub const WARP_VERSION: u8 = 3;
/// HLD §4 desktop file cap. Loader applies this via `read_capped` before parse.
pub const WARP_CAP_DESKTOP: usize = 512 * 1024 * 1024;
/// HLD §4 mobile file cap (named; v1 loader uses desktop).
pub const WARP_CAP_MOBILE: usize = 192 * 1024 * 1024;
/// HLD §4 seed locus cap.
pub const WARP_MAX_LOCI: usize = 4_096;

/// Pack `cooked` to `.warp` bytes. Fails on Unknown license, bad headers, or oversize.
pub fn pack_warp(cooked: &Cooked) -> Result<Vec<u8>, CompileError> {
    cooked.dag.exportable().map_err(CompileError::prove)?;
    check_seed_loci(&cooked.doc)?;
    if cooked.cas.len() > MAX_BLOBS {
        return Err(CompileError::prove(ProveError::CasFull));
    }
    for (_, bytes) in cooked.cas.iter() {
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(CompileError::prove(ProveError::BlobTooLarge {
                size: bytes.len(),
            }));
        }
        validate_blob(bytes)?;
    }
    encode_warp(cooked)
}

/// Write one `.warp` file.
pub fn write_warp(path: &Path, cooked: &Cooked) -> Result<(), CompileError> {
    let bytes = pack_warp(cooked)?;
    fs::write(path, bytes).map_err(|e| CompileError::Io(format!("{}: {e}", path.display())))
}

/// Parse `.warp` bytes. Caller must have applied the file-size cap already.
pub fn unpack_warp(bytes: &[u8]) -> Result<Cooked, CompileError> {
    if bytes.len() > WARP_CAP_DESKTOP {
        return Err(warp_err(format!(
            "file {} bytes exceeds cap {WARP_CAP_DESKTOP}",
            bytes.len()
        )));
    }
    let mut rest = bytes;
    let magic = take(&mut rest, 4)?;
    if magic != WARP_MAGIC {
        return Err(warp_err("bad magic"));
    }
    let version = take_u8(&mut rest)?;
    if version != 1 && version != WARP_VERSION {
        return Err(warp_err(format!("bad version {version}")));
    }
    let pad = take(&mut rest, 3)?;
    if pad != [0, 0, 0] {
        return Err(warp_err("bad pad"));
    }
    let compiler = take_u32(&mut rest)?;
    if compiler != COMPILER_VERSION {
        return Err(warp_err(format!("compiler version {compiler}")));
    }
    let optimized = version >= 3 && take_u8(&mut rest)? != 0;
    let cook_hash = take_hash(&mut rest)?;
    let canon_hash = take_hash(&mut rest)?;

    let doc_bytes = take_len_bytes(&mut rest)?;
    let doc_src = std::str::from_utf8(doc_bytes).map_err(|_| warp_err("doc utf8"))?;
    let doc: IntentDoc = from_ron(doc_src).map_err(|e| warp_err(e.to_string()))?;
    check_seed_loci(&doc)?;

    let n_blobs = take_u32(&mut rest)? as usize;
    if n_blobs > MAX_BLOBS {
        return Err(CompileError::prove(ProveError::CasFull));
    }
    let mut cas = Cas::new();
    for _ in 0..n_blobs {
        let id = take_blob(&mut rest)?;
        let blob = take_len_bytes(&mut rest)?;
        if blob.len() > MAX_BLOB_BYTES {
            return Err(CompileError::prove(ProveError::BlobTooLarge {
                size: blob.len(),
            }));
        }
        if blob_id_of(blob) != id {
            return Err(warp_err("blob id mismatch"));
        }
        validate_blob(blob)?;
        let got = cas.put(blob).map_err(CompileError::prove)?;
        if got != id {
            return Err(warp_err("blob id mismatch"));
        }
    }

    let dag = decode_dag(&mut rest)?;
    dag.exportable().map_err(CompileError::prove)?;
    dag.blobs_present(&cas).map_err(CompileError::prove)?;

    let n_bind = take_capped_count(&mut rest, WARP_MAX_LOCI, "bindings")?;
    let mut bindings = Vec::new();
    for _ in 0..n_bind {
        let locus = Name::from(take_str(&mut rest)?);
        let tag = Name::from(take_str(&mut rest)?);
        let hull = take_blob(&mut rest)?;
        let mesh = take_blob(&mut rest)?;
        let mat = take_u8(&mut rest)?;
        let material =
            MaterialTag::from_u8(mat).ok_or_else(|| warp_err(format!("material {mat}")))?;
        bindings.push(Binding {
            locus,
            tag,
            hull,
            mesh,
            material,
        });
    }

    let grains = decode_named_ids(&mut rest)?;
    let clips = decode_named_ids(&mut rest)?;
    if !rest.is_empty() {
        return Err(warp_err("trailing bytes"));
    }

    let mut canon = cook_canon(&doc).map_err(CompileError::canon)?;
    if optimized {
        canon.intern_predicates();
    }
    let digest = cook_digest(&doc, &kit_blobs_from_cas(&cas));
    let expected_cook = if optimized {
        optimized_cook_digest(digest)
    } else {
        digest
    };
    if expected_cook != cook_hash || digest != canon_hash {
        return Err(warp_err("canon hash mismatch"));
    }
    Ok(Cooked {
        doc,
        canon,
        cook_hash,
        canon_hash,
        cas,
        dag,
        bindings,
        grains,
        clips,
        optimized,
    })
}

fn encode_warp(cooked: &Cooked) -> Result<Vec<u8>, CompileError> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&WARP_MAGIC);
    buf.push(WARP_VERSION);
    buf.extend_from_slice(&[0, 0, 0]);
    buf.extend_from_slice(&COMPILER_VERSION.to_le_bytes());
    buf.push(u8::from(cooked.optimized));
    buf.extend_from_slice(cooked.cook_hash.as_bytes());
    buf.extend_from_slice(cooked.canon_hash.as_bytes());

    let doc_ron = to_ron(&cooked.doc).map_err(|e| warp_err(e.to_string()))?;
    put_bytes(&mut buf, doc_ron.as_bytes())?;

    put_u32(&mut buf, u32_len(cooked.cas.len())?);
    for (id, bytes) in cooked.cas.iter() {
        buf.extend_from_slice(id.as_bytes());
        put_bytes(&mut buf, bytes)?;
    }

    encode_dag(&cooked.dag, &mut buf)?;

    put_u32(&mut buf, u32_len(cooked.bindings.len())?);
    for b in &cooked.bindings {
        put_bytes(&mut buf, b.locus.as_str().as_bytes())?;
        put_bytes(&mut buf, b.tag.as_str().as_bytes())?;
        buf.extend_from_slice(b.hull.as_bytes());
        buf.extend_from_slice(b.mesh.as_bytes());
        buf.push(b.material.as_u8());
    }

    encode_named_ids(&mut buf, &cooked.grains)?;
    encode_named_ids(&mut buf, &cooked.clips)?;

    if buf.len() > WARP_CAP_DESKTOP {
        return Err(warp_err(format!(
            "file {} bytes exceeds cap {WARP_CAP_DESKTOP}",
            buf.len()
        )));
    }
    Ok(buf)
}

fn encode_named_ids(buf: &mut Vec<u8>, map: &BTreeMap<String, BlobId>) -> Result<(), CompileError> {
    put_u32(buf, u32_len(map.len())?);
    for (tag, id) in map {
        put_bytes(buf, tag.as_bytes())?;
        buf.extend_from_slice(id.as_bytes());
    }
    Ok(())
}

fn decode_named_ids(rest: &mut &[u8]) -> Result<BTreeMap<String, BlobId>, CompileError> {
    let n = take_capped_count(rest, MAX_BLOBS, "named ids")?;
    let mut out = BTreeMap::new();
    for _ in 0..n {
        let tag = take_str(rest)?.to_string();
        let id = take_blob(rest)?;
        out.insert(tag, id);
    }
    Ok(out)
}

fn encode_dag(dag: &ProvenanceDag, buf: &mut Vec<u8>) -> Result<(), CompileError> {
    put_u32(buf, u32_len(dag.len())?);
    let mut done = BTreeSet::new();
    while done.len() < dag.len() {
        let mut progress = false;
        for n in dag.iter() {
            if done.contains(&n.id) {
                continue;
            }
            if n.parents.iter().all(|p| done.contains(p)) {
                encode_node(n, buf)?;
                done.insert(n.id);
                progress = true;
            }
        }
        if !progress {
            return Err(warp_err("cyclic provenance"));
        }
    }
    Ok(())
}

fn encode_node(n: &ProvenanceNode, buf: &mut Vec<u8>) -> Result<(), CompileError> {
    buf.extend_from_slice(n.id.as_bytes());
    encode_kind(buf, &n.kind);
    encode_license(buf, &n.license)?;
    put_u32(buf, u32_len(n.parents.len())?);
    for p in &n.parents {
        buf.extend_from_slice(p.as_bytes());
    }
    Ok(())
}

fn decode_dag(rest: &mut &[u8]) -> Result<ProvenanceDag, CompileError> {
    let n = take_capped_count(rest, MAX_BLOBS.saturating_mul(4), "dag nodes")?;
    let mut dag = ProvenanceDag::new();
    for _ in 0..n {
        let id = ProvenanceId(take_hash(rest)?);
        let kind = decode_kind(rest)?;
        let license = decode_license(rest)?;
        let np = take_u32(rest)? as usize;
        let need = np
            .checked_mul(32)
            .ok_or_else(|| warp_err("parent count overflow"))?;
        if rest.len() < need {
            return Err(warp_err("truncated"));
        }
        let mut parents = Vec::new();
        for _ in 0..np {
            parents.push(ProvenanceId(take_hash(rest)?));
        }
        let got = dag
            .insert(kind, license, &parents)
            .map_err(CompileError::prove)?;
        if got != id {
            return Err(warp_err("provenance id mismatch"));
        }
    }
    Ok(dag)
}

fn encode_kind(buf: &mut Vec<u8>, kind: &ProvenanceKind) {
    match kind {
        ProvenanceKind::Artifact { blob, kind } => {
            buf.push(0);
            buf.extend_from_slice(blob.as_bytes());
            buf.push(*kind as u8);
        }
        ProvenanceKind::Intent { doc_hash } => {
            buf.push(1);
            buf.extend_from_slice(doc_hash.as_bytes());
        }
        ProvenanceKind::Activity { activity } => {
            buf.push(2);
            buf.push(*activity as u8);
        }
        ProvenanceKind::Agent { agent } => {
            buf.push(3);
            match agent {
                Agent::Author => buf.push(0),
                Agent::Compiler { version } => {
                    buf.push(1);
                    buf.extend_from_slice(&version.to_le_bytes());
                }
                Agent::Kitbash => buf.push(2),
                Agent::Model => buf.push(3),
            }
        }
        ProvenanceKind::TracePrefix { prefix_hash } => {
            buf.push(4);
            buf.extend_from_slice(prefix_hash.as_bytes());
        }
    }
}

fn decode_kind(rest: &mut &[u8]) -> Result<ProvenanceKind, CompileError> {
    match take_u8(rest)? {
        0 => {
            let blob = take_blob(rest)?;
            let k = take_u8(rest)?;
            let kind = ArtifactKind::from_u8(k).ok_or_else(|| warp_err(format!("kind {k}")))?;
            Ok(ProvenanceKind::Artifact { blob, kind })
        }
        1 => Ok(ProvenanceKind::Intent {
            doc_hash: take_hash(rest)?,
        }),
        2 => {
            let a = take_u8(rest)?;
            let activity = match a {
                0 => Activity::Cook,
                1 => Activity::Pin,
                2 => Activity::Commit,
                3 => Activity::Eval,
                _ => return Err(warp_err(format!("activity {a}"))),
            };
            Ok(ProvenanceKind::Activity { activity })
        }
        3 => {
            let agent = match take_u8(rest)? {
                0 => Agent::Author,
                1 => Agent::Compiler {
                    version: take_u32(rest)?,
                },
                2 => Agent::Kitbash,
                3 => Agent::Model,
                a => return Err(warp_err(format!("agent {a}"))),
            };
            Ok(ProvenanceKind::Agent { agent })
        }
        4 => Ok(ProvenanceKind::TracePrefix {
            prefix_hash: take_hash(rest)?,
        }),
        t => Err(warp_err(format!("provenance kind {t}"))),
    }
}

fn encode_license(buf: &mut Vec<u8>, lic: &LicenseSpan) -> Result<(), CompileError> {
    match lic {
        LicenseSpan::Unknown => buf.push(0),
        LicenseSpan::Spdx { id, copyright } => {
            buf.push(1);
            put_bytes(buf, id.as_bytes())?;
            put_bytes(buf, copyright.as_bytes())?;
        }
        LicenseSpan::Commissioned {
            holder,
            contract_hash,
        } => {
            buf.push(2);
            put_bytes(buf, holder.as_bytes())?;
            buf.extend_from_slice(contract_hash.as_bytes());
        }
    }
    Ok(())
}

fn decode_license(rest: &mut &[u8]) -> Result<LicenseSpan, CompileError> {
    match take_u8(rest)? {
        0 => Ok(LicenseSpan::Unknown),
        1 => {
            let id = take_str(rest)?.to_string();
            let copyright = take_str(rest)?.to_string();
            LicenseSpan::spdx(id, copyright).map_err(CompileError::prove)
        }
        2 => {
            let holder = take_str(rest)?.to_string();
            let contract_hash = take_hash(rest)?;
            LicenseSpan::commissioned(holder, contract_hash).map_err(CompileError::prove)
        }
        t => Err(warp_err(format!("license {t}"))),
    }
}

fn kit_blobs_from_cas(cas: &Cas) -> Vec<BlobId> {
    let mut ids = Vec::new();
    for (id, bytes) in cas.iter() {
        if let Ok(
            ArtifactKind::ClusteredMesh
            | ArtifactKind::Hull
            | ArtifactKind::Grain
            | ArtifactKind::ClipSet
            | ArtifactKind::SkinnedMesh,
        ) = peek_kind(bytes)
        {
            ids.push(id);
        }
    }
    ids
}

fn take_capped_count(rest: &mut &[u8], max: usize, what: &str) -> Result<usize, CompileError> {
    let n = take_u32(rest)? as usize;
    if n > max {
        return Err(warp_err(format!("{what} {n} > {max}")));
    }
    Ok(n)
}

fn check_seed_loci(doc: &IntentDoc) -> Result<(), CompileError> {
    let n = doc
        .seed
        .iter()
        .filter(|f| matches!(f, SeedFact::Locus { .. }))
        .count();
    if n > WARP_MAX_LOCI {
        return Err(warp_err(format!("seed loci {n} > {WARP_MAX_LOCI}")));
    }
    Ok(())
}

fn warp_err(s: impl Into<String>) -> CompileError {
    CompileError::Warp(s.into())
}

fn u32_len(n: usize) -> Result<u32, CompileError> {
    u32::try_from(n).map_err(|_| warp_err("length overflow"))
}

fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_bytes(buf: &mut Vec<u8>, data: &[u8]) -> Result<(), CompileError> {
    put_u32(buf, u32_len(data.len())?);
    buf.extend_from_slice(data);
    Ok(())
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], CompileError> {
    if rest.len() < n {
        return Err(warp_err("truncated"));
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_u8(rest: &mut &[u8]) -> Result<u8, CompileError> {
    Ok(take(rest, 1)?[0])
}

fn take_u32(rest: &mut &[u8]) -> Result<u32, CompileError> {
    let s = take(rest, 4)?;
    Ok(u32::from_le_bytes(s.try_into().expect("4")))
}

fn take_hash(rest: &mut &[u8]) -> Result<Hash, CompileError> {
    let s = take(rest, 32)?;
    Ok(Hash::from_bytes(s.try_into().expect("32")))
}

fn take_blob(rest: &mut &[u8]) -> Result<BlobId, CompileError> {
    let s = take(rest, 32)?;
    Ok(BlobId::from_bytes(s.try_into().expect("32")))
}

fn take_len_bytes<'a>(rest: &mut &'a [u8]) -> Result<&'a [u8], CompileError> {
    let n = take_u32(rest)? as usize;
    take(rest, n)
}

fn take_str<'a>(rest: &mut &'a [u8]) -> Result<&'a str, CompileError> {
    let b = take_len_bytes(rest)?;
    std::str::from_utf8(b).map_err(|_| warp_err("utf8"))
}

#[cfg(test)]
mod tests {
    use klotho_prove::{Agent, LicenseSpan, ProvenanceKind};

    use super::*;
    use crate::header::{MAX_TRIS, write_prefix};
    use crate::{MAGIC, cook_doc};

    fn hearth() -> Cooked {
        cook_doc(&hearth_slice::hearth_doc()).unwrap()
    }

    fn blob_ids(cas: &Cas) -> Vec<BlobId> {
        cas.iter().map(|(id, _)| id).collect()
    }

    #[test]
    fn caps_match_hld() {
        assert_eq!(WARP_MAGIC, *b"KWRP");
        assert_ne!(WARP_MAGIC, MAGIC);
        assert_eq!(WARP_VERSION, 3);
        assert_eq!(WARP_CAP_DESKTOP, 512 * 1024 * 1024);
        assert_eq!(WARP_CAP_MOBILE, 192 * 1024 * 1024);
        assert_eq!(WARP_MAX_LOCI, 4_096);
        assert_eq!(MAX_BLOBS, 16_384);
        assert_eq!(MAX_BLOB_BYTES, 32 * 1024 * 1024);
    }

    #[test]
    fn hearth_cooks_to_one_warp_and_round_trips() {
        let cooked = hearth();
        let a = pack_warp(&cooked).unwrap();
        let b = pack_warp(&cooked).unwrap();
        assert_eq!(a, b, "pack is deterministic");
        assert!(a.len() < WARP_CAP_DESKTOP);
        assert!(a.starts_with(&WARP_MAGIC));
        assert!(!a.starts_with(&MAGIC));

        let dir = std::env::temp_dir();
        let path = dir.join(format!("klotho-hearth-{}.warp", std::process::id()));
        write_warp(&path, &cooked).unwrap();
        assert!(path.is_file(), "Hearth cooks to one file");
        let on_disk = fs::read(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(on_disk, a);

        let unpacked = unpack_warp(&a).unwrap();
        assert_eq!(unpacked.cook_hash, cooked.cook_hash);
        assert_eq!(unpacked.canon_hash, cooked.canon_hash);
        assert_eq!(blob_ids(&unpacked.cas), blob_ids(&cooked.cas));
        assert_eq!(unpacked.cas.len(), cooked.cas.len());
        assert!(unpacked.canon.rite_id("lockpick").is_some());
        assert!(unpacked.dag.exportable().is_ok());
    }

    #[test]
    fn unknown_license_fails_pack() {
        let mut cooked = hearth();
        cooked
            .dag
            .insert(
                ProvenanceKind::Agent {
                    agent: Agent::Author,
                },
                LicenseSpan::Unknown,
                &[],
            )
            .unwrap();
        let e = pack_warp(&cooked).unwrap_err();
        assert!(
            matches!(e, CompileError::Prove(ref s) if s == "UnknownLicense"),
            "{e}"
        );
    }

    fn bad_mesh() -> Vec<u8> {
        let mut b = Vec::new();
        write_prefix(&mut b, ArtifactKind::ClusteredMesh);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&(MAX_TRIS.saturating_add(1).saturating_mul(3)).to_le_bytes());
        b
    }

    fn bad_grain() -> Vec<u8> {
        let mut b = Vec::new();
        write_prefix(&mut b, ArtifactKind::Grain);
        b.extend_from_slice(&44_100u32.to_le_bytes());
        b.push(1);
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&0i16.to_le_bytes());
        b
    }

    #[test]
    fn corrupt_mesh_header_fails_pack_and_unpack() {
        let mut cooked = hearth();
        let mut cas = Cas::new();
        cas.put(&bad_mesh()).unwrap();
        cooked.cas = cas;
        let e = pack_warp(&cooked).unwrap_err();
        assert!(
            matches!(e, CompileError::Header(ref s) if s.contains("tris")),
            "{e}"
        );

        let bytes = encode_warp(&cooked).unwrap();
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(matches!(e, CompileError::Header(_)), "{e}");
    }

    #[test]
    fn corrupt_grain_header_fails_pack_and_unpack() {
        let mut cooked = hearth();
        let mut cas = Cas::new();
        cas.put(&bad_grain()).unwrap();
        cooked.cas = cas;
        let e = pack_warp(&cooked).unwrap_err();
        assert!(
            matches!(e, CompileError::Header(ref s) if s.contains("hz")),
            "{e}"
        );

        let bytes = encode_warp(&cooked).unwrap();
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(matches!(e, CompileError::Header(_)), "{e}");
    }

    #[test]
    fn unpack_refuses_klth_magic() {
        let e = unpack_warp(b"KLTH").unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("magic")),
            "{e}"
        );
    }

    #[test]
    fn unpack_refuses_nonzero_pad() {
        let mut bytes = pack_warp(&hearth()).unwrap();
        bytes[5] = 1;
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("pad")),
            "{e}"
        );
    }

    #[test]
    fn unpack_refuses_flipped_header_hash() {
        let mut bytes = pack_warp(&hearth()).unwrap();
        bytes[13] ^= 1;
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("hash")),
            "{e}"
        );
    }

    #[test]
    fn unpack_refuses_spliced_intent_doc() {
        let mut cooked = hearth();
        cooked.doc.style.notes = "tampered notes".into();
        let bytes = encode_warp(&cooked).unwrap();
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("hash")),
            "{e}"
        );
    }

    #[test]
    fn unpack_refuses_huge_binding_count() {
        let mut cooked = hearth();
        cooked.bindings.clear();
        cooked.grains.clear();
        cooked.clips.clear();
        let mut bytes = encode_warp(&cooked).unwrap();
        let i = bytes.len() - 12;
        bytes[i..i + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("bindings")),
            "{e}"
        );
    }

    #[test]
    fn seed_loci_over_cap_fails_pack_and_unpack() {
        use klotho_core::LocusKind;

        let mut cooked = hearth();
        let extra = WARP_MAX_LOCI + 1
            - cooked
                .doc
                .seed
                .iter()
                .filter(|f| matches!(f, SeedFact::Locus { .. }))
                .count();
        for i in 0..extra {
            cooked.doc.seed.push(SeedFact::Locus {
                name: Name::from(format!("cap_locus_{i}").as_str()),
                kind: LocusKind::Relic,
            });
        }
        let e = pack_warp(&cooked).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("seed loci")),
            "{e}"
        );
        let bytes = encode_warp(&cooked).unwrap();
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("seed loci")),
            "{e}"
        );
    }
}
