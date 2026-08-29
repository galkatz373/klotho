//! Open a catalog, map Place shards, fetch CAS blobs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use klotho_core::{AabbMm, BlobId, Hash, Sigil};
use klotho_prove::{CATALOG_CAP, KCAS_VOLUME_CAP, PLACE_SHARD_CAP, blob_id_of};
use klotho_world::PlaceSnap;

use crate::codec::{
    CatalogDesc, KCAS_HEADER_LEN, PLACE_HEADER_LEN, check_place_sizes, decode_catalog, file_hash,
    kcas_blob, parse_kcas_header, parse_place_header, place_snap_from_bytes,
};
use crate::error::StreamError;
use crate::map::{file_len_at_most, map_or_read, read_capped};

/// Parsed catalog. Small enough to live in RAM (≤ [`CATALOG_CAP`]).
#[derive(Clone, Debug)]
pub struct StreamCatalog {
    dir: PathBuf,
    canon_hash: Hash,
    volumes: BTreeMap<Hash, String>,
    places: BTreeMap<Sigil, PlaceRec>,
    blobs: BTreeMap<BlobId, Hash>,
}

#[derive(Clone, Debug)]
struct PlaceRec {
    aabb: AabbMm,
    shard_id: Hash,
    filename: String,
    prefix: Hash,
}

impl StreamCatalog {
    /// Read `path` under [`CATALOG_CAP`], then parse.
    pub fn open(path: &Path) -> Result<Self, StreamError> {
        Self::open_capped(path, CATALOG_CAP)
    }

    /// Same as [`Self::open`] with an explicit read cap (tests use a small max).
    pub fn open_capped(path: &Path, cap: usize) -> Result<Self, StreamError> {
        let bytes = read_capped(path, cap)?;
        let (_compiler, desc) = decode_catalog(&bytes)?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Self::from_desc(dir, desc)
    }

    fn from_desc(dir: PathBuf, desc: CatalogDesc) -> Result<Self, StreamError> {
        let mut names = BTreeSet::new();
        let mut volumes = BTreeMap::new();
        for v in desc.volumes {
            if volumes.insert(v.id, v.filename.clone()).is_some() {
                return Err(StreamError::Duplicate);
            }
            if !names.insert(v.filename) {
                return Err(StreamError::Name);
            }
        }
        let mut places = BTreeMap::new();
        for p in desc.places {
            if !names.insert(p.filename.clone()) {
                return Err(StreamError::Name);
            }
            if places
                .insert(
                    p.place,
                    PlaceRec {
                        aabb: p.aabb,
                        shard_id: p.shard_id,
                        filename: p.filename,
                        prefix: p.prefix,
                    },
                )
                .is_some()
            {
                return Err(StreamError::Duplicate);
            }
        }
        let mut blobs = BTreeMap::new();
        for (blob, vol) in desc.blobs {
            if blobs.insert(blob, vol).is_some() {
                return Err(StreamError::Duplicate);
            }
        }
        Ok(Self {
            dir,
            canon_hash: desc.canon_hash,
            volumes,
            places,
            blobs,
        })
    }

    /// Cook digest recorded on the catalog.
    #[must_use]
    pub fn canon_hash(&self) -> Hash {
        self.canon_hash
    }

    /// `true` if `place` has a catalog row.
    #[must_use]
    pub fn contains_place(&self, place: Sigil) -> bool {
        self.places.contains_key(&place)
    }

    /// Directory holding shard and volume files.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Coarse residency AABB for `place`, if the catalog names it.
    #[must_use]
    pub fn place_aabb(&self, place: Sigil) -> Option<AabbMm> {
        self.places.get(&place).map(|r| r.aabb)
    }
}

/// Header-validate a Place shard, map it, decode to [`PlaceSnap`].
pub fn map_place(catalog: &StreamCatalog, place: Sigil) -> Result<Arc<PlaceSnap>, StreamError> {
    let rec = catalog
        .places
        .get(&place)
        .ok_or(StreamError::MissingPlace)?;
    let path = catalog.dir.join(&rec.filename);
    if !path.is_file() {
        return Err(StreamError::MissingShard);
    }
    let file_len = file_len_at_most(&path, PLACE_SHARD_CAP)?;
    if file_len < PLACE_HEADER_LEN {
        return Err(StreamError::Truncated);
    }

    let mut file = File::open(&path)?;
    let mut hdr = [0u8; PLACE_HEADER_LEN];
    file.read_exact(&mut hdr)?;
    let header = parse_place_header(&hdr)?;
    check_place_sizes(file_len, &header)?;
    if header.place != place {
        return Err(StreamError::PlaceMismatch);
    }
    if header.canon_hash != catalog.canon_hash {
        return Err(StreamError::CanonHashMismatch);
    }
    if header.prefix != rec.prefix {
        return Err(StreamError::PrefixMismatch);
    }

    let mapped = map_or_read(&path, PLACE_SHARD_CAP)?;
    let bytes = mapped.as_slice();
    if bytes.len() != file_len {
        return Err(StreamError::Truncated);
    }
    if file_hash(bytes) != rec.shard_id {
        return Err(StreamError::HashMismatch);
    }
    let snap = place_snap_from_bytes(bytes)?;
    if snap.place != place {
        return Err(StreamError::PlaceMismatch);
    }
    if snap.canon_hash != catalog.canon_hash {
        return Err(StreamError::CanonHashMismatch);
    }
    if snap.prefix != rec.prefix {
        return Err(StreamError::PrefixMismatch);
    }
    Ok(Arc::new(snap))
}

/// Fetch a blob from its KCAS volume. Recomputes [`blob_id_of`].
pub fn blob(catalog: &StreamCatalog, id: BlobId) -> Result<Arc<[u8]>, StreamError> {
    let vol_id = catalog.blobs.get(&id).ok_or(StreamError::MissingBlob)?;
    let filename = catalog
        .volumes
        .get(vol_id)
        .ok_or(StreamError::MissingBlob)?;
    let path = catalog.dir.join(filename);
    if !path.is_file() {
        return Err(StreamError::MissingShard);
    }
    let file_len = file_len_at_most(&path, KCAS_VOLUME_CAP)?;
    if file_len < KCAS_HEADER_LEN {
        return Err(StreamError::Truncated);
    }
    let mut file = File::open(&path)?;
    let mut hdr = [0u8; KCAS_HEADER_LEN];
    file.read_exact(&mut hdr)?;
    let _ = parse_kcas_header(&hdr)?;
    let mapped = map_or_read(&path, KCAS_VOLUME_CAP)?;
    if mapped.as_slice().len() != file_len {
        return Err(StreamError::Truncated);
    }
    if file_hash(mapped.as_slice()) != *vol_id {
        return Err(StreamError::HashMismatch);
    }
    let (_lic, bytes) = kcas_blob(mapped.as_slice(), id)?;
    if blob_id_of(&bytes) != id {
        return Err(StreamError::BlobIdMismatch);
    }
    Ok(Arc::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use klotho_core::{AabbMm, IVec3, LocusKind};
    use klotho_prove::LicenseSpan;
    use klotho_world::PlaceRow;

    use crate::codec::{
        CatalogDesc, KcasEntry, PlaceRef, VolumeRef, decode_catalog, encode_catalog, encode_kcas,
        encode_place_shard, file_hash, place_snap_from_bytes,
    };

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    fn mit() -> LicenseSpan {
        LicenseSpan::spdx("MIT", "").unwrap()
    }

    fn temp_dir() -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "klotho-stream-cat-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_pack(dir: &Path, snap: &PlaceSnap, blob_bytes: &[u8]) -> (PathBuf, BlobId) {
        let shard = encode_place_shard(snap).unwrap();
        let shard_name = "place-01.kplc";
        fs::write(dir.join(shard_name), &shard).unwrap();
        let vol = encode_kcas(&[KcasEntry {
            license: mit(),
            bytes: blob_bytes,
        }])
        .unwrap();
        let vol_name = "vol-0000.kcas";
        fs::write(dir.join(vol_name), &vol).unwrap();
        let blob_id = blob_id_of(blob_bytes);
        let desc = CatalogDesc {
            canon_hash: snap.canon_hash,
            volumes: vec![VolumeRef {
                id: file_hash(&vol),
                filename: vol_name.into(),
            }],
            places: vec![PlaceRef {
                place: snap.place,
                aabb: AabbMm::from_point(IVec3::ZERO),
                shard_id: file_hash(&shard),
                filename: shard_name.into(),
                prefix: snap.prefix,
            }],
            blobs: vec![(blob_id, file_hash(&vol))],
        };
        let cat = encode_catalog(1, &desc).unwrap();
        let cat_path = dir.join("catalog.kwrp");
        fs::write(&cat_path, cat).unwrap();
        (cat_path, blob_id)
    }

    fn sample_snap() -> PlaceSnap {
        let p = place(1);
        PlaceSnap::new(
            p,
            Hash::from_bytes([7; 32]),
            Hash::from_bytes([8; 32]),
            vec![PlaceRow::new(p, LocusKind::Place)],
        )
    }

    #[test]
    fn open_map_place_and_blob_round_trip() {
        let dir = temp_dir();
        let snap = sample_snap();
        let payload = b"hull-bytes";
        let (cat_path, blob_id) = write_pack(&dir, &snap, payload);
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(cat.canon_hash(), snap.canon_hash);
        assert_eq!(
            cat.place_aabb(snap.place),
            Some(AabbMm::from_point(IVec3::ZERO))
        );
        let got = map_place(&cat, snap.place).unwrap();
        assert_eq!(*got, snap);
        let from_bytes =
            place_snap_from_bytes(&fs::read(dir.join("place-01.kplc")).unwrap()).unwrap();
        assert_eq!(from_bytes, snap);
        let b = blob(&cat, blob_id).unwrap();
        assert_eq!(&*b, payload);
        assert_eq!(blob_id_of(&b), blob_id);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_catalog_entry_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(
            map_place(&cat, place(99)).unwrap_err(),
            StreamError::MissingPlace
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_shard_file_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        fs::remove_file(dir.join("place-01.kplc")).unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(
            map_place(&cat, snap.place).unwrap_err(),
            StreamError::MissingShard
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn shard_hash_mismatch_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let mut bytes = fs::read(dir.join("place-01.kplc")).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(dir.join("place-01.kplc"), bytes).unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        let e = map_place(&cat, snap.place).unwrap_err();
        assert!(
            matches!(
                e,
                StreamError::HashMismatch | StreamError::Trailing | StreamError::Truncated
            ),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn place_sigil_mismatch_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let other = PlaceSnap::new(
            place(2),
            snap.canon_hash,
            snap.prefix,
            vec![PlaceRow::new(place(2), LocusKind::Place)],
        );
        fs::write(
            dir.join("place-01.kplc"),
            encode_place_shard(&other).unwrap(),
        )
        .unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(
            map_place(&cat, snap.place).unwrap_err(),
            StreamError::PlaceMismatch
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn canon_hash_mismatch_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let other = PlaceSnap::new(
            snap.place,
            Hash::from_bytes([9; 32]),
            snap.prefix,
            vec![PlaceRow::new(snap.place, LocusKind::Place)],
        );
        fs::write(
            dir.join("place-01.kplc"),
            encode_place_shard(&other).unwrap(),
        )
        .unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        let e = map_place(&cat, snap.place).unwrap_err();
        assert!(
            matches!(
                e,
                StreamError::CanonHashMismatch | StreamError::HashMismatch
            ),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversize_catalog_refused_before_read() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let e = StreamCatalog::open_capped(&cat_path, 8).unwrap_err();
        assert!(matches!(e, StreamError::Oversize { cap: 8, .. }), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncated_shard_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let bytes = fs::read(dir.join("place-01.kplc")).unwrap();
        fs::write(dir.join("place-01.kplc"), &bytes[..PLACE_HEADER_LEN / 2]).unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(
            map_place(&cat, snap.place).unwrap_err(),
            StreamError::Truncated
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_kplc_magic_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let mut bytes = fs::read(dir.join("place-01.kplc")).unwrap();
        bytes[0] = b'X';
        fs::write(dir.join("place-01.kplc"), bytes).unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(map_place(&cat, snap.place).unwrap_err(), StreamError::Magic);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefix_mismatch_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let other = PlaceSnap::new(
            snap.place,
            snap.canon_hash,
            Hash::from_bytes([99; 32]),
            vec![PlaceRow::new(snap.place, LocusKind::Place)],
        );
        fs::write(
            dir.join("place-01.kplc"),
            encode_place_shard(&other).unwrap(),
        )
        .unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(
            map_place(&cat, snap.place).unwrap_err(),
            StreamError::PrefixMismatch
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_hash_mismatch_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, blob_id) = write_pack(&dir, &snap, b"xyz");
        let mut vol = fs::read(dir.join("vol-0000.kcas")).unwrap();
        vol[12] ^= 1;
        fs::write(dir.join("vol-0000.kcas"), vol).unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(blob(&cat, blob_id).unwrap_err(), StreamError::HashMismatch);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn blob_id_mismatch_with_matching_volume_hash_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let payload = b"xyz";
        let mut vol = encode_kcas(&[KcasEntry {
            license: mit(),
            bytes: payload,
        }])
        .unwrap();
        vol[KCAS_HEADER_LEN] ^= 1;
        let vol_hash = file_hash(&vol);
        fs::write(dir.join("vol-0000.kcas"), &vol).unwrap();
        let shard = encode_place_shard(&snap).unwrap();
        fs::write(dir.join("place-01.kplc"), &shard).unwrap();
        let blob_id = blob_id_of(payload);
        let desc = CatalogDesc {
            canon_hash: snap.canon_hash,
            volumes: vec![VolumeRef {
                id: vol_hash,
                filename: "vol-0000.kcas".into(),
            }],
            places: vec![PlaceRef {
                place: snap.place,
                aabb: AabbMm::from_point(IVec3::ZERO),
                shard_id: file_hash(&shard),
                filename: "place-01.kplc".into(),
                prefix: snap.prefix,
            }],
            blobs: vec![(blob_id, vol_hash)],
        };
        let cat_path = dir.join("catalog.kwrp");
        fs::write(&cat_path, encode_catalog(1, &desc).unwrap()).unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(
            blob(&cat, blob_id).unwrap_err(),
            StreamError::BlobIdMismatch
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_kcas_magic_fails_before_decode() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, blob_id) = write_pack(&dir, &snap, b"xyz");
        let mut vol = fs::read(dir.join("vol-0000.kcas")).unwrap();
        vol[0] = b'X';
        fs::write(dir.join("vol-0000.kcas"), vol).unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        assert_eq!(blob(&cat, blob_id).unwrap_err(), StreamError::Magic);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_duplicate_blob_row_fails() {
        let dir = temp_dir();
        let snap = sample_snap();
        let payload = b"x";
        let vol = encode_kcas(&[KcasEntry {
            license: mit(),
            bytes: payload,
        }])
        .unwrap();
        fs::write(dir.join("vol-0000.kcas"), &vol).unwrap();
        let shard = encode_place_shard(&snap).unwrap();
        fs::write(dir.join("place-01.kplc"), &shard).unwrap();
        let vol_hash = file_hash(&vol);
        let blob_id = blob_id_of(payload);
        let desc = CatalogDesc {
            canon_hash: snap.canon_hash,
            volumes: vec![VolumeRef {
                id: vol_hash,
                filename: "vol-0000.kcas".into(),
            }],
            places: vec![PlaceRef {
                place: snap.place,
                aabb: AabbMm::from_point(IVec3::ZERO),
                shard_id: file_hash(&shard),
                filename: "place-01.kplc".into(),
                prefix: snap.prefix,
            }],
            blobs: vec![(blob_id, vol_hash)],
        };
        let mut bytes = encode_catalog(1, &desc).unwrap();
        // Count is u32 immediately before the one blob row (two 32-byte hashes).
        let n = bytes.len();
        let count_off = n - 4 - 64;
        bytes[count_off..count_off + 4].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(blob_id.as_bytes());
        bytes.extend_from_slice(vol_hash.as_bytes());
        assert_eq!(decode_catalog(&bytes), Err(StreamError::Duplicate));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn declared_oversize_shard_refused_before_map() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let mut hdr = encode_place_shard(&snap).unwrap();
        let payload = klotho_prove::MAX_BLOB_BYTES.saturating_add(1) as u32;
        let off = PLACE_HEADER_LEN - 4;
        hdr[off..off + 4].copy_from_slice(&payload.to_le_bytes());
        hdr.truncate(PLACE_HEADER_LEN);
        fs::write(dir.join("place-01.kplc"), &hdr).unwrap();
        let cat = StreamCatalog::open(&cat_path).unwrap();
        let e = map_place(&cat, snap.place).unwrap_err();
        assert!(
            matches!(e, StreamError::Oversize { cap, .. } if cap == klotho_prove::MAX_BLOB_BYTES),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn header_validate_then_map_matches_from_bytes() {
        let dir = temp_dir();
        let snap = sample_snap();
        let (cat_path, _) = write_pack(&dir, &snap, b"x");
        let cat = StreamCatalog::open(&cat_path).unwrap();
        let mapped = map_place(&cat, snap.place).unwrap();
        let disk = fs::read(dir.join("place-01.kplc")).unwrap();
        let decoded = place_snap_from_bytes(&disk).unwrap();
        assert_eq!(*mapped, decoded);
        let _ = fs::remove_dir_all(&dir);
    }
}
