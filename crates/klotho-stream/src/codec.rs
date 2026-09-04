//! Little-endian catalog / KCAS / KPLC codecs. Caps every length prefix.

use std::collections::BTreeSet;

use klotho_core::{
    AabbMm, BlobId, Hash, IVec3, LocusKind, Mm, PhysRequest, PoseMm, ResourceId, Sigil, SimLod,
    Vel3, VelFx, YawMd,
};
use klotho_ir::{Channel, Rel};
use klotho_prove::{
    CATALOG_CAP, KCAS_VOLUME_CAP, LicenseSpan, MAX_BLOB_BYTES, MAX_BLOBS, PLACE_SHARD_CAP,
    blob_id_of, hash_bytes,
};
use klotho_world::{MAX_PLACE_ROWS, PlaceRow, PlaceSnap, RiteMachine};

use crate::error::StreamError;

/// Catalog magic. Same four bytes as the monolith warp; version 2 is the split.
pub const CATALOG_MAGIC: [u8; 4] = *b"KWRP";
/// Distinct from monolith warp version 1 so `unpack_warp` fails closed.
pub const CATALOG_VERSION: u8 = 2;
/// Catalog kind. Other kinds under version 2 are refused.
pub const CATALOG_KIND: u8 = 1;
/// Cook compiler version stored in the catalog header.
pub const CATALOG_COMPILER: u32 = 1;
/// CAS volume magic.
pub const KCAS_MAGIC: [u8; 4] = *b"KCAS";
/// Volume version.
pub const KCAS_VERSION: u8 = 1;
/// Fixed KCAS prefix: magic, version, pad, blob count.
pub const KCAS_HEADER_LEN: usize = 12;
/// Place shard magic.
pub const KPLC_MAGIC: [u8; 4] = *b"KPLC";
/// Shard version. v2 adds pitch/roll rates, support, attach_local, and rites
/// to each row; v1 shards fail closed with [`StreamError::Version`].
pub const KPLC_VERSION: u8 = 2;
/// Fixed KPLC prefix: magic, version, pad, place, hashes, counts.
pub const PLACE_HEADER_LEN: usize = 96;
/// Places named by one catalog.
pub const CATALOG_MAX_PLACES: usize = 16_384;
/// Volumes named by one catalog.
pub const CATALOG_MAX_VOLUMES: usize = 256;
/// Filename / UTF-8 field cap.
pub const MAX_NAME_BYTES: usize = 256;
/// SPDX / commissioned string cap.
pub const MAX_LICENSE_BYTES: usize = 64 * 1024;
/// Per-row qty list cap.
pub const MAX_ROW_QTY: usize = 1_024;
/// Per-row relation list cap.
pub const MAX_ROW_RELS: usize = 1_024;
/// Per-row knows list cap.
pub const MAX_ROW_KNOWS: usize = 1_024;
/// Per-row active-rite list cap.
pub const MAX_ROW_RITES: usize = 256;

/// One KCAS blob plus the license that must be exportable at cook.
#[derive(Clone, Debug)]
pub struct KcasEntry<'a> {
    /// License recorded at cook. [`LicenseSpan::Unknown`] is refused.
    pub license: LicenseSpan,
    /// Blob bytes. Id is always `blob_id_of(bytes)`.
    pub bytes: &'a [u8],
}

/// Catalog volume row. `id` is blake3 of the volume file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeRef {
    /// Content hash of the `.kcas` file.
    pub id: Hash,
    /// Single path segment, e.g. `vol-0000.kcas`.
    pub filename: String,
}

/// Catalog Place row. `shard_id` is blake3 of the `.kplc` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceRef {
    /// Place locus.
    pub place: Sigil,
    /// Coarse residency AABB, millimetres.
    pub aabb: AabbMm,
    /// Content hash of the shard file.
    pub shard_id: Hash,
    /// Single path segment, e.g. `place-<hex>.kplc`.
    pub filename: String,
    /// Capture prefix stored on the shard.
    pub prefix: Hash,
}

/// Bytes to encode as `catalog.kwrp`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatalogDesc {
    /// Cook digest every shard must match.
    pub canon_hash: Hash,
    /// Volumes in write order (`vol-0000`, …).
    pub volumes: Vec<VolumeRef>,
    /// Places. Encoder sorts by [`Sigil`].
    pub places: Vec<PlaceRef>,
    /// Blob id → volume content hash.
    pub blobs: Vec<(BlobId, Hash)>,
}

/// Parsed KPLC prefix. Payload follows at [`PLACE_HEADER_LEN`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceHeader {
    /// Place this shard belongs to.
    pub place: Sigil,
    /// Cook digest.
    pub canon_hash: Hash,
    /// Capture prefix.
    pub prefix: Hash,
    /// Declared row count. Decoder fails if the payload disagrees.
    pub row_count: u32,
    /// Payload bytes after the header.
    pub payload_size: u32,
}

/// Encode a Place shard (header + payload).
pub fn encode_place_shard(snap: &PlaceSnap) -> Result<Vec<u8>, StreamError> {
    if snap.len() > MAX_PLACE_ROWS {
        return Err(StreamError::RowCount);
    }
    let mut payload = Vec::new();
    for row in snap.rows() {
        encode_row(&mut payload, row)?;
    }
    if payload.len() > MAX_BLOB_BYTES {
        return Err(StreamError::Oversize {
            size: payload.len(),
            cap: MAX_BLOB_BYTES,
        });
    }
    let total = PLACE_HEADER_LEN.saturating_add(payload.len());
    if total > PLACE_SHARD_CAP {
        return Err(StreamError::Oversize {
            size: total,
            cap: PLACE_SHARD_CAP,
        });
    }
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&KPLC_MAGIC);
    buf.push(KPLC_VERSION);
    buf.extend_from_slice(&[0, 0, 0]);
    buf.extend_from_slice(&snap.place.raw().to_le_bytes());
    buf.extend_from_slice(snap.canon_hash.as_bytes());
    buf.extend_from_slice(snap.prefix.as_bytes());
    put_u32(&mut buf, u32_len(snap.len())?);
    put_u32(&mut buf, u32_len(payload.len())?);
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Header-validate `bytes` then decode the payload. No mmap; Miri-safe.
pub fn place_snap_from_bytes(bytes: &[u8]) -> Result<PlaceSnap, StreamError> {
    if bytes.len() > PLACE_SHARD_CAP {
        return Err(StreamError::Oversize {
            size: bytes.len(),
            cap: PLACE_SHARD_CAP,
        });
    }
    if bytes.len() < PLACE_HEADER_LEN {
        return Err(StreamError::Truncated);
    }
    let header = parse_place_header(&bytes[..PLACE_HEADER_LEN])?;
    check_place_sizes(bytes.len(), &header)?;
    decode_place_payload(&bytes[PLACE_HEADER_LEN..], &header)
}

/// Parse the fixed 96-byte KPLC prefix. Does not look at the payload.
pub fn parse_place_header(hdr: &[u8]) -> Result<PlaceHeader, StreamError> {
    if hdr.len() < PLACE_HEADER_LEN {
        return Err(StreamError::Truncated);
    }
    let mut rest = hdr;
    let magic = take(&mut rest, 4)?;
    if magic != KPLC_MAGIC {
        return Err(StreamError::Magic);
    }
    let version = take_u8(&mut rest)?;
    if version != KPLC_VERSION {
        return Err(StreamError::Version(version));
    }
    let pad = take(&mut rest, 3)?;
    if pad != [0, 0, 0] {
        return Err(StreamError::Version(version));
    }
    let place = Sigil::from_raw(take_u128(&mut rest)?);
    let canon_hash = take_hash(&mut rest)?;
    let prefix = take_hash(&mut rest)?;
    let row_count = take_u32(&mut rest)?;
    let payload_size = take_u32(&mut rest)?;
    Ok(PlaceHeader {
        place,
        canon_hash,
        prefix,
        row_count,
        payload_size,
    })
}

/// File length vs header payload size, row cap, payload cap.
pub fn check_place_sizes(file_len: usize, header: &PlaceHeader) -> Result<(), StreamError> {
    if header.row_count as usize > MAX_PLACE_ROWS {
        return Err(StreamError::RowCount);
    }
    let payload = header.payload_size as usize;
    if payload > MAX_BLOB_BYTES {
        return Err(StreamError::Oversize {
            size: payload,
            cap: MAX_BLOB_BYTES,
        });
    }
    let max_payload = PLACE_SHARD_CAP.saturating_sub(PLACE_HEADER_LEN);
    if payload > max_payload {
        return Err(StreamError::Oversize {
            size: payload,
            cap: max_payload,
        });
    }
    let need = PLACE_HEADER_LEN.saturating_add(payload);
    if file_len != need {
        if file_len < need {
            return Err(StreamError::Truncated);
        }
        return Err(StreamError::Trailing);
    }
    Ok(())
}

/// Decode payload rows. `payload.len()` must equal `header.payload_size`.
pub fn decode_place_payload(
    payload: &[u8],
    header: &PlaceHeader,
) -> Result<PlaceSnap, StreamError> {
    if payload.len() != header.payload_size as usize {
        return Err(StreamError::Truncated);
    }
    let n = header.row_count as usize;
    if n > MAX_PLACE_ROWS {
        return Err(StreamError::RowCount);
    }
    let mut rest = payload;
    let mut rows = Vec::new();
    for _ in 0..n {
        rows.push(decode_row(&mut rest)?);
    }
    if !rest.is_empty() {
        return Err(StreamError::Trailing);
    }
    Ok(PlaceSnap::new(
        header.place,
        header.canon_hash,
        header.prefix,
        rows,
    ))
}

/// Encode a KCAS volume. Blob ids are computed from content.
pub fn encode_kcas(entries: &[KcasEntry<'_>]) -> Result<Vec<u8>, StreamError> {
    if entries.len() > MAX_BLOBS {
        return Err(StreamError::Oversize {
            size: entries.len(),
            cap: MAX_BLOBS,
        });
    }
    let mut buf = Vec::new();
    buf.extend_from_slice(&KCAS_MAGIC);
    buf.push(KCAS_VERSION);
    buf.extend_from_slice(&[0, 0, 0]);
    put_u32(&mut buf, u32_len(entries.len())?);
    for e in entries {
        if e.bytes.len() > MAX_BLOB_BYTES {
            return Err(StreamError::Oversize {
                size: e.bytes.len(),
                cap: MAX_BLOB_BYTES,
            });
        }
        let id = blob_id_of(e.bytes);
        buf.extend_from_slice(id.as_bytes());
        encode_license(&mut buf, &e.license)?;
        put_bytes(&mut buf, e.bytes, MAX_BLOB_BYTES)?;
        if buf.len() > KCAS_VOLUME_CAP {
            return Err(StreamError::Oversize {
                size: buf.len(),
                cap: KCAS_VOLUME_CAP,
            });
        }
    }
    Ok(buf)
}

/// Parse the fixed 12-byte KCAS prefix. Caps blob count; does not look at blobs.
pub fn parse_kcas_header(hdr: &[u8]) -> Result<u32, StreamError> {
    if hdr.len() < KCAS_HEADER_LEN {
        return Err(StreamError::Truncated);
    }
    let mut rest = hdr;
    let magic = take(&mut rest, 4)?;
    if magic != KCAS_MAGIC {
        return Err(StreamError::Magic);
    }
    let version = take_u8(&mut rest)?;
    if version != KCAS_VERSION {
        return Err(StreamError::Version(version));
    }
    let pad = take(&mut rest, 3)?;
    if pad != [0, 0, 0] {
        return Err(StreamError::Version(version));
    }
    let n = take_capped_count(&mut rest, MAX_BLOBS)?;
    Ok(n as u32)
}

/// Decode a KCAS volume. Recomputes every blob id.
pub fn decode_kcas(bytes: &[u8]) -> Result<Vec<(BlobId, LicenseSpan, Vec<u8>)>, StreamError> {
    if bytes.len() > KCAS_VOLUME_CAP {
        return Err(StreamError::Oversize {
            size: bytes.len(),
            cap: KCAS_VOLUME_CAP,
        });
    }
    if bytes.len() < 4 {
        return Err(StreamError::Truncated);
    }
    if bytes[..4] != KCAS_MAGIC {
        return Err(StreamError::Magic);
    }
    if bytes.len() < KCAS_HEADER_LEN {
        return Err(StreamError::Truncated);
    }
    let n = parse_kcas_header(&bytes[..KCAS_HEADER_LEN])? as usize;
    let mut rest = &bytes[KCAS_HEADER_LEN..];
    let mut out = Vec::new();
    for _ in 0..n {
        let stored = take_blob(&mut rest)?;
        let license = decode_license(&mut rest)?;
        let blob = take_len_bytes(&mut rest, MAX_BLOB_BYTES)?;
        if blob_id_of(blob) != stored {
            return Err(StreamError::BlobIdMismatch);
        }
        out.push((stored, license, blob.to_vec()));
    }
    if !rest.is_empty() {
        return Err(StreamError::Trailing);
    }
    Ok(out)
}

/// Find `id` in a KCAS volume. Recomputes the id of every blob in the file.
pub fn kcas_blob(bytes: &[u8], id: BlobId) -> Result<(LicenseSpan, Vec<u8>), StreamError> {
    let all = decode_kcas(bytes)?;
    for (got, lic, blob) in all {
        if got == id {
            return Ok((lic, blob));
        }
    }
    Err(StreamError::MissingBlob)
}

/// Encode a version-2 catalog. `unpack_warp` treats this as a bad version.
pub fn encode_catalog(compiler: u32, desc: &CatalogDesc) -> Result<Vec<u8>, StreamError> {
    if desc.volumes.len() > CATALOG_MAX_VOLUMES {
        return Err(StreamError::Oversize {
            size: desc.volumes.len(),
            cap: CATALOG_MAX_VOLUMES,
        });
    }
    if desc.places.len() > CATALOG_MAX_PLACES {
        return Err(StreamError::Oversize {
            size: desc.places.len(),
            cap: CATALOG_MAX_PLACES,
        });
    }
    if desc.blobs.len() > MAX_BLOBS {
        return Err(StreamError::Oversize {
            size: desc.blobs.len(),
            cap: MAX_BLOBS,
        });
    }
    let mut places = desc.places.clone();
    places.sort_by_key(|p| p.place);
    let mut names = BTreeSet::new();
    let mut place_ids = BTreeSet::new();
    for p in &places {
        if !place_ids.insert(p.place) {
            return Err(StreamError::Duplicate);
        }
        check_filename(&p.filename)?;
        if !names.insert(p.filename.as_str()) {
            return Err(StreamError::Name);
        }
    }
    let mut vol_ids = BTreeSet::new();
    for v in &desc.volumes {
        if !vol_ids.insert(v.id) {
            return Err(StreamError::Duplicate);
        }
        check_filename(&v.filename)?;
        if !names.insert(v.filename.as_str()) {
            return Err(StreamError::Name);
        }
    }
    let mut blobs = desc.blobs.clone();
    blobs.sort_by_key(|(id, _)| *id);
    let mut blob_ids = BTreeSet::new();
    for (id, _) in &blobs {
        if !blob_ids.insert(*id) {
            return Err(StreamError::Duplicate);
        }
    }

    let mut buf = Vec::new();
    buf.extend_from_slice(&CATALOG_MAGIC);
    buf.push(CATALOG_VERSION);
    buf.push(CATALOG_KIND);
    buf.extend_from_slice(&[0, 0]);
    buf.extend_from_slice(&compiler.to_le_bytes());
    buf.extend_from_slice(desc.canon_hash.as_bytes());

    put_u32(&mut buf, u32_len(desc.volumes.len())?);
    for v in &desc.volumes {
        buf.extend_from_slice(v.id.as_bytes());
        put_bytes(&mut buf, v.filename.as_bytes(), MAX_NAME_BYTES)?;
    }
    put_u32(&mut buf, u32_len(places.len())?);
    for p in &places {
        buf.extend_from_slice(&p.place.raw().to_le_bytes());
        put_aabb(&mut buf, p.aabb);
        buf.extend_from_slice(p.shard_id.as_bytes());
        put_bytes(&mut buf, p.filename.as_bytes(), MAX_NAME_BYTES)?;
        buf.extend_from_slice(p.prefix.as_bytes());
    }
    put_u32(&mut buf, u32_len(blobs.len())?);
    for (blob, vol) in &blobs {
        buf.extend_from_slice(blob.as_bytes());
        buf.extend_from_slice(vol.as_bytes());
    }
    if buf.len() > CATALOG_CAP {
        return Err(StreamError::Oversize {
            size: buf.len(),
            cap: CATALOG_CAP,
        });
    }
    Ok(buf)
}

/// Decode a catalog. Caller already applied [`CATALOG_CAP`].
pub fn decode_catalog(bytes: &[u8]) -> Result<(u32, CatalogDesc), StreamError> {
    if bytes.len() > CATALOG_CAP {
        return Err(StreamError::Oversize {
            size: bytes.len(),
            cap: CATALOG_CAP,
        });
    }
    let mut rest = bytes;
    let magic = take(&mut rest, 4)?;
    if magic != CATALOG_MAGIC {
        return Err(StreamError::Magic);
    }
    let version = take_u8(&mut rest)?;
    if version != CATALOG_VERSION {
        return Err(StreamError::Version(version));
    }
    let kind = take_u8(&mut rest)?;
    if kind != CATALOG_KIND {
        return Err(StreamError::Kind);
    }
    let pad = take(&mut rest, 2)?;
    if pad != [0, 0] {
        return Err(StreamError::Version(version));
    }
    let compiler = take_u32(&mut rest)?;
    let canon_hash = take_hash(&mut rest)?;

    let n_vol = take_capped_count(&mut rest, CATALOG_MAX_VOLUMES)?;
    let mut volumes = Vec::new();
    let mut vol_ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for _ in 0..n_vol {
        let id = take_hash(&mut rest)?;
        if !vol_ids.insert(id) {
            return Err(StreamError::Duplicate);
        }
        let filename = take_str(&mut rest, MAX_NAME_BYTES)?.to_string();
        check_filename(&filename)?;
        if !names.insert(filename.clone()) {
            return Err(StreamError::Name);
        }
        volumes.push(VolumeRef { id, filename });
    }

    let n_pl = take_capped_count(&mut rest, CATALOG_MAX_PLACES)?;
    let mut places = Vec::new();
    let mut place_ids = BTreeSet::new();
    for _ in 0..n_pl {
        let place = Sigil::from_raw(take_u128(&mut rest)?);
        if !place_ids.insert(place) {
            return Err(StreamError::Duplicate);
        }
        let aabb = take_aabb(&mut rest)?;
        let shard_id = take_hash(&mut rest)?;
        let filename = take_str(&mut rest, MAX_NAME_BYTES)?.to_string();
        check_filename(&filename)?;
        if !names.insert(filename.clone()) {
            return Err(StreamError::Name);
        }
        let prefix = take_hash(&mut rest)?;
        places.push(PlaceRef {
            place,
            aabb,
            shard_id,
            filename,
            prefix,
        });
    }

    let n_blob = take_capped_count(&mut rest, MAX_BLOBS)?;
    let mut blobs = Vec::new();
    let mut blob_ids = BTreeSet::new();
    for _ in 0..n_blob {
        let blob = take_blob(&mut rest)?;
        if !blob_ids.insert(blob) {
            return Err(StreamError::Duplicate);
        }
        let vol = take_hash(&mut rest)?;
        blobs.push((blob, vol));
    }
    if !rest.is_empty() {
        return Err(StreamError::Trailing);
    }
    Ok((
        compiler,
        CatalogDesc {
            canon_hash,
            volumes,
            places,
            blobs,
        },
    ))
}

/// Content hash of `bytes` (catalog records this for shards and volumes).
#[must_use]
pub fn file_hash(bytes: &[u8]) -> Hash {
    hash_bytes(bytes)
}

fn encode_row(buf: &mut Vec<u8>, row: &PlaceRow) -> Result<(), StreamError> {
    buf.extend_from_slice(&row.sigil.raw().to_le_bytes());
    buf.push(row.kind.as_u8());
    match row.pose {
        None => buf.push(0),
        Some(p) => {
            buf.push(1);
            put_pose(buf, p);
        }
    }
    put_i32(buf, row.vel.x.0);
    put_i32(buf, row.vel.y.0);
    put_i32(buf, row.vel.z.0);
    put_i32(buf, row.yaw_rate);
    put_i32(buf, row.pitch_rate);
    put_i32(buf, row.roll_rate);
    match row.hull {
        None => buf.push(0),
        Some(h) => {
            buf.push(1);
            put_aabb(buf, h);
        }
    }
    buf.extend_from_slice(row.hull_id.as_bytes());
    buf.extend_from_slice(&row.afford.to_le_bytes());
    if row.qty.len() > MAX_ROW_QTY {
        return Err(StreamError::Oversize {
            size: row.qty.len(),
            cap: MAX_ROW_QTY,
        });
    }
    put_u32(buf, u32_len(row.qty.len())?);
    for (res, v) in &row.qty {
        buf.push(res.0);
        put_i32(buf, *v);
    }
    if row.rels.len() > MAX_ROW_RELS {
        return Err(StreamError::Oversize {
            size: row.rels.len(),
            cap: MAX_ROW_RELS,
        });
    }
    put_u32(buf, u32_len(row.rels.len())?);
    for (rel, s) in &row.rels {
        buf.push(rel.as_u8());
        buf.extend_from_slice(&s.raw().to_le_bytes());
    }
    buf.extend_from_slice(&row.island.to_le_bytes());
    buf.extend_from_slice(&row.sleep.to_le_bytes());
    buf.push(row.sim_lod.as_u8());
    match row.phys_req {
        None => buf.push(0),
        Some(r) => {
            buf.push(1);
            put_ivec3(buf, r.lin);
            put_ivec3(buf, r.ang);
        }
    }
    match row.support {
        None => buf.push(0),
        Some((nx, ny, nz, depth)) => {
            buf.push(1);
            put_i16(buf, nx);
            put_i16(buf, ny);
            put_i16(buf, nz);
            put_i32(buf, depth);
        }
    }
    match row.attach_local {
        None => buf.push(0),
        Some(v) => {
            buf.push(1);
            put_ivec3(buf, v);
        }
    }
    if row.rites.len() > MAX_ROW_RITES {
        return Err(StreamError::Oversize {
            size: row.rites.len(),
            cap: MAX_ROW_RITES,
        });
    }
    put_u32(buf, u32_len(row.rites.len())?);
    for (rite, m) in &row.rites {
        put_u16(buf, *rite);
        put_u16(buf, m.pc);
        put_u16(buf, m.wait_left);
        match m.target {
            None => buf.push(0),
            Some(t) => {
                buf.push(1);
                buf.extend_from_slice(&t.raw().to_le_bytes());
            }
        }
        match m.wait_ch {
            None => buf.push(0),
            Some(ch) => buf.push(ch.as_u8()),
        }
    }
    if row.knows.len() > MAX_ROW_KNOWS {
        return Err(StreamError::Oversize {
            size: row.knows.len(),
            cap: MAX_ROW_KNOWS,
        });
    }
    put_u32(buf, u32_len(row.knows.len())?);
    for k in &row.knows {
        buf.extend_from_slice(&k.to_le_bytes());
    }
    Ok(())
}

fn decode_row(rest: &mut &[u8]) -> Result<PlaceRow, StreamError> {
    let sigil = Sigil::from_raw(take_u128(rest)?);
    let kind = LocusKind::from_u8(take_u8(rest)?).ok_or(StreamError::Kind)?;
    let mut row = PlaceRow::new(sigil, kind);
    row.pose = match take_u8(rest)? {
        0 => None,
        1 => Some(take_pose(rest)?),
        _ => return Err(StreamError::Kind),
    };
    row.vel = Vel3::new(
        VelFx(take_i32(rest)?),
        VelFx(take_i32(rest)?),
        VelFx(take_i32(rest)?),
    );
    row.yaw_rate = take_i32(rest)?;
    row.pitch_rate = take_i32(rest)?;
    row.roll_rate = take_i32(rest)?;
    row.hull = match take_u8(rest)? {
        0 => None,
        1 => Some(take_aabb(rest)?),
        _ => return Err(StreamError::Kind),
    };
    row.hull_id = take_blob(rest)?;
    row.afford = take_u64(rest)?;
    let nq = take_capped_count(rest, MAX_ROW_QTY)?;
    for _ in 0..nq {
        let res = ResourceId(take_u8(rest)?);
        let v = take_i32(rest)?;
        row.qty.push((res, v));
    }
    let nr = take_capped_count(rest, MAX_ROW_RELS)?;
    for _ in 0..nr {
        let rel = Rel::from_u8(take_u8(rest)?).ok_or(StreamError::Kind)?;
        let s = Sigil::from_raw(take_u128(rest)?);
        row.rels.push((rel, s));
    }
    row.island = take_u16(rest)?;
    row.sleep = take_u16(rest)?;
    row.sim_lod = SimLod::from_u8(take_u8(rest)?).ok_or(StreamError::Kind)?;
    row.phys_req = match take_u8(rest)? {
        0 => None,
        1 => Some(PhysRequest {
            lin: take_ivec3(rest)?,
            ang: take_ivec3(rest)?,
        }),
        _ => return Err(StreamError::Kind),
    };
    row.support = match take_u8(rest)? {
        0 => None,
        1 => Some((
            take_i16(rest)?,
            take_i16(rest)?,
            take_i16(rest)?,
            take_i32(rest)?,
        )),
        _ => return Err(StreamError::Kind),
    };
    row.attach_local = match take_u8(rest)? {
        0 => None,
        1 => Some(take_ivec3(rest)?),
        _ => return Err(StreamError::Kind),
    };
    let nr_rites = take_capped_count(rest, MAX_ROW_RITES)?;
    for _ in 0..nr_rites {
        let rite = take_u16(rest)?;
        let pc = take_u16(rest)?;
        let wait_left = take_u16(rest)?;
        let target = match take_u8(rest)? {
            0 => None,
            1 => Some(Sigil::from_raw(take_u128(rest)?)),
            _ => return Err(StreamError::Kind),
        };
        let wait_ch = match take_u8(rest)? {
            0 => None,
            v => Some(Channel::from_u8(v).ok_or(StreamError::Kind)?),
        };
        row.rites.push((
            rite,
            RiteMachine {
                pc,
                wait_left,
                target,
                wait_ch,
            },
        ));
    }
    let nk = take_capped_count(rest, MAX_ROW_KNOWS)?;
    for _ in 0..nk {
        row.knows.push(take_u16(rest)?);
    }
    Ok(row)
}

fn encode_license(buf: &mut Vec<u8>, lic: &LicenseSpan) -> Result<(), StreamError> {
    match lic {
        LicenseSpan::Unknown => Err(StreamError::License),
        LicenseSpan::Spdx { id, copyright } => {
            buf.push(1);
            put_bytes(buf, id.as_bytes(), MAX_LICENSE_BYTES)?;
            put_bytes(buf, copyright.as_bytes(), MAX_LICENSE_BYTES)?;
            Ok(())
        }
        LicenseSpan::Commissioned {
            holder,
            contract_hash,
        } => {
            buf.push(2);
            put_bytes(buf, holder.as_bytes(), MAX_LICENSE_BYTES)?;
            buf.extend_from_slice(contract_hash.as_bytes());
            Ok(())
        }
    }
}

fn decode_license(rest: &mut &[u8]) -> Result<LicenseSpan, StreamError> {
    match take_u8(rest)? {
        0 => Err(StreamError::License),
        1 => {
            let id = take_str(rest, MAX_LICENSE_BYTES)?.to_string();
            let copyright = take_str(rest, MAX_LICENSE_BYTES)?.to_string();
            LicenseSpan::spdx(id, copyright).map_err(|_| StreamError::License)
        }
        2 => {
            let holder = take_str(rest, MAX_LICENSE_BYTES)?.to_string();
            let contract_hash = take_hash(rest)?;
            LicenseSpan::commissioned(holder, contract_hash).map_err(|_| StreamError::License)
        }
        _ => Err(StreamError::License),
    }
}

fn check_filename(name: &str) -> Result<(), StreamError> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || name.contains('\0')
    {
        return Err(StreamError::Name);
    }
    Ok(())
}

fn put_pose(buf: &mut Vec<u8>, p: PoseMm) {
    put_i32(buf, p.x.0);
    put_i32(buf, p.y.0);
    put_i32(buf, p.z.0);
    put_i32(buf, p.yaw.0);
    put_i32(buf, p.pitch.0);
    put_i32(buf, p.roll.0);
}

fn take_pose(rest: &mut &[u8]) -> Result<PoseMm, StreamError> {
    let mut p = PoseMm::new(
        Mm(take_i32(rest)?),
        Mm(take_i32(rest)?),
        Mm(take_i32(rest)?),
        YawMd(take_i32(rest)?),
    );
    p.pitch = YawMd(take_i32(rest)?);
    p.roll = YawMd(take_i32(rest)?);
    Ok(p)
}

fn put_aabb(buf: &mut Vec<u8>, a: AabbMm) {
    put_ivec3(buf, a.min);
    put_ivec3(buf, a.max);
}

fn take_aabb(rest: &mut &[u8]) -> Result<AabbMm, StreamError> {
    Ok(AabbMm::new(take_ivec3(rest)?, take_ivec3(rest)?))
}

fn put_ivec3(buf: &mut Vec<u8>, v: IVec3) {
    put_i32(buf, v.x);
    put_i32(buf, v.y);
    put_i32(buf, v.z);
}

fn take_ivec3(rest: &mut &[u8]) -> Result<IVec3, StreamError> {
    Ok(IVec3 {
        x: take_i32(rest)?,
        y: take_i32(rest)?,
        z: take_i32(rest)?,
    })
}

fn u32_len(n: usize) -> Result<u32, StreamError> {
    u32::try_from(n).map_err(|_| StreamError::Oversize {
        size: n,
        cap: u32::MAX as usize,
    })
}

fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_i16(buf: &mut Vec<u8>, v: i16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn take_i16(rest: &mut &[u8]) -> Result<i16, StreamError> {
    Ok(i16::from_le_bytes(take_arr::<2>(rest)?))
}

fn put_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_bytes(buf: &mut Vec<u8>, data: &[u8], cap: usize) -> Result<(), StreamError> {
    if data.len() > cap {
        return Err(StreamError::Oversize {
            size: data.len(),
            cap,
        });
    }
    put_u32(buf, u32_len(data.len())?);
    buf.extend_from_slice(data);
    Ok(())
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], StreamError> {
    if rest.len() < n {
        return Err(StreamError::Truncated);
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_arr<const N: usize>(rest: &mut &[u8]) -> Result<[u8; N], StreamError> {
    let s = take(rest, N)?;
    let mut a = [0u8; N];
    a.copy_from_slice(s);
    Ok(a)
}

fn take_u8(rest: &mut &[u8]) -> Result<u8, StreamError> {
    Ok(take_arr::<1>(rest)?[0])
}

fn take_u16(rest: &mut &[u8]) -> Result<u16, StreamError> {
    Ok(u16::from_le_bytes(take_arr::<2>(rest)?))
}

fn take_u32(rest: &mut &[u8]) -> Result<u32, StreamError> {
    Ok(u32::from_le_bytes(take_arr::<4>(rest)?))
}

fn take_i32(rest: &mut &[u8]) -> Result<i32, StreamError> {
    Ok(i32::from_le_bytes(take_arr::<4>(rest)?))
}

fn take_u64(rest: &mut &[u8]) -> Result<u64, StreamError> {
    Ok(u64::from_le_bytes(take_arr::<8>(rest)?))
}

fn take_u128(rest: &mut &[u8]) -> Result<u128, StreamError> {
    Ok(u128::from_le_bytes(take_arr::<16>(rest)?))
}

fn take_hash(rest: &mut &[u8]) -> Result<Hash, StreamError> {
    Ok(Hash::from_bytes(take_arr::<32>(rest)?))
}

fn take_blob(rest: &mut &[u8]) -> Result<BlobId, StreamError> {
    Ok(BlobId::from_bytes(take_arr::<32>(rest)?))
}

fn take_capped_count(rest: &mut &[u8], max: usize) -> Result<usize, StreamError> {
    let n = take_u32(rest)? as usize;
    if n > max {
        return Err(StreamError::Oversize { size: n, cap: max });
    }
    Ok(n)
}

fn take_len_bytes<'a>(rest: &mut &'a [u8], cap: usize) -> Result<&'a [u8], StreamError> {
    let n = take_u32(rest)? as usize;
    if n > cap {
        return Err(StreamError::Oversize { size: n, cap });
    }
    take(rest, n)
}

fn take_str<'a>(rest: &mut &'a [u8], cap: usize) -> Result<&'a str, StreamError> {
    let b = take_len_bytes(rest, cap)?;
    std::str::from_utf8(b).map_err(|_| StreamError::Name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{IVec3, LocusKind};

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn sample_snap() -> PlaceSnap {
        let p = place(1);
        let mut row = PlaceRow::new(p, LocusKind::Place);
        row.pose = Some(PoseMm::new(Mm(1), Mm(2), Mm(3), YawMd(4)));
        row.vel = Vel3::new(VelFx(5), VelFx(6), VelFx(7));
        row.yaw_rate = 8;
        row.pitch_rate = -9;
        row.roll_rate = 10;
        row.hull = Some(AabbMm::new(
            IVec3 { x: -1, y: 0, z: -1 },
            IVec3 { x: 1, y: 2, z: 1 },
        ));
        row.hull_id = BlobId::from_bytes([9; 32]);
        row.afford = 0x11;
        row.qty = vec![(ResourceId(2), 7)];
        row.rels = vec![(Rel::In, p)];
        row.island = 3;
        row.sleep = 4;
        row.sim_lod = SimLod::Far;
        row.phys_req = Some(PhysRequest {
            lin: IVec3 { x: 1, y: 0, z: 0 },
            ang: IVec3::ZERO,
        });
        row.support = Some((100, 200, -300, 42));
        row.attach_local = Some(IVec3 { x: 7, y: 8, z: 9 });
        row.rites = vec![(
            3,
            RiteMachine {
                pc: 4,
                wait_left: 5,
                target: Some(relic(7)),
                wait_ch: Some(Channel::Timing),
            },
        )];
        row.knows = vec![1, 2];
        let mut other = PlaceRow::new(relic(2), LocusKind::Relic);
        other.rels = vec![(Rel::In, p)];
        PlaceSnap::new(
            p,
            Hash::from_bytes([3; 32]),
            Hash::from_bytes([4; 32]),
            vec![row, other],
        )
    }

    fn mit() -> LicenseSpan {
        LicenseSpan::spdx("MIT", "c").unwrap()
    }

    #[test]
    fn place_shard_round_trips() {
        let snap = sample_snap();
        let bytes = encode_place_shard(&snap).unwrap();
        assert!(bytes.starts_with(&KPLC_MAGIC));
        let got = place_snap_from_bytes(&bytes).unwrap();
        assert_eq!(got, snap);
    }

    #[test]
    fn place_header_then_payload_decode() {
        let snap = sample_snap();
        let bytes = encode_place_shard(&snap).unwrap();
        let header = parse_place_header(&bytes[..PLACE_HEADER_LEN]).unwrap();
        check_place_sizes(bytes.len(), &header).unwrap();
        let got = decode_place_payload(&bytes[PLACE_HEADER_LEN..], &header).unwrap();
        assert_eq!(got, snap);
    }

    #[test]
    fn oversize_row_count_is_refused() {
        let n = MAX_PLACE_ROWS + 1;
        let rows = vec![PlaceRow::new(relic(1), LocusKind::Relic); n];
        let snap = PlaceSnap::new(place(1), Hash::ZERO, Hash::ZERO, rows);
        assert_eq!(encode_place_shard(&snap), Err(StreamError::RowCount));
    }

    #[test]
    fn declared_oversize_payload_refused_before_alloc() {
        let mut hdr = encode_place_shard(&PlaceSnap::new(
            place(1),
            Hash::ZERO,
            Hash::ZERO,
            Vec::new(),
        ))
        .unwrap();
        let payload = MAX_BLOB_BYTES.saturating_add(1) as u32;
        let off = PLACE_HEADER_LEN - 4;
        hdr[off..off + 4].copy_from_slice(&payload.to_le_bytes());
        hdr.truncate(PLACE_HEADER_LEN);
        let e = place_snap_from_bytes(&hdr).unwrap_err();
        assert!(
            matches!(e, StreamError::Oversize { cap, .. } if cap == MAX_BLOB_BYTES),
            "{e}"
        );
    }

    #[test]
    fn truncated_shard_fails() {
        let bytes = encode_place_shard(&sample_snap()).unwrap();
        let e = place_snap_from_bytes(&bytes[..bytes.len() - 1]).unwrap_err();
        assert!(
            matches!(e, StreamError::Truncated | StreamError::Trailing),
            "{e}"
        );
    }

    #[test]
    fn bad_kplc_magic_fails() {
        let mut bytes = encode_place_shard(&sample_snap()).unwrap();
        bytes[0] = b'X';
        assert_eq!(place_snap_from_bytes(&bytes), Err(StreamError::Magic));
    }

    #[test]
    fn kcas_round_trip_recomputes_id() {
        let payload = b"tiny-blob";
        let id = blob_id_of(payload);
        let bytes = encode_kcas(&[KcasEntry {
            license: mit(),
            bytes: payload,
        }])
        .unwrap();
        assert!(bytes.starts_with(&KCAS_MAGIC));
        let got = decode_kcas(&bytes).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, id);
        assert_eq!(got[0].2, payload);
        let (lic, b) = kcas_blob(&bytes, id).unwrap();
        assert_eq!(lic, mit());
        assert_eq!(b, payload);
    }

    #[test]
    fn kcas_blob_id_mismatch_fails() {
        let mut bytes = encode_kcas(&[KcasEntry {
            license: mit(),
            bytes: b"x",
        }])
        .unwrap();
        bytes[12] ^= 1;
        assert_eq!(decode_kcas(&bytes), Err(StreamError::BlobIdMismatch));
    }

    #[test]
    fn kcas_bad_magic_fails() {
        assert_eq!(decode_kcas(b"XXXX"), Err(StreamError::Magic));
        assert_eq!(decode_kcas(b"KWRP"), Err(StreamError::Magic));
        assert_eq!(decode_kcas(b"KPLC"), Err(StreamError::Magic));
    }

    #[test]
    fn kcas_unknown_license_fails() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&KCAS_MAGIC);
        buf.push(KCAS_VERSION);
        buf.extend_from_slice(&[0, 0, 0]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 32]);
        buf.push(0);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.push(b'a');
        assert_eq!(decode_kcas(&buf), Err(StreamError::License));
    }

    #[test]
    fn kcas_oversize_len_prefix_refused_before_alloc() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&KCAS_MAGIC);
        buf.push(KCAS_VERSION);
        buf.extend_from_slice(&[0, 0, 0]);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 32]);
        buf.push(1);
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(b"MIT");
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&(MAX_BLOB_BYTES.saturating_add(1) as u32).to_le_bytes());
        let e = decode_kcas(&buf).unwrap_err();
        assert!(
            matches!(e, StreamError::Oversize { cap, .. } if cap == MAX_BLOB_BYTES),
            "{e}"
        );
    }

    #[test]
    fn catalog_round_trip() {
        let desc = CatalogDesc {
            canon_hash: Hash::from_bytes([1; 32]),
            volumes: vec![VolumeRef {
                id: Hash::from_bytes([2; 32]),
                filename: "vol-0000.kcas".into(),
            }],
            places: vec![PlaceRef {
                place: place(1),
                aabb: AabbMm::from_point(IVec3::ZERO),
                shard_id: Hash::from_bytes([3; 32]),
                filename: "place-01.kplc".into(),
                prefix: Hash::from_bytes([4; 32]),
            }],
            blobs: vec![(BlobId::from_bytes([5; 32]), Hash::from_bytes([2; 32]))],
        };
        let bytes = encode_catalog(1, &desc).unwrap();
        assert!(bytes.starts_with(&CATALOG_MAGIC));
        assert_eq!(bytes[4], CATALOG_VERSION);
        let (compiler, got) = decode_catalog(&bytes).unwrap();
        assert_eq!(compiler, 1);
        assert_eq!(got, desc);
    }

    #[test]
    fn catalog_bad_magic_fails() {
        assert_eq!(decode_catalog(b"KCAS"), Err(StreamError::Magic));
        assert_eq!(decode_catalog(b"KPLC"), Err(StreamError::Magic));
    }

    #[test]
    fn catalog_v1_is_bad_version() {
        let mut bytes = encode_catalog(1, &CatalogDesc::default()).unwrap();
        bytes[4] = 1;
        assert_eq!(decode_catalog(&bytes), Err(StreamError::Version(1)));
    }

    #[test]
    fn catalog_oversize_place_count_refused() {
        let mut bytes = encode_catalog(1, &CatalogDesc::default()).unwrap();
        let n = bytes.len();
        bytes[n - 8..n - 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let e = decode_catalog(&bytes).unwrap_err();
        assert!(
            matches!(e, StreamError::Oversize { cap, .. } if cap == CATALOG_MAX_PLACES),
            "{e}"
        );
    }

    #[test]
    fn catalog_path_escape_refused() {
        let desc = CatalogDesc {
            canon_hash: Hash::ZERO,
            volumes: vec![VolumeRef {
                id: Hash::ZERO,
                filename: "../x.kcas".into(),
            }],
            places: Vec::new(),
            blobs: Vec::new(),
        };
        assert_eq!(encode_catalog(1, &desc), Err(StreamError::Name));
    }

    #[test]
    fn catalog_duplicate_blob_or_volume_refused() {
        let vol = VolumeRef {
            id: Hash::from_bytes([1; 32]),
            filename: "vol-0000.kcas".into(),
        };
        let blob = BlobId::from_bytes([2; 32]);
        let desc = CatalogDesc {
            canon_hash: Hash::ZERO,
            volumes: vec![vol.clone(), vol.clone()],
            places: Vec::new(),
            blobs: Vec::new(),
        };
        assert_eq!(encode_catalog(1, &desc), Err(StreamError::Duplicate));
        let desc = CatalogDesc {
            canon_hash: Hash::ZERO,
            volumes: vec![vol.clone()],
            places: Vec::new(),
            blobs: vec![(blob, vol.id), (blob, vol.id)],
        };
        assert_eq!(encode_catalog(1, &desc), Err(StreamError::Duplicate));
        let other = VolumeRef {
            id: Hash::from_bytes([3; 32]),
            filename: "vol-0000.kcas".into(),
        };
        let desc = CatalogDesc {
            canon_hash: Hash::ZERO,
            volumes: vec![vol, other],
            places: Vec::new(),
            blobs: Vec::new(),
        };
        assert_eq!(encode_catalog(1, &desc), Err(StreamError::Name));
    }

    #[test]
    fn catalog_decode_duplicate_place_fails() {
        let a = PlaceRef {
            place: place(1),
            aabb: AabbMm::from_point(IVec3::ZERO),
            shard_id: Hash::from_bytes([1; 32]),
            filename: "place-a.kplc".into(),
            prefix: Hash::ZERO,
        };
        let mut b = a.clone();
        b.place = place(2);
        b.filename = "place-b.kplc".into();
        b.shard_id = Hash::from_bytes([2; 32]);
        let desc = CatalogDesc {
            canon_hash: Hash::ZERO,
            volumes: Vec::new(),
            places: vec![a, b],
            blobs: Vec::new(),
        };
        let mut bytes = encode_catalog(1, &desc).unwrap();
        let p1 = place(1).raw().to_le_bytes();
        let p2 = place(2).raw().to_le_bytes();
        let mut replaced = false;
        for i in 0..bytes.len().saturating_sub(16) {
            if bytes[i..i + 16] == p2 {
                bytes[i..i + 16].copy_from_slice(&p1);
                replaced = true;
                break;
            }
        }
        assert!(replaced);
        assert_eq!(decode_catalog(&bytes), Err(StreamError::Duplicate));
    }

    #[test]
    fn truncated_kcas_fails() {
        let bytes = encode_kcas(&[KcasEntry {
            license: mit(),
            bytes: b"x",
        }])
        .unwrap();
        assert_eq!(
            decode_kcas(&bytes[..bytes.len() - 1]),
            Err(StreamError::Truncated)
        );
    }
}
