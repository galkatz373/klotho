//! Compact CAS-instance placement tables (K86).
//!
//! Materialized dressing is a sorted per-Place/zone chunk: instance groups
//! reference shared CAS blobs; repeated transforms are column- and
//! delta-encoded. Runtime streams the result. There is no generation seed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use klotho_core::{BlobId, Hash, IVec3, LocusKind, Sigil, YawMd};
use klotho_ir::Name;
use klotho_prove::{blob_id_of, hash_bytes};

use crate::error::CompileError;

/// Placement chunk magic.
pub const PLACEMENT_MAGIC: [u8; 4] = *b"KPLM";
/// Placement codec version.
pub const PLACEMENT_VERSION: u8 = 1;
/// Maximum records in one Place/zone chunk.
pub const MAX_PLACEMENT_RECORDS: usize = 65_536;
/// Maximum instance groups in one chunk.
pub const MAX_INSTANCE_GROUPS: usize = 4_096;

/// One dressing instance before grouping.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct RawPlacement {
    /// Owning Place name.
    pub place: Name,
    /// Owning zone.
    pub zone: Name,
    /// Mesh blob.
    pub mesh: BlobId,
    /// Material blob.
    pub material: BlobId,
    /// Optional clip blob.
    pub clip: Option<BlobId>,
    /// Variant index.
    pub variant: u16,
    /// Translation, millimetres.
    pub pose: IVec3,
    /// Yaw, millidegrees.
    pub yaw: YawMd,
    /// Uniform scale, permille.
    pub scale_permille: u16,
}

/// Shared binding keyed by content.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct InstanceGroup {
    /// Group identity: hash of the binding.
    pub id: Hash,
    /// Mesh blob.
    pub mesh: BlobId,
    /// Material blob.
    pub material: BlobId,
    /// Optional clip blob.
    pub clip: Option<BlobId>,
    /// Variant index.
    pub variant: u16,
    /// Record count in this chunk.
    pub count: u32,
}

/// One decoded placement record.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PlacementRecord {
    /// Instance group.
    pub group: u16,
    /// Absolute pose.
    pub pose: IVec3,
    /// Absolute yaw.
    pub yaw: YawMd,
    /// Absolute scale.
    pub scale_permille: u16,
}

/// Content-addressed Place/zone placement table.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PlacementChunk {
    /// Place name.
    pub place: Name,
    /// Zone name.
    pub zone: Name,
    /// blake3 of the encoded bytes.
    pub chunk_id: Hash,
    /// Encoded KPLM bytes.
    pub bytes: Vec<u8>,
    /// Groups in encode order.
    pub groups: Vec<InstanceGroup>,
    /// Decoded records (absolute).
    pub records: Vec<PlacementRecord>,
}

/// Materialized world: unique CAS blobs plus per-zone chunks.
#[derive(Clone, Debug, Default)]
pub struct MaterializedWorld {
    /// Unique source blobs. Duplicate bytes share an id.
    pub blobs: BTreeMap<BlobId, Vec<u8>>,
    /// Placement chunks, sorted by (place, zone).
    pub chunks: Vec<PlacementChunk>,
}

/// Place + zone identity for one placement chunk.
pub type ChunkKey = (Name, Name);

/// Incremental write report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacementWrite {
    /// Chunks whose bytes changed.
    pub dirty: Vec<ChunkKey>,
    /// Chunks reused byte-for-byte.
    pub reused: Vec<ChunkKey>,
    /// Written paths in chunk order.
    pub paths: Vec<PathBuf>,
}

/// Deterministic Place sigil from an authoring name.
#[must_use]
pub fn place_sigil(name: &Name) -> Sigil {
    let hash = hash_bytes(name.as_str().as_bytes());
    let mut id_bytes = [0u8; 16];
    id_bytes.copy_from_slice(&hash.0[..16]);
    let id = u128::from_le_bytes(id_bytes) & Sigil::ID_MASK;
    Sigil::pack_truncated(LocusKind::Place, 0, id)
}

/// Materialize dressing. Identical source bytes are stored once. Output is
/// fully expanded placement records; no generation graph is retained.
pub fn materialize(
    placements: &[RawPlacement],
    assets: &[(BlobId, Vec<u8>)],
) -> Result<MaterializedWorld, CompileError> {
    let mut blobs = BTreeMap::new();
    for (id, bytes) in assets {
        let derived = blob_id_of(bytes);
        if derived != *id {
            return Err(CompileError::Placement(format!(
                "blob id mismatch for {id}"
            )));
        }
        if let Some(existing) = blobs.get(id) {
            if existing != bytes {
                return Err(CompileError::Placement(format!("blob collision for {id}")));
            }
        } else {
            blobs.insert(*id, bytes.clone());
        }
    }
    for p in placements {
        for id in [p.mesh, p.material].into_iter().chain(p.clip) {
            if !blobs.contains_key(&id) {
                return Err(CompileError::Placement(format!("missing blob {id}")));
            }
        }
        if p.scale_permille == 0 {
            return Err(CompileError::Placement("scale permille is 0".into()));
        }
    }

    let mut by_chunk: BTreeMap<(Name, Name), Vec<&RawPlacement>> = BTreeMap::new();
    for p in placements {
        by_chunk
            .entry((p.place.clone(), p.zone.clone()))
            .or_default()
            .push(p);
    }
    let mut chunks = Vec::new();
    for ((place, zone), rows) in by_chunk {
        chunks.push(encode_chunk(&place, &zone, &rows)?);
    }
    Ok(MaterializedWorld { blobs, chunks })
}

/// Unique packaged blob count. Duplicate source bytes are not counted twice.
#[must_use]
pub fn unique_blob_count(world: &MaterializedWorld) -> usize {
    world.blobs.len()
}

/// Byte-compare previous and next chunks; only owning (place, zone) dirty.
#[must_use]
pub fn diff_chunks(
    prev: &[PlacementChunk],
    next: &[PlacementChunk],
) -> (Vec<ChunkKey>, Vec<ChunkKey>) {
    let mut prev_map = BTreeMap::new();
    for chunk in prev {
        prev_map.insert((chunk.place.clone(), chunk.zone.clone()), chunk.chunk_id);
    }
    let mut dirty = Vec::new();
    let mut reused = Vec::new();
    let mut seen = BTreeSet::new();
    for chunk in next {
        let key = (chunk.place.clone(), chunk.zone.clone());
        seen.insert(key.clone());
        match prev_map.get(&key) {
            Some(id) if *id == chunk.chunk_id => reused.push(key),
            _ => dirty.push(key),
        }
    }
    for key in prev_map.keys() {
        if !seen.contains(key) {
            dirty.push(key.clone());
        }
    }
    dirty.sort();
    reused.sort();
    (dirty, reused)
}

/// Write `.kplm` files; reuse identical bytes.
pub fn write_placement_chunks(
    dir: &Path,
    chunks: &[PlacementChunk],
) -> Result<PlacementWrite, CompileError> {
    fs::create_dir_all(dir).map_err(|e| CompileError::Io(format!("{}: {e}", dir.display())))?;
    let mut dirty = Vec::new();
    let mut reused = Vec::new();
    let mut paths = Vec::new();
    for chunk in chunks {
        let name = format!(
            "place-{}-{}.kplm",
            sanitize(chunk.place.as_str()),
            sanitize(chunk.zone.as_str())
        );
        let path = dir.join(&name);
        let changed = write_if_changed(&path, &chunk.bytes)?;
        if changed {
            dirty.push((chunk.place.clone(), chunk.zone.clone()));
        } else {
            reused.push((chunk.place.clone(), chunk.zone.clone()));
        }
        paths.push(path);
    }
    Ok(PlacementWrite {
        dirty,
        reused,
        paths,
    })
}

fn sanitize(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() { "_".into() } else { out }
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool, CompileError> {
    match fs::read(path) {
        Ok(existing) if existing == bytes => Ok(false),
        Ok(_) => {
            fs::write(path, bytes)
                .map_err(|e| CompileError::Io(format!("{}: {e}", path.display())))?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::write(path, bytes)
                .map_err(|e| CompileError::Io(format!("{}: {e}", path.display())))?;
            Ok(true)
        }
        Err(e) => Err(CompileError::Io(format!("{}: {e}", path.display()))),
    }
}

fn encode_chunk(
    place: &Name,
    zone: &Name,
    rows: &[&RawPlacement],
) -> Result<PlacementChunk, CompileError> {
    if rows.len() > MAX_PLACEMENT_RECORDS {
        return Err(CompileError::Placement("record cap".into()));
    }
    let mut group_keys: BTreeMap<(BlobId, BlobId, Option<BlobId>, u16), u16> = BTreeMap::new();
    let mut groups = Vec::new();
    let mut tagged = Vec::new();
    for row in rows {
        let key = (row.mesh, row.material, row.clip, row.variant);
        let idx = if let Some(i) = group_keys.get(&key) {
            *i
        } else {
            if groups.len() >= MAX_INSTANCE_GROUPS {
                return Err(CompileError::Placement("group cap".into()));
            }
            let idx = u16::try_from(groups.len())
                .map_err(|_| CompileError::Placement("group cap".into()))?;
            let id = group_id(row);
            groups.push(InstanceGroup {
                id,
                mesh: row.mesh,
                material: row.material,
                clip: row.clip,
                variant: row.variant,
                count: 0,
            });
            group_keys.insert(key, idx);
            idx
        };
        tagged.push((idx, *row));
    }
    tagged.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.pose.x.cmp(&b.1.pose.x))
            .then(a.1.pose.y.cmp(&b.1.pose.y))
            .then(a.1.pose.z.cmp(&b.1.pose.z))
            .then(a.1.yaw.0.cmp(&b.1.yaw.0))
            .then(a.1.scale_permille.cmp(&b.1.scale_permille))
    });
    let mut records = Vec::new();
    let mut group_col = Vec::new();
    let mut dx = Vec::new();
    let mut dy = Vec::new();
    let mut dz = Vec::new();
    let mut dyaw = Vec::new();
    let mut dscale = Vec::new();
    let mut prev: BTreeMap<u16, (IVec3, i32, u16)> = BTreeMap::new();
    for (idx, row) in &tagged {
        groups[*idx as usize].count += 1;
        let (px, py, pz, pyaw, pscale) = match prev.get(idx) {
            Some((pose, yaw, scale)) => (
                row.pose.x.wrapping_sub(pose.x),
                row.pose.y.wrapping_sub(pose.y),
                row.pose.z.wrapping_sub(pose.z),
                row.yaw.0.wrapping_sub(*yaw),
                i32::from(row.scale_permille).wrapping_sub(i32::from(*scale)),
            ),
            None => (
                row.pose.x,
                row.pose.y,
                row.pose.z,
                row.yaw.0,
                i32::from(row.scale_permille),
            ),
        };
        let dscale_i16 = i16::try_from(pscale)
            .map_err(|_| CompileError::Placement("scale delta overflow".into()))?;
        group_col.push(*idx);
        dx.push(px);
        dy.push(py);
        dz.push(pz);
        dyaw.push(pyaw);
        dscale.push(dscale_i16);
        records.push(PlacementRecord {
            group: *idx,
            pose: row.pose,
            yaw: row.yaw,
            scale_permille: row.scale_permille,
        });
        prev.insert(*idx, (row.pose, row.yaw.0, row.scale_permille));
    }

    let mut buf = Vec::new();
    buf.extend_from_slice(&PLACEMENT_MAGIC);
    buf.push(PLACEMENT_VERSION);
    buf.extend_from_slice(&[0, 0, 0]);
    put_name(&mut buf, place)?;
    put_name(&mut buf, zone)?;
    put_u16(&mut buf, u16_len(groups.len())?)?;
    for g in &groups {
        buf.extend_from_slice(g.id.as_bytes());
        buf.extend_from_slice(&g.mesh.0);
        buf.extend_from_slice(&g.material.0);
        match g.clip {
            Some(c) => {
                buf.push(1);
                buf.extend_from_slice(&c.0);
            }
            None => buf.push(0),
        }
        buf.extend_from_slice(&g.variant.to_le_bytes());
        buf.extend_from_slice(&g.count.to_le_bytes());
    }
    put_u32(&mut buf, u32_len(records.len())?)?;
    for v in &group_col {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    for v in &dx {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    for v in &dy {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    for v in &dz {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    for v in &dyaw {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    for v in &dscale {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    let chunk_id = hash_bytes(&buf);
    Ok(PlacementChunk {
        place: place.clone(),
        zone: zone.clone(),
        chunk_id,
        bytes: buf,
        groups,
        records,
    })
}

/// Decode a KPLM chunk.
pub fn decode_chunk(bytes: &[u8]) -> Result<PlacementChunk, CompileError> {
    if bytes.len() < 8 {
        return Err(CompileError::Placement("truncated".into()));
    }
    let mut rest = bytes;
    let magic = take(&mut rest, 4)?;
    if magic != PLACEMENT_MAGIC {
        return Err(CompileError::Placement("magic".into()));
    }
    let version = take_u8(&mut rest)?;
    if version != PLACEMENT_VERSION {
        return Err(CompileError::Placement(format!("version {version}")));
    }
    let _pad = take(&mut rest, 3)?;
    let place = take_name(&mut rest)?;
    let zone = take_name(&mut rest)?;
    let n_groups = usize::from(take_u16(&mut rest)?);
    if n_groups > MAX_INSTANCE_GROUPS {
        return Err(CompileError::Placement("group cap".into()));
    }
    let mut groups = Vec::with_capacity(n_groups);
    for _ in 0..n_groups {
        let id = Hash::from_bytes(take_arr(&mut rest)?);
        let mesh = BlobId(take_arr(&mut rest)?);
        let material = BlobId(take_arr(&mut rest)?);
        let clip = match take_u8(&mut rest)? {
            0 => None,
            1 => Some(BlobId(take_arr(&mut rest)?)),
            _ => return Err(CompileError::Placement("clip flag".into())),
        };
        let variant = take_u16(&mut rest)?;
        let count = take_u32(&mut rest)?;
        groups.push(InstanceGroup {
            id,
            mesh,
            material,
            clip,
            variant,
            count,
        });
    }
    let n = take_u32(&mut rest)? as usize;
    if n > MAX_PLACEMENT_RECORDS {
        return Err(CompileError::Placement("record cap".into()));
    }
    let mut group_col = Vec::with_capacity(n);
    for _ in 0..n {
        group_col.push(take_u16(&mut rest)?);
    }
    let mut dx = Vec::with_capacity(n);
    let mut dy = Vec::with_capacity(n);
    let mut dz = Vec::with_capacity(n);
    let mut dyaw = Vec::with_capacity(n);
    let mut dscale = Vec::with_capacity(n);
    for _ in 0..n {
        dx.push(take_i32(&mut rest)?);
    }
    for _ in 0..n {
        dy.push(take_i32(&mut rest)?);
    }
    for _ in 0..n {
        dz.push(take_i32(&mut rest)?);
    }
    for _ in 0..n {
        dyaw.push(take_i32(&mut rest)?);
    }
    for _ in 0..n {
        dscale.push(take_i16(&mut rest)?);
    }
    if !rest.is_empty() {
        return Err(CompileError::Placement("trailing bytes".into()));
    }
    let mut prev: BTreeMap<u16, (IVec3, i32, u16)> = BTreeMap::new();
    let mut records = Vec::with_capacity(n);
    for i in 0..n {
        let g = group_col[i];
        if usize::from(g) >= groups.len() {
            return Err(CompileError::Placement("group index".into()));
        }
        let (pose, yaw, scale) = match prev.get(&g) {
            Some((p, y, s)) => {
                let pose = IVec3 {
                    x: p.x.wrapping_add(dx[i]),
                    y: p.y.wrapping_add(dy[i]),
                    z: p.z.wrapping_add(dz[i]),
                };
                let yaw = y.wrapping_add(dyaw[i]);
                let scale = u16::try_from(i32::from(*s).wrapping_add(i32::from(dscale[i])))
                    .map_err(|_| CompileError::Placement("scale".into()))?;
                (pose, yaw, scale)
            }
            None => {
                let pose = IVec3 {
                    x: dx[i],
                    y: dy[i],
                    z: dz[i],
                };
                let yaw = dyaw[i];
                let scale = u16::try_from(i32::from(dscale[i]))
                    .map_err(|_| CompileError::Placement("scale".into()))?;
                (pose, yaw, scale)
            }
        };
        records.push(PlacementRecord {
            group: g,
            pose,
            yaw: YawMd(yaw),
            scale_permille: scale,
        });
        prev.insert(g, (pose, yaw, scale));
    }
    Ok(PlacementChunk {
        place,
        zone,
        chunk_id: hash_bytes(bytes),
        bytes: bytes.to_vec(),
        groups,
        records,
    })
}

fn group_id(row: &RawPlacement) -> Hash {
    let mut buf = Vec::new();
    buf.extend_from_slice(&row.mesh.0);
    buf.extend_from_slice(&row.material.0);
    match row.clip {
        Some(c) => {
            buf.push(1);
            buf.extend_from_slice(&c.0);
        }
        None => buf.push(0),
    }
    buf.extend_from_slice(&row.variant.to_le_bytes());
    hash_bytes(&buf)
}

fn put_name(buf: &mut Vec<u8>, name: &Name) -> Result<(), CompileError> {
    let bytes = name.as_str().as_bytes();
    put_u16(buf, u16_len(bytes.len())?)?;
    buf.extend_from_slice(bytes);
    Ok(())
}

fn put_u16(buf: &mut Vec<u8>, v: u16) -> Result<(), CompileError> {
    buf.extend_from_slice(&v.to_le_bytes());
    Ok(())
}

fn put_u32(buf: &mut Vec<u8>, v: u32) -> Result<(), CompileError> {
    buf.extend_from_slice(&v.to_le_bytes());
    Ok(())
}

fn u16_len(n: usize) -> Result<u16, CompileError> {
    u16::try_from(n).map_err(|_| CompileError::Placement("u16 length".into()))
}

fn u32_len(n: usize) -> Result<u32, CompileError> {
    u32::try_from(n).map_err(|_| CompileError::Placement("u32 length".into()))
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], CompileError> {
    if rest.len() < n {
        return Err(CompileError::Placement("truncated".into()));
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_u8(rest: &mut &[u8]) -> Result<u8, CompileError> {
    Ok(take(rest, 1)?[0])
}

fn take_u16(rest: &mut &[u8]) -> Result<u16, CompileError> {
    let b = take(rest, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn take_u32(rest: &mut &[u8]) -> Result<u32, CompileError> {
    let b = take(rest, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn take_i32(rest: &mut &[u8]) -> Result<i32, CompileError> {
    Ok(take_u32(rest)? as i32)
}

fn take_i16(rest: &mut &[u8]) -> Result<i16, CompileError> {
    Ok(take_u16(rest)? as i16)
}

fn take_arr<const N: usize>(rest: &mut &[u8]) -> Result<[u8; N], CompileError> {
    let b = take(rest, N)?;
    let mut out = [0u8; N];
    out.copy_from_slice(b);
    Ok(out)
}

fn take_name(rest: &mut &[u8]) -> Result<Name, CompileError> {
    let n = usize::from(take_u16(rest)?);
    let bytes = take(rest, n)?;
    let s = std::str::from_utf8(bytes).map_err(|_| CompileError::Placement("utf8".into()))?;
    Name::new(s).map_err(|e| CompileError::Placement(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(tag: &[u8]) -> (BlobId, Vec<u8>) {
        let bytes = tag.to_vec();
        (blob_id_of(&bytes), bytes)
    }

    fn place(name: &str, zone: &str, mesh: BlobId, mat: BlobId, x: i32) -> RawPlacement {
        RawPlacement {
            place: Name::from(name),
            zone: Name::from(zone),
            mesh,
            material: mat,
            clip: None,
            variant: 0,
            pose: IVec3 { x, y: 0, z: 0 },
            yaw: YawMd(0),
            scale_permille: 1000,
        }
    }

    #[test]
    fn duplicate_source_blobs_are_stored_once() {
        let (mesh, mesh_bytes) = blob(b"tree-mesh");
        let (mat, mat_bytes) = blob(b"bark");
        let world = materialize(
            &[
                place("hub", "dress", mesh, mat, 0),
                place("hub", "dress", mesh, mat, 1000),
                place("combat", "dress", mesh, mat, 0),
            ],
            &[(mesh, mesh_bytes), (mat, mat_bytes)],
        )
        .unwrap();
        assert_eq!(unique_blob_count(&world), 2);
        assert_eq!(world.chunks.len(), 2);
        let hub = world
            .chunks
            .iter()
            .find(|c| c.place.as_str() == "hub")
            .unwrap();
        assert_eq!(hub.groups.len(), 1);
        assert_eq!(hub.records.len(), 2);
        assert_eq!(hub.records[1].pose.x, 1000);
    }

    #[test]
    fn round_trip_is_stable_and_has_no_generation_seed() {
        let (mesh, mesh_bytes) = blob(b"rock");
        let (mat, mat_bytes) = blob(b"stone");
        let world = materialize(
            &[
                place("hub", "dress", mesh, mat, 250),
                place("hub", "dress", mesh, mat, 500),
            ],
            &[(mesh, mesh_bytes), (mat, mat_bytes)],
        )
        .unwrap();
        let decoded = decode_chunk(&world.chunks[0].bytes).unwrap();
        assert_eq!(decoded.records, world.chunks[0].records);
        assert_eq!(decoded.groups, world.chunks[0].groups);
        assert!(!world.chunks[0].bytes.windows(4).any(|w| w == b"SEED"));
        assert_eq!(
            hash_bytes(&world.chunks[0].bytes).to_string(),
            world.chunks[0].chunk_id.to_string()
        );
    }

    #[test]
    fn one_zone_edit_invalidates_only_owning_chunk() {
        let (mesh, mesh_bytes) = blob(b"tree-mesh");
        let (mat, mat_bytes) = blob(b"bark");
        let assets = [(mesh, mesh_bytes.clone()), (mat, mat_bytes.clone())];
        let prev = materialize(
            &[
                place("hub", "dress", mesh, mat, 0),
                place("combat", "dress", mesh, mat, 0),
            ],
            &assets,
        )
        .unwrap();
        let next = materialize(
            &[
                place("hub", "dress", mesh, mat, 0),
                place("combat", "dress", mesh, mat, 40),
            ],
            &assets,
        )
        .unwrap();
        let (dirty, reused) = diff_chunks(&prev.chunks, &next.chunks);
        assert_eq!(dirty, vec![(Name::from("combat"), Name::from("dress"))]);
        assert_eq!(reused, vec![(Name::from("hub"), Name::from("dress"))]);
    }

    #[test]
    fn write_reuses_identical_bytes() {
        let (mesh, mesh_bytes) = blob(b"tree-mesh");
        let (mat, mat_bytes) = blob(b"bark");
        let world = materialize(
            &[place("hub", "dress", mesh, mat, 0)],
            &[(mesh, mesh_bytes), (mat, mat_bytes)],
        )
        .unwrap();
        let dir = std::env::temp_dir().join(format!(
            "klotho-kplm-{}-{}",
            std::process::id(),
            world.chunks[0].chunk_id.0[0]
        ));
        let first = write_placement_chunks(&dir, &world.chunks).unwrap();
        assert_eq!(first.dirty.len(), 1);
        let second = write_placement_chunks(&dir, &world.chunks).unwrap();
        assert!(second.dirty.is_empty());
        assert_eq!(second.reused.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
