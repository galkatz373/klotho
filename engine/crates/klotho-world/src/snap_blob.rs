//! Canonical LE encoding of a [`WorldSnapshot`] projection.

use klotho_core::{
    AabbMm, AffordanceId, BlobId, ConstraintState, Epoch, Hash, IVec3, LocusKind, MAX_LOCI_PROCESS,
    Mm, PhysRequest, PoseMm, ResourceId, Sigil, SimLod, Support, Tick, Vel3, VelFx, YawMd,
};
use klotho_ir::{Channel, Rel};

use crate::error::SnapError;
use crate::proj::RiteMachine;
use crate::world::WorldSnapshot;

/// Snapshot blob magic.
pub const SNAP_MAGIC: [u8; 4] = *b"KSNP";
/// Snapshot blob version. v1 has no constraint table; v2 appends one; v3 preserves action identity, WAIT clock and contact ledger.
pub const SNAP_VERSION: u8 = 3;
/// Oldest readable snapshot version.
pub const SNAP_VERSION_MIN: u8 = 1;
/// Constraint-state rows in one snapshot.
pub const MAX_SNAP_CONSTRAINTS: usize = 512;
/// Encoded projection cap (same numeric gate as a save blob).
pub const SNAP_BLOB_CAP: usize = 64 * 1024 * 1024;
/// Packed-row cap for a snapshot blob.
pub const MAX_SNAP_ROWS: usize = MAX_LOCI_PROCESS;
/// Per-row quantity list cap.
pub const MAX_ROW_QTY: usize = 1_024;
/// Per-row relation list cap.
pub const MAX_ROW_RELS: usize = 1_024;
/// Per-row knows list cap.
pub const MAX_ROW_KNOWS: usize = 1_024;
/// Per-row rite list cap.
pub const MAX_ROW_RITES: usize = 1_024;

/// One packed locus as stored on an epoch / pause snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapRow {
    /// Identity.
    pub sigil: Sigil,
    /// Packed kind.
    pub kind: LocusKind,
    /// Pose, if any.
    pub pose: Option<PoseMm>,
    /// Linear velocity.
    pub vel: Vel3,
    /// Yaw rate, millideg / tick.
    pub yaw_rate: i32,
    /// Pitch rate, millideg / tick.
    pub pitch_rate: i32,
    /// Roll rate, millideg / tick.
    pub roll_rate: i32,
    /// Sleep ticks.
    pub sleep_ticks: u16,
    /// Contact-group id.
    pub island: u16,
    /// Last admitted support.
    pub support: Option<Support>,
    /// Pending `PHYS_REQ`, if any.
    pub phys_req: Option<PhysRequest>,
    /// Seat offset in the parent's yaw frame.
    pub attach_local: Option<IVec3>,
    /// Outgoing relations.
    pub rels: Vec<(Rel, Sigil)>,
    /// Quantity rows.
    pub qty: Vec<(ResourceId, i32)>,
    /// Active rites `(rite_id, machine)`.
    pub rites: Vec<(u16, RiteMachine)>,
    /// Known fact ids.
    pub knows: Vec<u16>,
    /// Local hull AABB.
    pub hull: Option<AabbMm>,
    /// Canonical hull blob.
    pub hull_id: BlobId,
    /// Affordance bits 0..63.
    pub afford: u64,
    /// Simulation LOD.
    pub sim_lod: SimLod,
}

impl SnapRow {
    /// Empty columns for `sigil`.
    #[must_use]
    pub fn new(sigil: Sigil, kind: LocusKind) -> Self {
        Self {
            sigil,
            kind,
            pose: None,
            vel: Vel3::ZERO,
            yaw_rate: 0,
            pitch_rate: 0,
            roll_rate: 0,
            sleep_ticks: 0,
            island: 0,
            support: None,
            phys_req: None,
            attach_local: None,
            rels: Vec::new(),
            qty: Vec::new(),
            rites: Vec::new(),
            knows: Vec::new(),
            hull: None,
            hull_id: BlobId::ZERO,
            afford: 0,
            sim_lod: SimLod::Full,
        }
    }
}

/// Refuse before allocating a blob larger than [`SNAP_BLOB_CAP`].
pub fn check_snap_size(size: usize) -> Result<(), SnapError> {
    if size > SNAP_BLOB_CAP {
        Err(SnapError::Oversize {
            size,
            cap: SNAP_BLOB_CAP,
        })
    } else {
        Ok(())
    }
}

pub(crate) fn encode_snapshot(snap: &WorldSnapshot) -> Result<Vec<u8>, SnapError> {
    let rows = snap.projection().capture_snap_rows();
    let constraints: Vec<(Sigil, ConstraintState)> =
        snap.projection().constraint_states().collect();
    encode_parts(
        snap.epoch,
        snap.tick,
        snap.canon_hash,
        snap.trace_prefix_hash,
        snap.projection().opaque_id(),
        &rows,
        &constraints,
    )
}

pub(crate) fn decode_snapshot(bytes: &[u8]) -> Result<WorldSnapshot, SnapError> {
    let parts = decode_parts(bytes)?;
    WorldSnapshot::from_snap_parts(
        parts.epoch,
        parts.tick,
        parts.canon_hash,
        parts.prefix,
        parts.opaque,
        parts.rows,
        parts.constraints,
    )
}

#[derive(Debug, Eq, PartialEq)]
struct SnapParts {
    epoch: Epoch,
    tick: Tick,
    canon_hash: Hash,
    prefix: Hash,
    opaque: Option<AffordanceId>,
    rows: Vec<SnapRow>,
    constraints: Vec<(Sigil, ConstraintState)>,
}

pub(crate) fn encode_parts(
    epoch: Epoch,
    tick: Tick,
    canon_hash: Hash,
    prefix: Hash,
    opaque: Option<AffordanceId>,
    rows: &[SnapRow],
    constraints: &[(Sigil, ConstraintState)],
) -> Result<Vec<u8>, SnapError> {
    if rows.len() > MAX_SNAP_ROWS {
        return Err(SnapError::Oversize {
            size: rows.len(),
            cap: MAX_SNAP_ROWS,
        });
    }
    if constraints.len() > MAX_SNAP_CONSTRAINTS {
        return Err(SnapError::Oversize {
            size: constraints.len(),
            cap: MAX_SNAP_CONSTRAINTS,
        });
    }
    let mut buf = Vec::new();
    buf.extend_from_slice(&SNAP_MAGIC);
    buf.push(SNAP_VERSION);
    buf.extend_from_slice(&[0, 0, 0]);
    buf.extend_from_slice(&epoch.0.to_le_bytes());
    buf.extend_from_slice(&tick.0.to_le_bytes());
    buf.extend_from_slice(canon_hash.as_bytes());
    buf.extend_from_slice(prefix.as_bytes());
    match opaque {
        None => {
            buf.push(0);
            put_u16(&mut buf, 0);
        }
        Some(id) => {
            buf.push(1);
            put_u16(&mut buf, id.0);
        }
    }
    put_u32(&mut buf, u32_len(rows.len())?);
    for row in rows {
        encode_row(&mut buf, row)?;
        check_snap_size(buf.len())?;
    }
    put_u32(&mut buf, u32_len(constraints.len())?);
    for &(id, state) in constraints {
        buf.extend_from_slice(&id.raw().to_le_bytes());
        put_i32(&mut buf, state.impulse);
        buf.push(u8::from(state.broken));
        check_snap_size(buf.len())?;
    }
    check_snap_size(buf.len())?;
    Ok(buf)
}

fn decode_parts(bytes: &[u8]) -> Result<SnapParts, SnapError> {
    if bytes.len() > SNAP_BLOB_CAP {
        return Err(SnapError::Oversize {
            size: bytes.len(),
            cap: SNAP_BLOB_CAP,
        });
    }
    let mut rest = bytes;
    let magic = take(&mut rest, 4)?;
    if magic != SNAP_MAGIC {
        return Err(SnapError::Magic);
    }
    let version = take_u8(&mut rest)?;
    if !(SNAP_VERSION_MIN..=SNAP_VERSION).contains(&version) {
        return Err(SnapError::Version(version));
    }
    let pad = take(&mut rest, 3)?;
    if pad != [0, 0, 0] {
        return Err(SnapError::Pad);
    }
    let epoch = Epoch(take_u64(&mut rest)?);
    let tick = Tick(take_u64(&mut rest)?);
    let canon_hash = take_hash(&mut rest)?;
    let prefix = take_hash(&mut rest)?;
    let opaque = match take_u8(&mut rest)? {
        0 => {
            let _ = take_u16(&mut rest)?;
            None
        }
        1 => Some(AffordanceId(take_u16(&mut rest)?)),
        _ => return Err(SnapError::Kind),
    };
    let n = take_capped_count(&mut rest, MAX_SNAP_ROWS)?;
    let mut rows = Vec::new();
    for _ in 0..n {
        rows.push(decode_row(&mut rest, version)?);
    }
    let mut constraints = Vec::new();
    if version >= 2 {
        let cn = take_capped_count(&mut rest, MAX_SNAP_CONSTRAINTS)?;
        for _ in 0..cn {
            let id = Sigil::from_raw(take_u128(&mut rest)?);
            let impulse = take_i32(&mut rest)?;
            let broken = match take_u8(&mut rest)? {
                0 => false,
                1 => true,
                _ => return Err(SnapError::Kind),
            };
            constraints.push((id, ConstraintState { impulse, broken }));
        }
    }
    if !rest.is_empty() {
        return Err(SnapError::Trailing);
    }
    Ok(SnapParts {
        epoch,
        tick,
        canon_hash,
        prefix,
        opaque,
        rows,
        constraints,
    })
}

fn encode_row(buf: &mut Vec<u8>, row: &SnapRow) -> Result<(), SnapError> {
    check_row_caps(row)?;
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
    put_u16(buf, row.sleep_ticks);
    put_u16(buf, row.island);
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
    match row.phys_req {
        None => buf.push(0),
        Some(r) => {
            buf.push(1);
            put_ivec3(buf, r.lin);
            put_ivec3(buf, r.ang);
        }
    }
    match row.attach_local {
        None => buf.push(0),
        Some(v) => {
            buf.push(1);
            put_ivec3(buf, v);
        }
    }
    put_u32(buf, u32_len(row.rels.len())?);
    for (rel, s) in &row.rels {
        buf.push(rel.as_u8());
        buf.extend_from_slice(&s.raw().to_le_bytes());
    }
    put_u32(buf, u32_len(row.qty.len())?);
    for (res, v) in &row.qty {
        buf.push(res.0);
        put_i32(buf, *v);
    }
    put_u32(buf, u32_len(row.rites.len())?);
    for (rite, m) in &row.rites {
        put_u16(buf, *rite);
        put_u16(buf, m.pc);
        put_u16(buf, m.wait_left);
        buf.extend_from_slice(&m.started_at.0.to_le_bytes());
        buf.extend_from_slice(&m.wait_at.0.to_le_bytes());
        buf.push(u8::from(m.contact_hit));
        buf.push(m.contact_agency);
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
    put_u32(buf, u32_len(row.knows.len())?);
    for k in &row.knows {
        put_u16(buf, *k);
    }
    match row.hull {
        None => buf.push(0),
        Some(h) => {
            buf.push(1);
            put_aabb(buf, h);
        }
    }
    buf.extend_from_slice(row.hull_id.as_bytes());
    buf.extend_from_slice(&row.afford.to_le_bytes());
    buf.push(row.sim_lod.as_u8());
    Ok(())
}

fn decode_row(rest: &mut &[u8], version: u8) -> Result<SnapRow, SnapError> {
    let sigil = Sigil::from_raw(take_u128(rest)?);
    let kind = LocusKind::from_u8(take_u8(rest)?).ok_or(SnapError::Kind)?;
    let mut row = SnapRow::new(sigil, kind);
    row.pose = match take_u8(rest)? {
        0 => None,
        1 => Some(take_pose(rest)?),
        _ => return Err(SnapError::Kind),
    };
    row.vel = Vel3::new(
        VelFx(take_i32(rest)?),
        VelFx(take_i32(rest)?),
        VelFx(take_i32(rest)?),
    );
    row.yaw_rate = take_i32(rest)?;
    row.pitch_rate = take_i32(rest)?;
    row.roll_rate = take_i32(rest)?;
    row.sleep_ticks = take_u16(rest)?;
    row.island = take_u16(rest)?;
    row.support = match take_u8(rest)? {
        0 => None,
        1 => Some((
            take_i16(rest)?,
            take_i16(rest)?,
            take_i16(rest)?,
            take_i32(rest)?,
        )),
        _ => return Err(SnapError::Kind),
    };
    row.phys_req = match take_u8(rest)? {
        0 => None,
        1 => Some(PhysRequest {
            lin: take_ivec3(rest)?,
            ang: take_ivec3(rest)?,
        }),
        _ => return Err(SnapError::Kind),
    };
    row.attach_local = match take_u8(rest)? {
        0 => None,
        1 => Some(take_ivec3(rest)?),
        _ => return Err(SnapError::Kind),
    };
    let nr = take_capped_count(rest, MAX_ROW_RELS)?;
    for _ in 0..nr {
        let rel = Rel::from_u8(take_u8(rest)?).ok_or(SnapError::Kind)?;
        let s = Sigil::from_raw(take_u128(rest)?);
        row.rels.push((rel, s));
    }
    let nq = take_capped_count(rest, MAX_ROW_QTY)?;
    for _ in 0..nq {
        let res = ResourceId(take_u8(rest)?);
        let v = take_i32(rest)?;
        row.qty.push((res, v));
    }
    let nri = take_capped_count(rest, MAX_ROW_RITES)?;
    for _ in 0..nri {
        let rite = take_u16(rest)?;
        let pc = take_u16(rest)?;
        let wait_left = take_u16(rest)?;
        let started_at = if version >= 3 {
            klotho_core::Tick(take_u64(rest)?)
        } else {
            klotho_core::Tick::ZERO
        };
        let wait_at = if version >= 3 {
            klotho_core::Tick(take_u64(rest)?)
        } else {
            klotho_core::Tick::ZERO
        };
        let contact_hit = if version >= 3 {
            match take_u8(rest)? {
                0 => false,
                1 => true,
                _ => return Err(SnapError::Kind),
            }
        } else {
            false
        };
        let contact_agency = if version >= 3 {
            let v = take_u8(rest)?;
            if v > 4 {
                return Err(SnapError::Kind);
            }
            v
        } else {
            0
        };
        let target = match take_u8(rest)? {
            0 => None,
            1 => Some(Sigil::from_raw(take_u128(rest)?)),
            _ => return Err(SnapError::Kind),
        };
        let wait_ch = match take_u8(rest)? {
            0 => None,
            v => Some(Channel::from_u8(v).ok_or(SnapError::Kind)?),
        };
        row.rites.push((
            rite,
            RiteMachine {
                contact_agency,
                contact_hit,
                started_at,
                wait_at,
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
    row.hull = match take_u8(rest)? {
        0 => None,
        1 => Some(take_aabb(rest)?),
        _ => return Err(SnapError::Kind),
    };
    row.hull_id = take_blob(rest)?;
    row.afford = take_u64(rest)?;
    row.sim_lod = SimLod::from_u8(take_u8(rest)?).ok_or(SnapError::Kind)?;
    Ok(row)
}

pub(crate) fn check_row_caps(row: &SnapRow) -> Result<(), SnapError> {
    if row.qty.len() > MAX_ROW_QTY {
        return Err(SnapError::Oversize {
            size: row.qty.len(),
            cap: MAX_ROW_QTY,
        });
    }
    if row.rels.len() > MAX_ROW_RELS {
        return Err(SnapError::Oversize {
            size: row.rels.len(),
            cap: MAX_ROW_RELS,
        });
    }
    if row.knows.len() > MAX_ROW_KNOWS {
        return Err(SnapError::Oversize {
            size: row.knows.len(),
            cap: MAX_ROW_KNOWS,
        });
    }
    if row.rites.len() > MAX_ROW_RITES {
        return Err(SnapError::Oversize {
            size: row.rites.len(),
            cap: MAX_ROW_RITES,
        });
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

fn take_pose(rest: &mut &[u8]) -> Result<PoseMm, SnapError> {
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

fn take_aabb(rest: &mut &[u8]) -> Result<AabbMm, SnapError> {
    Ok(AabbMm::new(take_ivec3(rest)?, take_ivec3(rest)?))
}

fn put_ivec3(buf: &mut Vec<u8>, v: IVec3) {
    put_i32(buf, v.x);
    put_i32(buf, v.y);
    put_i32(buf, v.z);
}

fn take_ivec3(rest: &mut &[u8]) -> Result<IVec3, SnapError> {
    Ok(IVec3 {
        x: take_i32(rest)?,
        y: take_i32(rest)?,
        z: take_i32(rest)?,
    })
}

fn u32_len(n: usize) -> Result<u32, SnapError> {
    u32::try_from(n).map_err(|_| SnapError::Oversize {
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

fn put_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], SnapError> {
    if rest.len() < n {
        return Err(SnapError::Truncated);
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_arr<const N: usize>(rest: &mut &[u8]) -> Result<[u8; N], SnapError> {
    let s = take(rest, N)?;
    let mut a = [0u8; N];
    a.copy_from_slice(s);
    Ok(a)
}

fn take_u8(rest: &mut &[u8]) -> Result<u8, SnapError> {
    Ok(take_arr::<1>(rest)?[0])
}

fn take_u16(rest: &mut &[u8]) -> Result<u16, SnapError> {
    Ok(u16::from_le_bytes(take_arr::<2>(rest)?))
}

fn take_i16(rest: &mut &[u8]) -> Result<i16, SnapError> {
    Ok(i16::from_le_bytes(take_arr::<2>(rest)?))
}

fn take_u32(rest: &mut &[u8]) -> Result<u32, SnapError> {
    Ok(u32::from_le_bytes(take_arr::<4>(rest)?))
}

fn take_i32(rest: &mut &[u8]) -> Result<i32, SnapError> {
    Ok(i32::from_le_bytes(take_arr::<4>(rest)?))
}

fn take_u64(rest: &mut &[u8]) -> Result<u64, SnapError> {
    Ok(u64::from_le_bytes(take_arr::<8>(rest)?))
}

fn take_u128(rest: &mut &[u8]) -> Result<u128, SnapError> {
    Ok(u128::from_le_bytes(take_arr::<16>(rest)?))
}

fn take_hash(rest: &mut &[u8]) -> Result<Hash, SnapError> {
    Ok(Hash::from_bytes(take_arr::<32>(rest)?))
}

fn take_blob(rest: &mut &[u8]) -> Result<BlobId, SnapError> {
    Ok(BlobId::from_bytes(take_arr::<32>(rest)?))
}

fn take_capped_count(rest: &mut &[u8], max: usize) -> Result<usize, SnapError> {
    let n = take_u32(rest)? as usize;
    if n > max {
        return Err(SnapError::Oversize { size: n, cap: max });
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::LocusKind;

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn header(epoch: Epoch, tick: Tick, canon: Hash, prefix: Hash, row_count: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&SNAP_MAGIC);
        b.push(SNAP_VERSION);
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(&epoch.0.to_le_bytes());
        b.extend_from_slice(&tick.0.to_le_bytes());
        b.extend_from_slice(canon.as_bytes());
        b.extend_from_slice(prefix.as_bytes());
        b.push(0);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&row_count.to_le_bytes());
        b
    }

    #[test]
    fn snap_cap_constant() {
        assert_eq!(SNAP_BLOB_CAP, 64 * 1024 * 1024);
        assert_eq!(
            check_snap_size(SNAP_BLOB_CAP + 1),
            Err(SnapError::Oversize {
                size: SNAP_BLOB_CAP + 1,
                cap: SNAP_BLOB_CAP,
            })
        );
        assert_eq!(check_snap_size(SNAP_BLOB_CAP), Ok(()));
    }

    #[test]
    fn encode_calls_check_snap_size() {
        let rows = vec![SnapRow::new(relic(1), LocusKind::Relic)];
        let bytes = encode_parts(
            Epoch::ZERO,
            Tick::ZERO,
            Hash::ZERO,
            Hash::ZERO,
            None,
            &rows,
            &[],
        )
        .unwrap();
        assert_eq!(check_snap_size(bytes.len()), Ok(()));
        assert!(bytes.len() < SNAP_BLOB_CAP);
    }

    #[test]
    fn wrong_magic_refused() {
        let mut b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, 0);
        b[0] = b'X';
        assert_eq!(decode_parts(&b), Err(SnapError::Magic));
    }

    #[test]
    fn wrong_version_refused() {
        let mut b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, 0);
        b[4] = 9;
        assert_eq!(decode_parts(&b), Err(SnapError::Version(9)));
    }

    #[test]
    fn wrong_pad_refused() {
        let mut b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, 0);
        b[5] = 1;
        assert_eq!(decode_parts(&b), Err(SnapError::Pad));
    }

    #[test]
    fn trailing_bytes_refused() {
        let mut b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, 0);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.push(0);
        assert_eq!(decode_parts(&b), Err(SnapError::Trailing));
    }

    #[test]
    fn oversize_row_count_refused_before_alloc() {
        let b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, u32::MAX);
        assert_eq!(
            decode_parts(&b),
            Err(SnapError::Oversize {
                size: u32::MAX as usize,
                cap: MAX_SNAP_ROWS,
            })
        );
    }

    #[test]
    fn oversize_qty_list_refused_before_alloc() {
        let mut b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, 1);
        b.extend_from_slice(&relic(1).raw().to_le_bytes());
        b.push(LocusKind::Relic.as_u8());
        b.push(0); // pose
        b.extend_from_slice(&[0u8; 12 + 12]); // vel + rates
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.push(0); // support
        b.push(0); // phys_req
        b.push(0); // attach
        b.extend_from_slice(&0u32.to_le_bytes()); // rels
        b.extend_from_slice(&u32::MAX.to_le_bytes()); // qty
        assert_eq!(
            decode_parts(&b),
            Err(SnapError::Oversize {
                size: u32::MAX as usize,
                cap: MAX_ROW_QTY,
            })
        );
    }

    #[test]
    fn oversize_rel_list_refused_before_alloc() {
        let mut b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, 1);
        b.extend_from_slice(&relic(1).raw().to_le_bytes());
        b.push(LocusKind::Relic.as_u8());
        b.push(0);
        b.extend_from_slice(&[0u8; 24]);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.push(0);
        b.push(0);
        b.push(0);
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode_parts(&b),
            Err(SnapError::Oversize {
                size: u32::MAX as usize,
                cap: MAX_ROW_RELS,
            })
        );
    }

    #[test]
    fn truncated_header_refused() {
        assert_eq!(decode_parts(b"KS"), Err(SnapError::Truncated));
    }

    fn row_prefix() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&relic(1).raw().to_le_bytes());
        b.push(LocusKind::Relic.as_u8());
        b.push(0);
        b.extend_from_slice(&[0u8; 24]);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.push(0);
        b.push(0);
        b.push(0);
        b
    }

    #[test]
    fn oversize_knows_list_refused_before_alloc() {
        let mut b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, 1);
        b.extend_from_slice(&row_prefix());
        b.extend_from_slice(&0u32.to_le_bytes()); // rels
        b.extend_from_slice(&0u32.to_le_bytes()); // qty
        b.extend_from_slice(&0u32.to_le_bytes()); // rites
        b.extend_from_slice(&u32::MAX.to_le_bytes()); // knows
        assert_eq!(
            decode_parts(&b),
            Err(SnapError::Oversize {
                size: u32::MAX as usize,
                cap: MAX_ROW_KNOWS,
            })
        );
    }

    #[test]
    fn oversize_rite_list_refused_before_alloc() {
        let mut b = header(Epoch::ZERO, Tick::ZERO, Hash::ZERO, Hash::ZERO, 1);
        b.extend_from_slice(&row_prefix());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode_parts(&b),
            Err(SnapError::Oversize {
                size: u32::MAX as usize,
                cap: MAX_ROW_RITES,
            })
        );
    }
}
