//! Sharded catalog writer: `catalog.kwrp` + KCAS volumes + Place shards.

use std::fs;
use std::path::{Path, PathBuf};

use klotho_core::{AabbMm, BlobId, Hash, Sigil};
use klotho_prove::{
    CATALOG_CAP, KCAS_VOLUME_CAP, LicenseSpan, ProveError, ProvenanceDag, ProvenanceKind,
    blob_id_of,
};
use klotho_stream::{
    CatalogDesc, KcasEntry, PlaceRef, PlaceSnap, VolumeRef, encode_catalog, encode_kcas,
    encode_place_shard, file_hash,
};

use crate::cook::{COMPILER_VERSION, Cooked};
use crate::error::CompileError;

/// Catalog file name under the output directory.
pub const CATALOG_FILE: &str = "catalog.kwrp";

/// Paths and hashes written by [`write_catalog`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogManifest {
    /// `catalog.kwrp` path.
    pub catalog_path: PathBuf,
    /// Cook digest copied from [`Cooked`].
    pub canon_hash: Hash,
    /// Volume file content hashes and paths, write order.
    pub volumes: Vec<(Hash, PathBuf)>,
    /// Place shards in sigil order.
    pub places: Vec<PlaceCatalogEntry>,
}

/// One Place shard on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceCatalogEntry {
    /// Place locus.
    pub place: Sigil,
    /// Coarse AABB.
    pub aabb: AabbMm,
    /// blake3 of the shard file.
    pub shard_id: Hash,
    /// Shard path.
    pub shard_path: PathBuf,
    /// Capture prefix stored on the shard.
    pub prefix: Hash,
}

/// Pack `cooked` + Place snaps into `dir` as catalog, volumes, and shards.
///
/// Fails on Unknown license, oversize, or a snap whose `canon_hash` disagrees.
pub fn write_catalog(
    dir: &Path,
    cooked: &Cooked,
    places: &[(PlaceSnap, AabbMm)],
) -> Result<CatalogManifest, CompileError> {
    cooked.dag.exportable().map_err(CompileError::prove)?;
    fs::create_dir_all(dir).map_err(|e| CompileError::Io(format!("{}: {e}", dir.display())))?;

    let mut order: Vec<usize> = (0..places.len()).collect();
    order.sort_by_key(|&i| places[i].0.place);
    let mut seen: Option<Sigil> = None;
    for &i in &order {
        let snap = &places[i].0;
        if snap.canon_hash != cooked.canon_hash {
            return Err(warp_err("place canon hash mismatch"));
        }
        if seen == Some(snap.place) {
            return Err(warp_err("duplicate place"));
        }
        seen = Some(snap.place);
    }

    let mut entries = Vec::new();
    for (id, bytes) in cooked.cas.iter() {
        let license = license_of(&cooked.dag, id)?;
        if blob_id_of(bytes) != id {
            return Err(warp_err("blob id mismatch"));
        }
        entries.push((license, bytes));
    }
    let packed = pack_volumes(&entries)?;

    let mut volumes = Vec::new();
    let mut blob_index = Vec::new();
    for (i, vol) in packed.iter().enumerate() {
        if vol.bytes.len() > KCAS_VOLUME_CAP {
            return Err(warp_err(format!(
                "volume {} bytes exceeds cap {KCAS_VOLUME_CAP}",
                vol.bytes.len()
            )));
        }
        let name = format!("vol-{i:04}.kcas");
        let path = dir.join(&name);
        fs::write(&path, &vol.bytes)
            .map_err(|e| CompileError::Io(format!("{}: {e}", path.display())))?;
        let id = file_hash(&vol.bytes);
        for blob in &vol.ids {
            blob_index.push((*blob, id));
        }
        volumes.push(VolumeRef { id, filename: name });
    }

    let mut place_refs = Vec::new();
    let mut place_entries = Vec::new();
    for &i in &order {
        let (snap, aabb) = &places[i];
        let shard = encode_place_shard(snap).map_err(stream_err)?;
        let name = format!("place-{:032x}.kplc", snap.place.raw());
        let path = dir.join(&name);
        fs::write(&path, &shard)
            .map_err(|e| CompileError::Io(format!("{}: {e}", path.display())))?;
        let shard_id = file_hash(&shard);
        place_refs.push(PlaceRef {
            place: snap.place,
            aabb: *aabb,
            shard_id,
            filename: name,
            prefix: snap.prefix,
        });
        place_entries.push(PlaceCatalogEntry {
            place: snap.place,
            aabb: *aabb,
            shard_id,
            shard_path: path,
            prefix: snap.prefix,
        });
    }

    let desc = CatalogDesc {
        canon_hash: cooked.canon_hash,
        volumes: volumes.clone(),
        places: place_refs,
        blobs: blob_index,
    };
    let catalog = encode_catalog(COMPILER_VERSION, &desc).map_err(stream_err)?;
    if catalog.len() > CATALOG_CAP {
        return Err(warp_err(format!(
            "catalog {} bytes exceeds cap {CATALOG_CAP}",
            catalog.len()
        )));
    }
    let catalog_path = dir.join(CATALOG_FILE);
    fs::write(&catalog_path, catalog)
        .map_err(|e| CompileError::Io(format!("{}: {e}", catalog_path.display())))?;

    Ok(CatalogManifest {
        catalog_path,
        canon_hash: cooked.canon_hash,
        volumes: volumes
            .into_iter()
            .map(|v| (v.id, dir.join(v.filename)))
            .collect(),
        places: place_entries,
    })
}

struct PackedVolume {
    bytes: Vec<u8>,
    ids: Vec<BlobId>,
}

fn pack_volumes(entries: &[(LicenseSpan, &[u8])]) -> Result<Vec<PackedVolume>, CompileError> {
    let mut out = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    for i in 0..entries.len() {
        cur.push(i);
        match encode_slice(entries, &cur) {
            Ok(bytes) if bytes.len() <= KCAS_VOLUME_CAP => {}
            Ok(_) | Err(klotho_stream::StreamError::Oversize { .. }) => {
                cur.pop();
                if cur.is_empty() {
                    return Err(warp_err("kcas volume cap"));
                }
                let bytes = encode_slice(entries, &cur).map_err(stream_err)?;
                out.push(PackedVolume {
                    bytes,
                    ids: ids_of(entries, &cur),
                });
                cur.clear();
                cur.push(i);
                let bytes = encode_slice(entries, &cur).map_err(stream_err)?;
                if bytes.len() > KCAS_VOLUME_CAP {
                    return Err(warp_err("kcas volume cap"));
                }
            }
            Err(e) => return Err(stream_err(e)),
        }
    }
    if !cur.is_empty() {
        let bytes = encode_slice(entries, &cur).map_err(stream_err)?;
        out.push(PackedVolume {
            bytes,
            ids: ids_of(entries, &cur),
        });
    }
    Ok(out)
}

fn encode_slice(
    entries: &[(LicenseSpan, &[u8])],
    idx: &[usize],
) -> Result<Vec<u8>, klotho_stream::StreamError> {
    let kcas: Vec<KcasEntry<'_>> = idx
        .iter()
        .map(|&i| KcasEntry {
            license: entries[i].0.clone(),
            bytes: entries[i].1,
        })
        .collect();
    encode_kcas(&kcas)
}

fn ids_of(entries: &[(LicenseSpan, &[u8])], idx: &[usize]) -> Vec<BlobId> {
    idx.iter().map(|&i| blob_id_of(entries[i].1)).collect()
}

fn license_of(dag: &ProvenanceDag, id: BlobId) -> Result<LicenseSpan, CompileError> {
    for n in dag.iter() {
        if let ProvenanceKind::Artifact { blob, .. } = n.kind {
            if blob == id {
                if !n.license.is_exportable() {
                    return Err(CompileError::prove(ProveError::UnknownLicense));
                }
                return Ok(n.license.clone());
            }
        }
    }
    Err(warp_err(format!("no license for {id}")))
}

fn stream_err(e: klotho_stream::StreamError) -> CompileError {
    CompileError::Warp(e.to_string())
}

fn warp_err(s: impl Into<String>) -> CompileError {
    CompileError::Warp(s.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use klotho_core::{IVec3, LocusKind};
    use klotho_prove::{Agent, ProvenanceKind, blob_id_of};
    use klotho_stream::{
        CATALOG_VERSION, PlaceRow, StreamCatalog, blob, map_place, place_snap_from_bytes,
    };

    use crate::cook_doc;
    use crate::warp::{WARP_VERSION, pack_warp, unpack_warp};

    fn hearth() -> Cooked {
        cook_doc(&hearth_slice::hearth_doc()).unwrap()
    }

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    fn temp_dir() -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "klotho-compile-cat-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn snap_for(cooked: &Cooked, p: Sigil) -> PlaceSnap {
        PlaceSnap::new(
            p,
            cooked.canon_hash,
            Hash::from_bytes([2; 32]),
            vec![PlaceRow::new(p, LocusKind::Place)],
        )
    }

    #[test]
    fn unpack_warp_refuses_catalog_version() {
        let cooked = hearth();
        let p = place(1);
        let dir = temp_dir();
        let man = write_catalog(
            &dir,
            &cooked,
            &[(snap_for(&cooked, p), AabbMm::from_point(IVec3::ZERO))],
        )
        .unwrap();
        let bytes = fs::read(&man.catalog_path).unwrap();
        assert!(bytes.starts_with(b"KWRP"));
        assert_eq!(bytes[4], CATALOG_VERSION);
        assert_ne!(bytes[4], WARP_VERSION);
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("version")),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn catalog_v2_bytes_fail_unpack_warp() {
        let mut bytes = b"KWRP".to_vec();
        bytes.push(2);
        bytes.extend_from_slice(&[1, 0, 0]);
        let e = unpack_warp(&bytes).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("version")),
            "{e}"
        );
    }

    #[test]
    fn write_catalog_round_trips_through_stream() {
        let cooked = hearth();
        let p = place(7);
        let snap = snap_for(&cooked, p);
        let dir = temp_dir();
        let aabb = AabbMm::new(
            IVec3 {
                x: -10,
                y: 0,
                z: -10,
            },
            IVec3 {
                x: 10,
                y: 10,
                z: 10,
            },
        );
        let man = write_catalog(&dir, &cooked, &[(snap.clone(), aabb)]).unwrap();
        assert_eq!(man.canon_hash, cooked.canon_hash);
        assert_eq!(man.places.len(), 1);

        let cat = StreamCatalog::open(&man.catalog_path).unwrap();
        let got = map_place(&cat, p).unwrap();
        assert_eq!(*got, snap);
        let disk = fs::read(&man.places[0].shard_path).unwrap();
        assert_eq!(place_snap_from_bytes(&disk).unwrap(), snap);

        let (id, bytes) = cooked.cas.iter().next().expect("cas");
        let fetched = blob(&cat, id).unwrap();
        assert_eq!(&*fetched, bytes);
        assert_eq!(blob_id_of(&fetched), id);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_license_fails_write_catalog() {
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
        let dir = temp_dir();
        let e = write_catalog(&dir, &cooked, &[]).unwrap_err();
        assert!(
            matches!(e, CompileError::Prove(ref s) if s == "UnknownLicense"),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn monolith_pack_still_round_trips() {
        let cooked = hearth();
        let a = pack_warp(&cooked).unwrap();
        let unpacked = unpack_warp(&a).unwrap();
        assert_eq!(unpacked.canon_hash, cooked.canon_hash);
    }

    #[test]
    fn snap_canon_mismatch_fails() {
        let cooked = hearth();
        let p = place(1);
        let snap = PlaceSnap::new(
            p,
            Hash::from_bytes([9; 32]),
            Hash::ZERO,
            vec![PlaceRow::new(p, LocusKind::Place)],
        );
        let dir = temp_dir();
        let e =
            write_catalog(&dir, &cooked, &[(snap, AabbMm::from_point(IVec3::ZERO))]).unwrap_err();
        assert!(
            matches!(e, CompileError::Warp(ref s) if s.contains("canon")),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
