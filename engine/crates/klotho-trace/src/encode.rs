//! Canonical little-endian encoding of [`TraceEvent`]. Hashed bytes never go
//! through serde. Tag numbers are frozen: bump [`EVENT_VERSION`] to invalidate.

use klotho_core::{Mm, PoseMm, ResourceId, Sigil, Tick, Vel3, VelFx, YawMd};

use crate::error::TraceError;
use crate::event::{IslandSnap, PoseReason, RelTag, RiteEnd, TraceBody, TraceEvent};

/// Encoding version. Bump ⇒ every prefix hash changes.
pub const EVENT_VERSION: u8 = 1;

const TAG_RITE_BEGAN: u8 = 1;
const TAG_RITE_ADVANCED: u8 = 2;
const TAG_RITE_ENDED: u8 = 3;
const TAG_QTY_CHANGED: u8 = 4;
const TAG_ISLAND_SNAP: u8 = 5;
const TAG_POSE_COMMITTED: u8 = 6;
const TAG_SAVE_REQUESTED: u8 = 7;
const TAG_LEARNED: u8 = 8;
const TAG_REL_ADD: u8 = 9;
const TAG_REL_DEL: u8 = 10;
const TAG_EMITTED: u8 = 11;
const TAG_UTTERED: u8 = 12;
const TAG_PLACE_LOADED: u8 = 13;
const TAG_PLACE_EVICTED: u8 = 14;
const TAG_SPAWNED: u8 = 15;
const TAG_DESPAWNED: u8 = 16;
const TAG_MOTION_CONTACT: u8 = 17;
const TAG_MOTION_AUTHORIZED: u8 = 18;

/// Encode one event to canonical LE bytes.
#[must_use]
pub fn encode_event(e: &TraceEvent) -> Vec<u8> {
    let mut b = Buf::new();
    b.u8(EVENT_VERSION);
    b.u64_le(e.tick.0);
    match &e.body {
        TraceBody::RiteBegan {
            actor,
            rite,
            target,
        } => {
            b.u8(TAG_RITE_BEGAN);
            b.sigil(*actor);
            b.u16_le(*rite);
            b.opt_sigil(*target);
        }
        TraceBody::RiteAdvanced {
            actor,
            rite,
            pc,
            wait_left,
        } => {
            b.u8(TAG_RITE_ADVANCED);
            b.sigil(*actor);
            b.u16_le(*rite);
            b.u16_le(*pc);
            b.u16_le(*wait_left);
        }
        TraceBody::RiteEnded {
            actor,
            rite,
            status,
        } => {
            b.u8(TAG_RITE_ENDED);
            b.sigil(*actor);
            b.u16_le(*rite);
            b.u8(status.as_u8());
        }
        TraceBody::QtyChanged {
            id,
            res,
            to,
            quantum,
        } => {
            b.u8(TAG_QTY_CHANGED);
            b.sigil(*id);
            b.u8(res.0);
            b.i32_le(*to);
            b.i32_le(*quantum);
        }
        TraceBody::IslandSnap(s) => {
            b.u8(TAG_ISLAND_SNAP);
            encode_snap(&mut b, s);
        }
        TraceBody::PoseCommitted { s, pose, reason } => {
            b.u8(TAG_POSE_COMMITTED);
            b.sigil(*s);
            b.i32_le(pose.x.0);
            b.i32_le(pose.z.0);
            b.i32_le(pose.yaw.0);
            b.i32_le(pose.y.0);
            b.i32_le(pose.pitch.0);
            b.i32_le(pose.roll.0);
            b.u8(*reason as u8);
        }
        TraceBody::SaveRequested => b.u8(TAG_SAVE_REQUESTED),
        TraceBody::Learned { mind, fact } => {
            b.u8(TAG_LEARNED);
            b.sigil(*mind);
            b.u16_le(*fact);
        }
        TraceBody::RelAdd { a, rel, b: obj } => {
            b.u8(TAG_REL_ADD);
            b.sigil(*a);
            b.u8(rel.0);
            b.sigil(*obj);
        }
        TraceBody::RelDel { a, rel, b: obj } => {
            b.u8(TAG_REL_DEL);
            b.sigil(*a);
            b.u8(rel.0);
            b.sigil(*obj);
        }
        TraceBody::Emitted { kind, a, b: obj } => {
            b.u8(TAG_EMITTED);
            b.u16_le(*kind);
            b.sigil(*a);
            b.opt_sigil(*obj);
        }
        TraceBody::MotionActionAuthorized {
            actor,
            rite,
            instance,
            channel,
        } => {
            b.u8(TAG_MOTION_AUTHORIZED);
            b.sigil(*actor);
            b.u16_le(*rite);
            b.u64_le(instance.0);
            b.u8(*channel);
        }
        TraceBody::MotionContactAdmitted {
            actor,
            instrument,
            target,
            rite,
            instance,
            channel,
            boundary,
        } => {
            b.u8(TAG_MOTION_CONTACT);
            b.sigil(*actor);
            b.bytes.extend_from_slice(&instrument.0);
            b.sigil(*target);
            b.u16_le(*rite);
            b.u64_le(instance.0);
            b.u8(*channel);
            b.u16_le(*boundary);
        }
        TraceBody::Uttered { speaker, fact_ids } => {
            b.u8(TAG_UTTERED);
            b.sigil(*speaker);
            b.u32_le(fact_ids.len() as u32);
            for f in fact_ids {
                b.u16_le(*f);
            }
        }
        TraceBody::PlaceLoaded { place, n } => {
            b.u8(TAG_PLACE_LOADED);
            b.sigil(*place);
            b.u32_le(*n);
        }
        TraceBody::PlaceEvicted { place } => {
            b.u8(TAG_PLACE_EVICTED);
            b.sigil(*place);
        }
        TraceBody::Spawned {
            template,
            sigil,
            at,
        } => {
            b.u8(TAG_SPAWNED);
            b.u16_le(*template);
            b.sigil(*sigil);
            encode_pose(&mut b, *at);
        }
        TraceBody::Despawned { sigil, generation } => {
            b.u8(TAG_DESPAWNED);
            b.sigil(*sigil);
            b.u8(*generation);
        }
    }
    b.bytes
}

/// Decode one event. Used by tests and later replay loaders.
pub fn decode_event(bytes: &[u8]) -> Result<TraceEvent, TraceError> {
    let mut r = Reader { bytes, pos: 0 };
    let ver = r.u8()?;
    if ver != EVENT_VERSION {
        return Err(TraceError::BadEvent);
    }
    let tick = Tick(r.u64_le()?);
    let tag = r.u8()?;
    let body = match tag {
        TAG_RITE_BEGAN => TraceBody::RiteBegan {
            actor: r.sigil()?,
            rite: r.u16_le()?,
            target: r.opt_sigil()?,
        },
        TAG_RITE_ADVANCED => TraceBody::RiteAdvanced {
            actor: r.sigil()?,
            rite: r.u16_le()?,
            pc: r.u16_le()?,
            wait_left: r.u16_le()?,
        },
        TAG_RITE_ENDED => TraceBody::RiteEnded {
            actor: r.sigil()?,
            rite: r.u16_le()?,
            status: RiteEnd::from_u8(r.u8()?).ok_or(TraceError::BadEvent)?,
        },
        TAG_QTY_CHANGED => TraceBody::QtyChanged {
            id: r.sigil()?,
            res: ResourceId(r.u8()?),
            to: r.i32_le()?,
            quantum: r.i32_le()?,
        },
        TAG_ISLAND_SNAP => TraceBody::IslandSnap(decode_snap(&mut r)?),
        TAG_POSE_COMMITTED => {
            let s = r.sigil()?;
            let x = Mm(r.i32_le()?);
            let z = Mm(r.i32_le()?);
            let yaw = YawMd(r.i32_le()?);
            // Tail 1 = reason only; 13 = y/pitch/roll + reason.
            let (y, pitch, roll) = match r.remaining() {
                13 => (Mm(r.i32_le()?), YawMd(r.i32_le()?), YawMd(r.i32_le()?)),
                1 => (Mm::ZERO, YawMd::ZERO, YawMd::ZERO),
                _ => return Err(TraceError::BadEvent),
            };
            TraceBody::PoseCommitted {
                s,
                pose: PoseMm {
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                    roll,
                },
                reason: match r.u8()? {
                    1 => PoseReason::Interact,
                    2 => PoseReason::Land,
                    3 => PoseReason::Pick,
                    4 => PoseReason::Drop,
                    5 => PoseReason::Hinge,
                    _ => return Err(TraceError::BadEvent),
                },
            }
        }
        TAG_SAVE_REQUESTED => TraceBody::SaveRequested,
        TAG_LEARNED => TraceBody::Learned {
            mind: r.sigil()?,
            fact: r.u16_le()?,
        },
        TAG_REL_ADD => TraceBody::RelAdd {
            a: r.sigil()?,
            rel: RelTag(r.u8()?),
            b: r.sigil()?,
        },
        TAG_REL_DEL => TraceBody::RelDel {
            a: r.sigil()?,
            rel: RelTag(r.u8()?),
            b: r.sigil()?,
        },
        TAG_EMITTED => TraceBody::Emitted {
            kind: r.u16_le()?,
            a: r.sigil()?,
            b: r.opt_sigil()?,
        },
        TAG_MOTION_AUTHORIZED => TraceBody::MotionActionAuthorized {
            actor: r.sigil()?,
            rite: r.u16_le()?,
            instance: Tick(r.u64_le()?),
            channel: r.u8()?,
        },
        TAG_MOTION_CONTACT => TraceBody::MotionContactAdmitted {
            actor: r.sigil()?,
            instrument: klotho_core::Hash(
                r.take(32)?.try_into().map_err(|_| TraceError::BadEvent)?,
            ),
            target: r.sigil()?,
            rite: r.u16_le()?,
            instance: Tick(r.u64_le()?),
            channel: r.u8()?,
            boundary: r.u16_le()?,
        },
        TAG_UTTERED => {
            let speaker = r.sigil()?;
            let n = r.count_capped(2)?;
            let mut fact_ids = Vec::with_capacity(n);
            for _ in 0..n {
                fact_ids.push(r.u16_le()?);
            }
            TraceBody::Uttered { speaker, fact_ids }
        }
        TAG_PLACE_LOADED => TraceBody::PlaceLoaded {
            place: r.sigil()?,
            n: r.u32_le()?,
        },
        TAG_PLACE_EVICTED => TraceBody::PlaceEvicted { place: r.sigil()? },
        TAG_SPAWNED => TraceBody::Spawned {
            template: r.u16_le()?,
            sigil: r.sigil()?,
            at: decode_pose(&mut r, true)?,
        },
        TAG_DESPAWNED => TraceBody::Despawned {
            sigil: r.sigil()?,
            generation: r.u8()?,
        },
        _ => return Err(TraceError::BadEvent),
    };
    if r.pos != r.bytes.len() {
        return Err(TraceError::BadEvent);
    }
    Ok(TraceEvent { tick, body })
}

fn encode_snap(b: &mut Buf, s: &IslandSnap) {
    debug_assert_eq!(s.poses.len(), s.members.len());
    debug_assert_eq!(s.vels.len(), s.members.len());
    debug_assert_eq!(s.yaw_rates.len(), s.members.len());
    debug_assert_eq!(s.sleep_ticks.len(), s.members.len());
    b.u16_le(s.island);
    b.u32_le(s.members.len() as u32);
    for &m in &s.members {
        b.sigil(m);
    }
    for p in &s.poses {
        encode_pose(b, *p);
    }
    for v in &s.vels {
        b.i32_le(v.x.0);
        b.i32_le(v.y.0);
        b.i32_le(v.z.0);
    }
    for &y in &s.yaw_rates {
        b.i32_le(y);
    }
    for &t in &s.sleep_ticks {
        b.u16_le(t);
    }
}

fn decode_snap(r: &mut Reader<'_>) -> Result<IslandSnap, TraceError> {
    let island = r.u16_le()?;
    // Cap uses the v1 per-member floor (sigil + xz-yaw pose + xz vel + rates).
    // Newer rows are larger; the tighter floor still refuses huge `n`.
    let n = r.count_capped(16 + 16 + 8 + 4 + 2)?;
    let mut members = Vec::with_capacity(n);
    for _ in 0..n {
        members.push(r.sigil()?);
    }
    let (wide_pose, wide_vel) = snap_layout(r.remaining(), n)?;
    let mut poses = Vec::with_capacity(n);
    for _ in 0..n {
        poses.push(decode_pose(r, wide_pose)?);
    }
    let mut vels = Vec::with_capacity(n);
    for _ in 0..n {
        vels.push(decode_vel(r, wide_vel)?);
    }
    let mut yaw_rates = Vec::with_capacity(n);
    for _ in 0..n {
        yaw_rates.push(r.i32_le()?);
    }
    let mut sleep_ticks = Vec::with_capacity(n);
    for _ in 0..n {
        sleep_ticks.push(r.u16_le()?);
    }
    IslandSnap::new(island, members, poses, vels, yaw_rates, sleep_ticks)
}

/// v1: pose x/z/y/yaw (16) + vel x/z (8) + yaw_rate (4) + sleep (2) = 30.
/// v2: pose + pitch/roll (24) + vel xyz (12) + yaw_rate (4) + sleep (2) = 42.
fn snap_layout(remaining: usize, n: usize) -> Result<(bool, bool), TraceError> {
    if n == 0 {
        return if remaining == 0 {
            Ok((true, true))
        } else {
            Err(TraceError::BadEvent)
        };
    }
    if remaining % n != 0 {
        return Err(TraceError::BadEvent);
    }
    match remaining / n {
        30 => Ok((false, false)),
        42 => Ok((true, true)),
        _ => Err(TraceError::BadEvent),
    }
}

fn encode_pose(b: &mut Buf, p: PoseMm) {
    b.i32_le(p.x.0);
    b.i32_le(p.z.0);
    b.i32_le(p.y.0);
    b.i32_le(p.yaw.0);
    b.i32_le(p.pitch.0);
    b.i32_le(p.roll.0);
}

fn decode_pose(r: &mut Reader<'_>, wide: bool) -> Result<PoseMm, TraceError> {
    let x = Mm(r.i32_le()?);
    let z = Mm(r.i32_le()?);
    let y = Mm(r.i32_le()?);
    let yaw = YawMd(r.i32_le()?);
    // v1 rows omit pitch/roll; missing axes are 0.
    let (pitch, roll) = if wide {
        (YawMd(r.i32_le()?), YawMd(r.i32_le()?))
    } else {
        (YawMd::ZERO, YawMd::ZERO)
    };
    Ok(PoseMm {
        x,
        y,
        z,
        yaw,
        pitch,
        roll,
    })
}

fn decode_vel(r: &mut Reader<'_>, wide: bool) -> Result<Vel3, TraceError> {
    let x = VelFx(r.i32_le()?);
    if wide {
        let y = VelFx(r.i32_le()?);
        let z = VelFx(r.i32_le()?);
        Ok(Vel3::new(x, y, z))
    } else {
        // v1 rows omit vel.y; missing axis is 0.
        let z = VelFx(r.i32_le()?);
        Ok(Vel3::new(x, VelFx::ZERO, z))
    }
}

struct Buf {
    bytes: Vec<u8>,
}

impl Buf {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }

    fn u16_le(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    fn u32_le(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    fn u64_le(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    fn i32_le(&mut self, v: i32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    fn sigil(&mut self, s: Sigil) {
        self.bytes.extend_from_slice(&s.raw().to_le_bytes());
    }

    fn opt_sigil(&mut self, s: Option<Sigil>) {
        match s {
            None => self.u8(0),
            Some(v) => {
                self.u8(1);
                self.sigil(v);
            }
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], TraceError> {
        let end = self.pos.checked_add(n).ok_or(TraceError::BadEvent)?;
        if end > self.bytes.len() {
            return Err(TraceError::BadEvent);
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, TraceError> {
        Ok(self.take(1)?[0])
    }

    fn u16_le(&mut self) -> Result<u16, TraceError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32_le(&mut self) -> Result<u32, TraceError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn count_capped(&mut self, elem_bytes: usize) -> Result<usize, TraceError> {
        let n = self.u32_le()? as usize;
        // n is untrusted; with_capacity must not see u32::MAX.
        if elem_bytes == 0 || n > self.remaining() / elem_bytes {
            return Err(TraceError::BadEvent);
        }
        Ok(n)
    }

    fn u64_le(&mut self) -> Result<u64, TraceError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn i32_le(&mut self) -> Result<i32, TraceError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn sigil(&mut self) -> Result<Sigil, TraceError> {
        let b = self.take(16)?;
        let mut raw = [0u8; 16];
        raw.copy_from_slice(b);
        Ok(Sigil::from_raw(u128::from_le_bytes(raw)))
    }

    fn opt_sigil(&mut self) -> Result<Option<Sigil>, TraceError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.sigil()?)),
            _ => Err(TraceError::BadEvent),
        }
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{Hash, LocusKind, Mm, PoseMm, YawMd};

    use super::*;
    use crate::event::{PoseReason, ProposalKind, RelTag, RiteEnd, TraceBody};

    fn actor(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, id).unwrap()
    }

    #[test]
    fn rite_began_round_trip() {
        let e = TraceEvent::new(
            Tick(3),
            TraceBody::RiteBegan {
                actor: actor(1),
                rite: 7,
                target: Some(actor(2)),
            },
        );
        let bytes = encode_event(&e);
        assert_eq!(bytes[0], EVENT_VERSION);
        assert_eq!(decode_event(&bytes).unwrap(), e);
    }

    #[test]
    fn le_tick() {
        let e = TraceEvent::new(Tick(1), TraceBody::SaveRequested);
        let bytes = encode_event(&e);
        assert_eq!(&bytes[1..9], &[1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(bytes[9], TAG_SAVE_REQUESTED);
    }

    #[test]
    fn pose_committed_land_still_round_trips() {
        let e = TraceEvent::new(
            Tick(1),
            TraceBody::PoseCommitted {
                s: actor(1),
                pose: PoseMm::new(Mm(1), Mm(0), Mm(2), YawMd(3)),
                reason: PoseReason::Land,
            },
        );
        assert_eq!(decode_event(&encode_event(&e)).unwrap(), e);
    }

    #[test]
    fn island_snap_period_is_two_hz_at_hearth_sixty() {
        assert_eq!(crate::ISLAND_SNAP_PERIOD_TICKS, 30);
    }

    #[test]
    fn island_snap_round_trip() {
        let s = actor(9);
        let mut pose = PoseMm::new(Mm(10), Mm(5), Mm(20), YawMd(7));
        pose.pitch = YawMd(1_000);
        pose.roll = YawMd(2_000);
        let snap = IslandSnap::new(
            4,
            vec![s],
            vec![pose],
            vec![Vel3::new(
                VelFx::ONE,
                VelFx::from_mm_per_tick(3),
                VelFx::ZERO,
            )],
            vec![0],
            vec![3],
        )
        .unwrap();
        let e = TraceEvent::new(Tick(0), TraceBody::IslandSnap(snap));
        let got = decode_event(&encode_event(&e)).unwrap();
        assert_eq!(got, e);
        let TraceBody::IslandSnap(snap) = got.body else {
            panic!("expected snap");
        };
        assert_eq!(snap.poses[0].pitch, YawMd(1_000));
        assert_eq!(snap.poses[0].roll, YawMd(2_000));
        assert_eq!(snap.vels[0].y, VelFx::from_mm_per_tick(3));
    }

    #[test]
    fn island_snap_v1_pose_vel_missing_axes_are_zero() {
        let s = actor(9);
        let mut bytes = vec![EVENT_VERSION];
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.push(TAG_ISLAND_SNAP);
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&s.raw().to_le_bytes());
        for v in [10i32, 20, 0, 0] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&VelFx::ONE.0.to_le_bytes());
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        let e = decode_event(&bytes).unwrap();
        let TraceBody::IslandSnap(snap) = e.body else {
            panic!("expected snap");
        };
        assert_eq!(snap.poses[0].x, Mm(10));
        assert_eq!(snap.poses[0].z, Mm(20));
        assert_eq!(snap.poses[0].pitch, YawMd::ZERO);
        assert_eq!(snap.poses[0].roll, YawMd::ZERO);
        assert_eq!(
            snap.vels[0],
            Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::ONE)
        );
    }

    #[test]
    fn unknown_tag_fails() {
        let bytes = [EVENT_VERSION, 0, 0, 0, 0, 0, 0, 0, 0, 99];
        assert_eq!(decode_event(&bytes), Err(TraceError::BadEvent));
    }

    #[test]
    fn snap_len_mismatch() {
        assert_eq!(
            IslandSnap::new(0, vec![actor(1)], vec![], vec![], vec![], vec![]),
            Err(TraceError::SnapLen)
        );
    }

    #[test]
    fn uttered_count_capped_by_remaining() {
        let mut bytes = vec![EVENT_VERSION];
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.push(TAG_UTTERED);
        bytes.extend_from_slice(&actor(1).raw().to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_event(&bytes), Err(TraceError::BadEvent));
    }

    #[test]
    fn island_snap_count_capped_by_remaining() {
        let mut bytes = vec![EVENT_VERSION];
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.push(TAG_ISLAND_SNAP);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_event(&bytes), Err(TraceError::BadEvent));
    }

    #[test]
    fn pose_committed_six_dof_round_trip() {
        let mut pose = PoseMm::new(Mm(10), Mm(50), Mm(20), YawMd(7));
        pose.pitch = YawMd(1_000);
        pose.roll = YawMd(2_000);
        let e = TraceEvent::new(
            Tick(1),
            TraceBody::PoseCommitted {
                s: actor(1),
                pose,
                reason: PoseReason::Land,
            },
        );
        let got = decode_event(&encode_event(&e)).unwrap();
        assert_eq!(got, e);
        let TraceBody::PoseCommitted { pose: p, .. } = got.body else {
            panic!("expected pose");
        };
        assert_eq!(p.y, Mm(50));
        assert_eq!(p.pitch, YawMd(1_000));
        assert_eq!(p.roll, YawMd(2_000));
    }

    #[test]
    fn pose_committed_old_xz_yaw_missing_axes_are_zero() {
        let s = actor(1);
        let mut bytes = vec![EVENT_VERSION];
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.push(TAG_POSE_COMMITTED);
        bytes.extend_from_slice(&s.raw().to_le_bytes());
        for v in [10i32, 20, 7] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.push(PoseReason::Land as u8);
        let e = decode_event(&bytes).unwrap();
        let TraceBody::PoseCommitted { pose, reason, .. } = e.body else {
            panic!("expected pose");
        };
        assert_eq!(pose.x, Mm(10));
        assert_eq!(pose.z, Mm(20));
        assert_eq!(pose.yaw, YawMd(7));
        assert_eq!(pose.y, Mm::ZERO);
        assert_eq!(pose.pitch, YawMd::ZERO);
        assert_eq!(pose.roll, YawMd::ZERO);
        assert_eq!(reason, PoseReason::Land);
    }

    #[test]
    fn rite_end_evicted_round_trip_unknown_is_bad_event() {
        let e = TraceEvent::new(
            Tick(2),
            TraceBody::RiteEnded {
                actor: actor(1),
                rite: 3,
                status: RiteEnd::Evicted,
            },
        );
        assert_eq!(decode_event(&encode_event(&e)).unwrap(), e);
        assert_eq!(RiteEnd::Evicted.as_u8(), 3);
        let mut bytes = encode_event(&e);
        let last = bytes.len() - 1;
        bytes[last] = 4;
        assert_eq!(decode_event(&bytes), Err(TraceError::BadEvent));
        bytes[last] = 99;
        assert_eq!(decode_event(&bytes), Err(TraceError::BadEvent));
    }

    #[test]
    fn motion_contact_and_authorization_round_trip() {
        let events = [
            TraceEvent::new(
                Tick(7),
                TraceBody::MotionActionAuthorized {
                    actor: actor(1),
                    rite: 3,
                    instance: Tick(6),
                    channel: 2,
                },
            ),
            TraceEvent::new(
                Tick(8),
                TraceBody::MotionContactAdmitted {
                    actor: actor(1),
                    instrument: Hash::from_bytes([9; 32]),
                    target: actor(2),
                    rite: 3,
                    instance: Tick(6),
                    channel: 2,
                    boundary: 1,
                },
            ),
        ];
        for event in events {
            assert_eq!(decode_event(&encode_event(&event)).unwrap(), event);
        }
    }

    #[test]
    fn rel_tags_zero_through_twelve_round_trip() {
        for tag in 0u8..=12 {
            let e = TraceEvent::new(
                Tick(0),
                TraceBody::RelAdd {
                    a: actor(1),
                    rel: RelTag(tag),
                    b: actor(2),
                },
            );
            let got = decode_event(&encode_event(&e)).unwrap();
            assert_eq!(got, e, "rel tag {tag}");
        }
        assert_eq!(RelTag::PILOTED_BY.0, 11);
        assert_eq!(RelTag::ATTACHED_TO.0, 12);
        assert_eq!(RelTag::DEAD.0, 10);
    }

    #[test]
    fn place_spawn_despawn_round_trip() {
        let mut at = PoseMm::new(Mm(1), Mm(2), Mm(3), YawMd(4));
        at.pitch = YawMd(5);
        at.roll = YawMd(6);
        for body in [
            TraceBody::PlaceLoaded {
                place: actor(8),
                n: 12,
            },
            TraceBody::PlaceEvicted { place: actor(8) },
            TraceBody::Spawned {
                template: 7,
                sigil: actor(9),
                at,
            },
            TraceBody::Despawned {
                sigil: actor(9),
                generation: 2,
            },
        ] {
            let e = TraceEvent::new(Tick(4), body);
            assert_eq!(decode_event(&encode_event(&e)).unwrap(), e);
        }
    }

    #[test]
    fn proposal_kind_discriminants() {
        assert_eq!(ProposalKind::Player.as_u8(), 1);
        assert_eq!(ProposalKind::Infer.as_u8(), 5);
        assert_eq!(ProposalKind::Phys.as_u8(), 6);
        assert_eq!(ProposalKind::Residency.as_u8(), 7);
        assert_eq!(ProposalKind::from_u8(6), Some(ProposalKind::Phys));
        assert_eq!(ProposalKind::from_u8(7), Some(ProposalKind::Residency));
        assert_eq!(ProposalKind::from_u8(8), None);
    }
}
