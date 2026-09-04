//! Frozen packets. Canonical little-endian; hashed/signed bytes never go
//! through serde.

use klotho_core::{
    AffordanceId, Epoch, Hash, LawId, PlayerId, PoseMm, RejectReason, ResourceId, Sigil, Tick,
    Vel3, YawMd,
};
use klotho_ir::{Agency, Analog, AssistLevel, Channel, IntentTarget, Name, PlayerIntent, Verb};
use klotho_prove::hash_bytes;
use klotho_trace::{TraceEvent, decode_event, encode_event};

use crate::error::NetError;
use crate::sign::Signed;

/// Maximum framed payload (1 MiB). Length prefixes above this are refused
/// before any allocation from the untrusted count.
pub const MAX_PACKET: usize = 1 << 20;
/// Maximum `TraceEvent`s in one [`Packet::TraceDelta`] (aligns with `MAX_LOCI`).
pub const MAX_EVENTS: usize = 4_096;
/// Maximum [`SnapshotBlob`] bytes. Fits under [`MAX_PACKET`] after headers.
pub const MAX_BLOB: usize = MAX_PACKET - 256;
/// Maximum canonical [`PlayerIntent`] encoding.
pub const MAX_INTENT: usize = 64 * 1024;
/// Maximum [`Packet::PoseDelta`] payload per client per tick.
pub const MAX_POSE_DELTA: usize = 64 * 1024;
/// Maximum Interest places or codebook sigils (`local_ix` is `u16`).
pub const MAX_INTEREST: usize = 65_535;

/// ASCII token whose blake3 is [`CompilerStamp::current`].
pub const STAMP_TOKEN: &[u8] = b"klotho-net/0.2.0";

const TAG_HELLO: u8 = 1;
const TAG_INTENT: u8 = 2;
const TAG_TRACE_DELTA: u8 = 3;
const TAG_NACK: u8 = 4;
const TAG_SNAPSHOT: u8 = 5;
const TAG_INTEREST: u8 = 6;
const TAG_POSE_DELTA: u8 = 7;
const TAG_RESYNC: u8 = 8;

const KIND_FULL: u8 = 0;
const KIND_DELTA: u8 = 1;

const INTENT_VERSION: u8 = 1;

const TARGET_NONE: u8 = 0;
const TARGET_SIGIL: u8 = 1;
const TARGET_NAME: u8 = 2;

const REJ_LAW: u8 = 1;
const REJ_MISSING_AFF: u8 = 2;
const REJ_TIMING: u8 = 3;
const REJ_RESOURCE: u8 = 4;
const REJ_HALLUCINATED: u8 = 5;
const REJ_STALE: u8 = 6;
const REJ_UNCLAIMED: u8 = 7;
const REJ_WITNESS: u8 = 8;
const REJ_WRONG_HULL: u8 = 9;
const REJ_CONFLICT: u8 = 10;
const REJ_BUDGET: u8 = 11;
const REJ_TOO_MANY_ISLANDS: u8 = 12;
const REJ_ISLAND_TOO_LARGE: u8 = 13;
const REJ_RESIDENCY: u8 = 14;
const REJ_EPOCH_MISMATCH: u8 = 15;

/// blake3 of [`STAMP_TOKEN`]. Hello mismatch on stamp, epoch, or canon_hash disconnects.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct CompilerStamp(pub Hash);

impl CompilerStamp {
    /// Stamp for this crate version.
    #[must_use]
    pub fn current() -> Self {
        Self(hash_bytes(STAMP_TOKEN))
    }
}

/// Opaque join/resync blob. Reconstruction uses TraceDelta, not this payload.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct SnapshotBlob(pub Vec<u8>);

/// Ordered Interest codebook. `local_ix` indexes [`Self::sigils`].
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct InterestDict {
    /// Dictionary generation. Wraps mod 65536.
    pub interest_gen: u16,
    /// Interested places.
    pub places: Vec<Sigil>,
    /// Mover codebook. Hot [`Packet::PoseDelta`] carries indexes, not these bytes.
    pub sigils: Vec<Sigil>,
}

/// Full pose row after Resync or an Interest generation change.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PoseFull {
    /// Index into the client's current Interest codebook.
    pub local_ix: u16,
    /// Absolute pose (XYZ millimetres, then yaw/pitch/roll millidegrees).
    pub pose: PoseMm,
    /// Velocity (VelFx raw 16.16 per axis).
    pub vel: Vel3,
}

/// Hot delta row. 12 B `dpose` plus `u16` index; no Sigil.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PoseDeltaEntry {
    /// Index into the client's current Interest codebook.
    pub local_ix: u16,
    /// `(dx, dy, dz, dyaw, dpitch, droll)` vs last applied full-or-delta pose.
    pub dpose: [i16; 6],
}

/// PoseDelta body: Full after Resync or dictionary change, Delta in steady state.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum PoseBlock {
    /// Absolute poses. Idle movers may still appear.
    Full(Vec<PoseFull>),
    /// Millimetre / millidegree deltas. Idle movers omitted.
    Delta(Vec<PoseDeltaEntry>),
}

/// Frozen listen / dedicated-server packet.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum Packet {
    /// Join advertisement. `slot` is 0 on the client request; the reply assigns it.
    Hello {
        /// Cooked Canon identity. Mismatch → disconnect (no replay).
        canon_hash: Hash,
        /// Cook / hull epoch. Mismatch → disconnect (no replay).
        epoch: Epoch,
        /// [`CompilerStamp::current`]. Mismatch → disconnect (no replay).
        build: CompilerStamp,
        /// 32-byte ed25519 verifying key. Truncated or invalid → error, not a default.
        verifying_key: [u8; 32],
        /// Assigned [`PlayerId`] (reply) or 0 (client request).
        slot: PlayerId,
        /// Advertised intent rate. Listen default 20; dedicated may use 30 or 60.
        intent_hz: u8,
    },
    /// Signed PlayerIntent. Host/Server verifies then keeps the latest unconsumed slot.
    Intent {
        /// Signature over the canonical LE intent bytes.
        signed: Signed<PlayerIntent>,
    },
    /// Events committed since `from`. Rejects travel as [`Packet::Nack`].
    TraceDelta {
        /// Parent tick the receiver must currently be at.
        from: Tick,
        /// Interest generation this delta was packed against.
        interest_gen: u16,
        /// Admitted events, commit order.
        events: Vec<TraceEvent>,
    },
    /// Legal reject (K19). Not a prefix-hash input.
    Nack {
        /// Tick of the rejected proposal.
        tick: Tick,
        /// Why it was not committed.
        reason: RejectReason,
    },
    /// Join/resync checkpoint. Prefix mismatch after join is desync.
    Snapshot {
        /// Tick of this checkpoint.
        tick: Tick,
        /// Cook / hull epoch at this checkpoint.
        epoch: Epoch,
        /// Canon identity.
        canon_hash: Hash,
        /// Trace prefix at `tick`.
        trace_prefix_hash: Hash,
        /// Optional place Sigil (`place_present` on the wire).
        place: Option<Sigil>,
        /// Capped opaque bytes; empty is valid.
        blob: SnapshotBlob,
    },
    /// Server→client ordered codebook. Loss or a generation skip is Resync.
    Interest {
        /// Dictionary generation. Wraps mod 65536.
        interest_gen: u16,
        /// Interested places.
        places: Vec<Sigil>,
        /// Mover codebook used by [`Packet::PoseDelta`].
        sigils: Vec<Sigil>,
    },
    /// Unhashed overlay input. Never a Trace prefix input.
    PoseDelta {
        /// Tick these poses were published.
        tick: Tick,
        /// Interest generation this payload indexes.
        interest_gen: u16,
        /// Full or packed delta poses.
        block: PoseBlock,
    },
    /// Recover from Interest generation skip or dictionary loss.
    Resync {
        /// Tick to resume from.
        tick: Tick,
        /// Session epoch.
        epoch: Epoch,
        /// Trace prefix at `tick`.
        prefix: Hash,
    },
}

/// Length-prefix a payload. Refuses payloads larger than [`MAX_PACKET`].
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, NetError> {
    if payload.len() > MAX_PACKET {
        return Err(NetError::Oversize);
    }
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Split a length-prefixed frame. The length is capped before any payload alloc.
pub fn decode_frame(bytes: &[u8]) -> Result<&[u8], NetError> {
    if bytes.len() < 4 {
        return Err(NetError::Truncated);
    }
    let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if len > MAX_PACKET {
        return Err(NetError::Oversize);
    }
    let rest = &bytes[4..];
    if rest.len() < len {
        return Err(NetError::Truncated);
    }
    if rest.len() != len {
        return Err(NetError::Truncated);
    }
    Ok(rest)
}

/// Encode a packet to a payload (no length prefix).
pub fn encode_packet(pkt: &Packet) -> Result<Vec<u8>, NetError> {
    let mut b = Buf::new();
    match pkt {
        Packet::Hello {
            canon_hash,
            epoch,
            build,
            verifying_key,
            slot,
            intent_hz,
        } => {
            b.u8(TAG_HELLO);
            b.hash(*canon_hash);
            b.u64_le(epoch.0);
            b.hash(build.0);
            b.bytes.extend_from_slice(verifying_key);
            b.u8(slot.0);
            b.u8(*intent_hz);
        }
        Packet::Intent { signed } => {
            b.u8(TAG_INTENT);
            b.bytes.extend_from_slice(&signed.signature);
            let payload = encode_player_intent(&signed.value)?;
            if payload.len() > MAX_INTENT {
                return Err(NetError::Oversize);
            }
            b.u32_le(payload.len() as u32);
            b.bytes.extend_from_slice(&payload);
        }
        Packet::TraceDelta {
            from,
            interest_gen,
            events,
        } => {
            if events.len() > MAX_EVENTS {
                return Err(NetError::Oversize);
            }
            b.u8(TAG_TRACE_DELTA);
            b.u64_le(from.0);
            b.u16_le(*interest_gen);
            b.u32_le(events.len() as u32);
            for e in events {
                let enc = encode_event(e);
                b.u32_le(enc.len() as u32);
                b.bytes.extend_from_slice(&enc);
            }
        }
        Packet::Nack { tick, reason } => {
            b.u8(TAG_NACK);
            b.u64_le(tick.0);
            encode_reject(&mut b, *reason);
        }
        Packet::Snapshot {
            tick,
            epoch,
            canon_hash,
            trace_prefix_hash,
            place,
            blob,
        } => {
            if blob.0.len() > MAX_BLOB {
                return Err(NetError::Oversize);
            }
            b.u8(TAG_SNAPSHOT);
            b.u64_le(tick.0);
            b.u64_le(epoch.0);
            b.hash(*canon_hash);
            b.hash(*trace_prefix_hash);
            match place {
                None => b.u8(0),
                Some(s) => {
                    b.u8(1);
                    b.sigil(*s);
                }
            }
            b.u32_le(blob.0.len() as u32);
            b.bytes.extend_from_slice(&blob.0);
        }
        Packet::Interest {
            interest_gen,
            places,
            sigils,
        } => {
            if places.len() > MAX_INTEREST || sigils.len() > MAX_INTEREST {
                return Err(NetError::Oversize);
            }
            b.u8(TAG_INTEREST);
            b.u16_le(*interest_gen);
            b.u32_le(places.len() as u32);
            for s in places {
                b.sigil(*s);
            }
            b.u32_le(sigils.len() as u32);
            for s in sigils {
                b.sigil(*s);
            }
        }
        Packet::PoseDelta {
            tick,
            interest_gen,
            block,
        } => {
            b.u8(TAG_POSE_DELTA);
            b.u64_le(tick.0);
            b.u16_le(*interest_gen);
            encode_pose_block(&mut b, block)?;
            if b.bytes.len() > MAX_POSE_DELTA {
                return Err(NetError::Oversize);
            }
        }
        Packet::Resync {
            tick,
            epoch,
            prefix,
        } => {
            b.u8(TAG_RESYNC);
            b.u64_le(tick.0);
            b.u64_le(epoch.0);
            b.hash(*prefix);
        }
    }
    if b.bytes.len() > MAX_PACKET {
        return Err(NetError::Oversize);
    }
    Ok(b.bytes)
}

/// Decode a packet payload.
pub fn decode_packet(bytes: &[u8]) -> Result<Packet, NetError> {
    if bytes.len() > MAX_PACKET {
        return Err(NetError::Oversize);
    }
    let mut r = Reader { bytes, pos: 0 };
    let tag = r.u8()?;
    let pkt = match tag {
        TAG_HELLO => {
            let canon_hash = r.hash()?;
            let epoch = Epoch(r.u64_le()?);
            let build = CompilerStamp(r.hash()?);
            let mut verifying_key = [0u8; 32];
            verifying_key.copy_from_slice(r.take(32)?);
            let slot = PlayerId(r.u8()?);
            let intent_hz = r.u8()?;
            Packet::Hello {
                canon_hash,
                epoch,
                build,
                verifying_key,
                slot,
                intent_hz,
            }
        }
        TAG_INTENT => {
            let mut signature = [0u8; 64];
            signature.copy_from_slice(r.take(64)?);
            let n = r.u32_capped(MAX_INTENT)?;
            let payload = r.take(n)?;
            let value = decode_player_intent(payload)?;
            Packet::Intent {
                signed: Signed { signature, value },
            }
        }
        TAG_TRACE_DELTA => {
            let from = Tick(r.u64_le()?);
            let interest_gen = r.u16_le()?;
            let n = r.u32_capped(MAX_EVENTS)?;
            let mut events = Vec::new();
            for _ in 0..n {
                let m = r.u32_capped(MAX_PACKET)?;
                let ev = r.take(m)?;
                let e = decode_event(ev).map_err(|_| NetError::BadEvent)?;
                events.push(e);
            }
            Packet::TraceDelta {
                from,
                interest_gen,
                events,
            }
        }
        TAG_NACK => {
            let tick = Tick(r.u64_le()?);
            let reason = decode_reject(&mut r)?;
            Packet::Nack { tick, reason }
        }
        TAG_SNAPSHOT => {
            let tick = Tick(r.u64_le()?);
            let epoch = Epoch(r.u64_le()?);
            let canon_hash = r.hash()?;
            let trace_prefix_hash = r.hash()?;
            let place = match r.u8()? {
                0 => None,
                1 => Some(r.sigil()?),
                _ => return Err(NetError::UnknownTag),
            };
            let n = r.u32_capped(MAX_BLOB)?;
            let blob = SnapshotBlob(r.take(n)?.to_vec());
            Packet::Snapshot {
                tick,
                epoch,
                canon_hash,
                trace_prefix_hash,
                place,
                blob,
            }
        }
        TAG_INTEREST => {
            let interest_gen = r.u16_le()?;
            let np = r.u32_capped(MAX_INTEREST)?;
            let mut places = Vec::new();
            for _ in 0..np {
                places.push(r.sigil()?);
            }
            let ns = r.u32_capped(MAX_INTEREST)?;
            let mut sigils = Vec::new();
            for _ in 0..ns {
                sigils.push(r.sigil()?);
            }
            Packet::Interest {
                interest_gen,
                places,
                sigils,
            }
        }
        TAG_POSE_DELTA => {
            if bytes.len() > MAX_POSE_DELTA {
                return Err(NetError::Oversize);
            }
            let tick = Tick(r.u64_le()?);
            let interest_gen = r.u16_le()?;
            let block = decode_pose_block(&mut r)?;
            Packet::PoseDelta {
                tick,
                interest_gen,
                block,
            }
        }
        TAG_RESYNC => {
            let tick = Tick(r.u64_le()?);
            let epoch = Epoch(r.u64_le()?);
            let prefix = r.hash()?;
            Packet::Resync {
                tick,
                epoch,
                prefix,
            }
        }
        _ => return Err(NetError::UnknownTag),
    };
    if r.pos != r.bytes.len() {
        return Err(NetError::Truncated);
    }
    Ok(pkt)
}

fn encode_pose_block(b: &mut Buf, block: &PoseBlock) -> Result<(), NetError> {
    match block {
        PoseBlock::Full(entries) => {
            if entries.len() > u16::MAX as usize {
                return Err(NetError::Oversize);
            }
            b.u8(KIND_FULL);
            b.u16_le(entries.len() as u16);
            for e in entries {
                b.u16_le(e.local_ix);
                encode_pose_mm(b, &e.pose);
                encode_vel3(b, &e.vel);
            }
        }
        PoseBlock::Delta(entries) => {
            if entries.len() > u16::MAX as usize {
                return Err(NetError::Oversize);
            }
            b.u8(KIND_DELTA);
            b.u16_le(entries.len() as u16);
            for e in entries {
                b.u16_le(e.local_ix);
                for d in e.dpose {
                    b.i16_le(d);
                }
            }
        }
    }
    Ok(())
}

fn decode_pose_block(r: &mut Reader<'_>) -> Result<PoseBlock, NetError> {
    match r.u8()? {
        KIND_FULL => {
            let n = r.u16_le()? as usize;
            let mut entries = Vec::new();
            for _ in 0..n {
                let local_ix = r.u16_le()?;
                let pose = decode_pose_mm(r)?;
                let vel = decode_vel3(r)?;
                entries.push(PoseFull {
                    local_ix,
                    pose,
                    vel,
                });
            }
            Ok(PoseBlock::Full(entries))
        }
        KIND_DELTA => {
            let n = r.u16_le()? as usize;
            let mut entries = Vec::new();
            for _ in 0..n {
                let local_ix = r.u16_le()?;
                let dpose = [
                    r.i16_le()?,
                    r.i16_le()?,
                    r.i16_le()?,
                    r.i16_le()?,
                    r.i16_le()?,
                    r.i16_le()?,
                ];
                entries.push(PoseDeltaEntry { local_ix, dpose });
            }
            Ok(PoseBlock::Delta(entries))
        }
        _ => Err(NetError::UnknownTag),
    }
}

fn encode_pose_mm(b: &mut Buf, p: &PoseMm) {
    b.i32_le(p.x.0);
    b.i32_le(p.y.0);
    b.i32_le(p.z.0);
    b.i32_le(p.yaw.0);
    b.i32_le(p.pitch.0);
    b.i32_le(p.roll.0);
}

fn decode_pose_mm(r: &mut Reader<'_>) -> Result<PoseMm, NetError> {
    Ok(PoseMm {
        x: klotho_core::Mm(r.i32_le()?),
        y: klotho_core::Mm(r.i32_le()?),
        z: klotho_core::Mm(r.i32_le()?),
        yaw: YawMd(r.i32_le()?),
        pitch: YawMd(r.i32_le()?),
        roll: YawMd(r.i32_le()?),
    })
}

fn encode_vel3(b: &mut Buf, v: &Vel3) {
    b.i32_le(v.x.0);
    b.i32_le(v.y.0);
    b.i32_le(v.z.0);
}

fn decode_vel3(r: &mut Reader<'_>) -> Result<Vel3, NetError> {
    Ok(Vel3 {
        x: klotho_core::VelFx(r.i32_le()?),
        y: klotho_core::VelFx(r.i32_le()?),
        z: klotho_core::VelFx(r.i32_le()?),
    })
}

/// Canonical LE bytes of a [`PlayerIntent`]. This is the signature payload.
pub fn encode_player_intent(pi: &PlayerIntent) -> Result<Vec<u8>, NetError> {
    let mut b = Buf::new();
    b.u8(INTENT_VERSION);
    b.u8(pi.player.0);
    b.u64_le(pi.at.0);
    b.u8(pi.verb.as_u8());
    encode_target(&mut b, &pi.target)?;
    b.u16_le(pi.analog.phase);
    b.i16_le(pi.analog.stick_x);
    b.i16_le(pi.analog.stick_z);
    b.i32_le(pi.analog.look_yaw.0);
    b.i32_le(pi.analog.look_pitch);
    if pi.agency.claimed.len() > 16 {
        return Err(NetError::Oversize);
    }
    b.u8(pi.agency.claimed.len() as u8);
    for ch in &pi.agency.claimed {
        b.u8(*ch as u8);
    }
    b.u8(match pi.agency.assist {
        AssistLevel::None => 0,
    });
    if b.bytes.len() > MAX_INTENT {
        return Err(NetError::Oversize);
    }
    Ok(b.bytes)
}

/// Inverse of [`encode_player_intent`]. Trailing bytes are an error.
pub fn decode_player_intent(bytes: &[u8]) -> Result<PlayerIntent, NetError> {
    if bytes.len() > MAX_INTENT {
        return Err(NetError::Oversize);
    }
    let mut r = Reader { bytes, pos: 0 };
    if r.u8()? != INTENT_VERSION {
        return Err(NetError::BadIntent);
    }
    let player = PlayerId(r.u8()?);
    let at = Tick(r.u64_le()?);
    let verb = Verb::from_u8(r.u8()?).ok_or(NetError::BadIntent)?;
    let target = decode_target(&mut r)?;
    let analog = Analog {
        phase: r.u16_le()?,
        stick_x: r.i16_le()?,
        stick_z: r.i16_le()?,
        look_yaw: YawMd(r.i32_le()?),
        look_pitch: r.i32_le()?,
    };
    let n = r.u8()? as usize;
    if n > 16 {
        return Err(NetError::Oversize);
    }
    let mut claimed = Vec::new();
    for _ in 0..n {
        claimed.push(channel(r.u8()?)?);
    }
    let assist = match r.u8()? {
        0 => AssistLevel::None,
        _ => return Err(NetError::BadIntent),
    };
    if r.pos != r.bytes.len() {
        return Err(NetError::BadIntent);
    }
    Ok(PlayerIntent {
        player,
        at,
        verb,
        target,
        analog,
        agency: Agency { claimed, assist },
    })
}

fn encode_target(b: &mut Buf, t: &IntentTarget) -> Result<(), NetError> {
    match t {
        IntentTarget::None => b.u8(TARGET_NONE),
        IntentTarget::Sigil(s) => {
            b.u8(TARGET_SIGIL);
            b.sigil(*s);
        }
        IntentTarget::Name(n) => {
            let bytes = n.as_str().as_bytes();
            if bytes.len() > MAX_INTENT {
                return Err(NetError::Oversize);
            }
            b.u8(TARGET_NAME);
            b.u32_le(bytes.len() as u32);
            b.bytes.extend_from_slice(bytes);
        }
    }
    Ok(())
}

fn decode_target(r: &mut Reader<'_>) -> Result<IntentTarget, NetError> {
    match r.u8()? {
        TARGET_NONE => Ok(IntentTarget::None),
        TARGET_SIGIL => Ok(IntentTarget::Sigil(r.sigil()?)),
        TARGET_NAME => {
            let n = r.u32_capped(MAX_INTENT)?;
            let s = core::str::from_utf8(r.take(n)?).map_err(|_| NetError::BadIntent)?;
            Ok(IntentTarget::Name(Name::from(s)))
        }
        _ => Err(NetError::BadIntent),
    }
}

fn channel(v: u8) -> Result<Channel, NetError> {
    match v {
        1 => Ok(Channel::Timing),
        2 => Ok(Channel::Aim),
        3 => Ok(Channel::ResourceSpend),
        4 => Ok(Channel::DialogueChoice),
        _ => Err(NetError::BadIntent),
    }
}

fn encode_reject(b: &mut Buf, r: RejectReason) {
    match r {
        RejectReason::Law(id) => {
            b.u8(REJ_LAW);
            b.u16_le(id.0);
        }
        RejectReason::MissingAffordance(id) => {
            b.u8(REJ_MISSING_AFF);
            b.u16_le(id.0);
        }
        RejectReason::TimingMiss => b.u8(REJ_TIMING),
        RejectReason::Resource(id) => {
            b.u8(REJ_RESOURCE);
            b.u8(id.0);
        }
        RejectReason::HallucinatedFact => b.u8(REJ_HALLUCINATED),
        RejectReason::StaleEpoch => b.u8(REJ_STALE),
        RejectReason::UnclaimedAgency => b.u8(REJ_UNCLAIMED),
        RejectReason::WitnessMismatch => b.u8(REJ_WITNESS),
        RejectReason::WrongHull => b.u8(REJ_WRONG_HULL),
        RejectReason::Conflict => b.u8(REJ_CONFLICT),
        RejectReason::Budget => b.u8(REJ_BUDGET),
        RejectReason::TooManyIslands => b.u8(REJ_TOO_MANY_ISLANDS),
        RejectReason::IslandTooLarge => b.u8(REJ_ISLAND_TOO_LARGE),
        RejectReason::Residency => b.u8(REJ_RESIDENCY),
        RejectReason::EpochMismatch => b.u8(REJ_EPOCH_MISMATCH),
    }
}

fn decode_reject(r: &mut Reader<'_>) -> Result<RejectReason, NetError> {
    match r.u8()? {
        REJ_LAW => Ok(RejectReason::Law(LawId(r.u16_le()?))),
        REJ_MISSING_AFF => Ok(RejectReason::MissingAffordance(AffordanceId(r.u16_le()?))),
        REJ_TIMING => Ok(RejectReason::TimingMiss),
        REJ_RESOURCE => Ok(RejectReason::Resource(ResourceId(r.u8()?))),
        REJ_HALLUCINATED => Ok(RejectReason::HallucinatedFact),
        REJ_STALE => Ok(RejectReason::StaleEpoch),
        REJ_UNCLAIMED => Ok(RejectReason::UnclaimedAgency),
        REJ_WITNESS => Ok(RejectReason::WitnessMismatch),
        REJ_WRONG_HULL => Ok(RejectReason::WrongHull),
        REJ_CONFLICT => Ok(RejectReason::Conflict),
        REJ_BUDGET => Ok(RejectReason::Budget),
        REJ_TOO_MANY_ISLANDS => Ok(RejectReason::TooManyIslands),
        REJ_ISLAND_TOO_LARGE => Ok(RejectReason::IslandTooLarge),
        REJ_RESIDENCY => Ok(RejectReason::Residency),
        REJ_EPOCH_MISMATCH => Ok(RejectReason::EpochMismatch),
        _ => Err(NetError::UnknownTag),
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

    fn i16_le(&mut self, v: i16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    fn i32_le(&mut self, v: i32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    fn hash(&mut self, h: Hash) {
        self.bytes.extend_from_slice(h.as_bytes());
    }

    fn sigil(&mut self, s: Sigil) {
        self.bytes.extend_from_slice(&s.raw().to_le_bytes());
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], NetError> {
        let end = self.pos.checked_add(n).ok_or(NetError::Truncated)?;
        if end > self.bytes.len() {
            return Err(NetError::Truncated);
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, NetError> {
        Ok(self.take(1)?[0])
    }

    fn u16_le(&mut self) -> Result<u16, NetError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32_le(&mut self) -> Result<u32, NetError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u32_capped(&mut self, cap: usize) -> Result<usize, NetError> {
        let n = self.u32_le()? as usize;
        if n > cap {
            return Err(NetError::Oversize);
        }
        Ok(n)
    }

    fn u64_le(&mut self) -> Result<u64, NetError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn i16_le(&mut self) -> Result<i16, NetError> {
        let b = self.take(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]))
    }

    fn i32_le(&mut self) -> Result<i32, NetError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn hash(&mut self) -> Result<Hash, NetError> {
        let b = self.take(32)?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(b);
        Ok(Hash::from_bytes(raw))
    }

    fn sigil(&mut self) -> Result<Sigil, NetError> {
        let b = self.take(16)?;
        let mut raw = [0u8; 16];
        raw.copy_from_slice(b);
        Ok(Sigil::from_raw(u128::from_le_bytes(raw)))
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, Mm, PoseMm, Vel3, VelFx};
    use klotho_ir::Agency;
    use klotho_trace::{IslandSnap, PoseReason, TraceBody};

    use super::*;

    fn look() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(1),
            at: Tick(3),
            verb: Verb::Look,
            target: IntentTarget::None,
            analog: Analog {
                phase: 10,
                stick_x: -2,
                stick_z: 4,
                look_yaw: YawMd(100),
                look_pitch: -50,
            },
            agency: Agency::none(),
        }
    }

    fn round_trip(pkt: Packet) {
        let enc = encode_packet(&pkt).unwrap();
        let got = decode_packet(&enc).unwrap();
        assert_eq!(got, pkt);
        let frame = encode_frame(&enc).unwrap();
        let payload = decode_frame(&frame).unwrap();
        assert_eq!(decode_packet(payload).unwrap(), pkt);
    }

    fn six_dof() -> PoseMm {
        PoseMm {
            x: Mm(10),
            y: Mm(20),
            z: Mm(30),
            yaw: YawMd(40),
            pitch: YawMd(50),
            roll: YawMd(60),
        }
    }

    #[test]
    fn packet_round_trip_every_variant() {
        let stamp = CompilerStamp::current();
        let actor = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        let place = Sigil::pack(LocusKind::Place, 0, 2).unwrap();
        round_trip(Packet::Hello {
            canon_hash: Hash::from_bytes([0x11; 32]),
            epoch: Epoch(7),
            build: stamp,
            verifying_key: [0x22; 32],
            slot: PlayerId(1),
            intent_hz: 30,
        });
        round_trip(Packet::Intent {
            signed: Signed {
                signature: [0x33; 64],
                value: look(),
            },
        });
        let events = vec![
            TraceEvent::new(Tick(1), TraceBody::SaveRequested),
            TraceEvent::new(
                Tick(1),
                TraceBody::RiteAdvanced {
                    actor,
                    rite: 2,
                    pc: 3,
                    wait_left: 4,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::QtyChanged {
                    id: actor,
                    res: ResourceId(1),
                    to: 8,
                    quantum: 10,
                },
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::IslandSnap(
                    IslandSnap::new(
                        0,
                        vec![actor],
                        vec![PoseMm::new(Mm(1), Mm(0), Mm(2), YawMd(0))],
                        vec![Vel3::ZERO],
                        vec![0],
                        vec![0],
                    )
                    .unwrap(),
                ),
            ),
            TraceEvent::new(
                Tick(1),
                TraceBody::PoseCommitted {
                    s: actor,
                    pose: PoseMm::new(Mm(1), Mm(0), Mm(2), YawMd(0)),
                    reason: PoseReason::Land,
                },
            ),
        ];
        round_trip(Packet::TraceDelta {
            from: Tick(0),
            interest_gen: 3,
            events,
        });
        round_trip(Packet::Nack {
            tick: Tick(4),
            reason: RejectReason::Budget,
        });
        round_trip(Packet::Nack {
            tick: Tick(4),
            reason: RejectReason::Law(LawId(7)),
        });
        round_trip(Packet::Nack {
            tick: Tick(4),
            reason: RejectReason::Residency,
        });
        round_trip(Packet::Nack {
            tick: Tick(4),
            reason: RejectReason::EpochMismatch,
        });
        round_trip(Packet::Snapshot {
            tick: Tick(2),
            epoch: Epoch(1),
            canon_hash: Hash::from_bytes([0x44; 32]),
            trace_prefix_hash: Hash::from_bytes([0x55; 32]),
            place: Some(place),
            blob: SnapshotBlob(vec![1, 2, 3]),
        });
        round_trip(Packet::Snapshot {
            tick: Tick(0),
            epoch: Epoch::ZERO,
            canon_hash: Hash::ZERO,
            trace_prefix_hash: Hash::ZERO,
            place: None,
            blob: SnapshotBlob(Vec::new()),
        });
        round_trip(Packet::Interest {
            interest_gen: 9,
            places: vec![place],
            sigils: vec![actor],
        });
        round_trip(Packet::PoseDelta {
            tick: Tick(5),
            interest_gen: 9,
            block: PoseBlock::Full(vec![PoseFull {
                local_ix: 0,
                pose: six_dof(),
                vel: Vel3::new(VelFx(1), VelFx(2), VelFx(3)),
            }]),
        });
        round_trip(Packet::PoseDelta {
            tick: Tick(6),
            interest_gen: 9,
            block: PoseBlock::Delta(vec![PoseDeltaEntry {
                local_ix: 0,
                dpose: [1, -2, 3, -4, 5, -6],
            }]),
        });
        round_trip(Packet::Resync {
            tick: Tick(7),
            epoch: Epoch(1),
            prefix: Hash::from_bytes([0x66; 32]),
        });
        let named = PlayerIntent {
            target: IntentTarget::Name(Name::from("oak_door")),
            agency: Agency {
                claimed: vec![Channel::Timing],
                assist: AssistLevel::None,
            },
            ..look()
        };
        round_trip(Packet::Intent {
            signed: Signed {
                signature: [0x01; 64],
                value: named,
            },
        });
    }

    #[test]
    fn pose_delta_6dof_round_trip_no_sigil_in_hot_payload() {
        let actor = Sigil::pack(LocusKind::Actor, 7, 99).unwrap();
        let sigil_bytes = actor.raw().to_le_bytes();
        let delta = Packet::PoseDelta {
            tick: Tick(11),
            interest_gen: 2,
            block: PoseBlock::Delta(vec![PoseDeltaEntry {
                local_ix: 0,
                dpose: [10, 20, 30, 40, 50, 60],
            }]),
        };
        let enc = encode_packet(&delta).unwrap();
        assert_eq!(decode_packet(&enc).unwrap(), delta);
        assert!(
            !enc.windows(16).any(|w| w == sigil_bytes),
            "hot Delta payload must not contain Sigil bytes"
        );

        let full = Packet::PoseDelta {
            tick: Tick(12),
            interest_gen: 2,
            block: PoseBlock::Full(vec![PoseFull {
                local_ix: 0,
                pose: six_dof(),
                vel: Vel3::new(VelFx(4), VelFx(5), VelFx(6)),
            }]),
        };
        let enc_full = encode_packet(&full).unwrap();
        match decode_packet(&enc_full).unwrap() {
            Packet::PoseDelta {
                block: PoseBlock::Full(entries),
                ..
            } => {
                assert_eq!(entries[0].pose, six_dof());
                assert_ne!(entries[0].pose.y, Mm(0));
                assert_ne!(entries[0].pose.pitch, YawMd(0));
                assert_ne!(entries[0].pose.roll, YawMd(0));
            }
            other => panic!("expected Full PoseDelta, got {other:?}"),
        }
        assert_eq!(decode_packet(&enc_full).unwrap(), full);
    }

    #[test]
    fn truncated_oversize_event_count_error() {
        assert_eq!(decode_packet(&[]), Err(NetError::Truncated));
        assert_eq!(decode_packet(&[TAG_HELLO]), Err(NetError::Truncated));
        let mut hello = vec![TAG_HELLO];
        hello.extend_from_slice(&[0u8; 32 + 8 + 32 + 31]);
        assert_eq!(decode_packet(&hello), Err(NetError::Truncated));

        let mut oversize_count = vec![TAG_TRACE_DELTA];
        oversize_count.extend_from_slice(&0u64.to_le_bytes());
        oversize_count.extend_from_slice(&0u16.to_le_bytes());
        oversize_count.extend_from_slice(&((MAX_EVENTS as u32) + 1).to_le_bytes());
        assert_eq!(decode_packet(&oversize_count), Err(NetError::Oversize));

        let mut big_len = Vec::new();
        big_len.extend_from_slice(&((MAX_PACKET as u32) + 1).to_le_bytes());
        big_len.extend_from_slice(&[0u8; 8]);
        assert_eq!(decode_frame(&big_len), Err(NetError::Oversize));

        let mut short_frame = Vec::new();
        short_frame.extend_from_slice(&8u32.to_le_bytes());
        short_frame.extend_from_slice(&[1, 2, 3]);
        assert_eq!(decode_frame(&short_frame), Err(NetError::Truncated));

        let blob = SnapshotBlob(vec![0; MAX_BLOB + 1]);
        assert_eq!(
            encode_packet(&Packet::Snapshot {
                tick: Tick(0),
                epoch: Epoch::ZERO,
                canon_hash: Hash::ZERO,
                trace_prefix_hash: Hash::ZERO,
                place: None,
                blob,
            }),
            Err(NetError::Oversize)
        );

        let mut too_many = Vec::new();
        too_many.resize(
            MAX_EVENTS + 1,
            TraceEvent::new(Tick(0), TraceBody::SaveRequested),
        );
        assert_eq!(
            encode_packet(&Packet::TraceDelta {
                from: Tick(0),
                interest_gen: 0,
                events: too_many,
            }),
            Err(NetError::Oversize)
        );

        let mut intent_len = vec![TAG_INTENT];
        intent_len.extend_from_slice(&[0u8; 64]);
        intent_len.extend_from_slice(&((MAX_INTENT as u32) + 1).to_le_bytes());
        assert_eq!(decode_packet(&intent_len), Err(NetError::Oversize));

        let mut blob_len = vec![TAG_SNAPSHOT];
        blob_len.extend_from_slice(&0u64.to_le_bytes());
        blob_len.extend_from_slice(&0u64.to_le_bytes());
        blob_len.extend_from_slice(&[0u8; 32]);
        blob_len.extend_from_slice(&[0u8; 32]);
        blob_len.push(0);
        blob_len.extend_from_slice(&((MAX_BLOB as u32) + 1).to_le_bytes());
        assert_eq!(decode_packet(&blob_len), Err(NetError::Oversize));

        let mut interest_len = vec![TAG_INTEREST];
        interest_len.extend_from_slice(&0u16.to_le_bytes());
        interest_len.extend_from_slice(&((MAX_INTEREST as u32) + 1).to_le_bytes());
        assert_eq!(decode_packet(&interest_len), Err(NetError::Oversize));

        let many: Vec<PoseDeltaEntry> = (0..5_000)
            .map(|i| PoseDeltaEntry {
                local_ix: i as u16,
                dpose: [0; 6],
            })
            .collect();
        assert_eq!(
            encode_packet(&Packet::PoseDelta {
                tick: Tick(0),
                interest_gen: 0,
                block: PoseBlock::Delta(many),
            }),
            Err(NetError::Oversize)
        );

        let mut pose_over = vec![TAG_POSE_DELTA];
        pose_over.extend_from_slice(&0u64.to_le_bytes());
        pose_over.extend_from_slice(&0u16.to_le_bytes());
        pose_over.push(KIND_DELTA);
        pose_over.extend_from_slice(&0u16.to_le_bytes());
        pose_over.resize(MAX_POSE_DELTA + 1, 0);
        assert_eq!(decode_packet(&pose_over), Err(NetError::Oversize));
    }

    #[test]
    fn inner_event_count_oversize_is_error() {
        fn wrap_event(ev: &[u8]) -> Vec<u8> {
            let mut pkt = vec![TAG_TRACE_DELTA];
            pkt.extend_from_slice(&0u64.to_le_bytes());
            pkt.extend_from_slice(&0u16.to_le_bytes());
            pkt.extend_from_slice(&1u32.to_le_bytes());
            pkt.extend_from_slice(&(ev.len() as u32).to_le_bytes());
            pkt.extend_from_slice(ev);
            pkt
        }

        let mut snap = vec![1u8];
        snap.extend_from_slice(&0u64.to_le_bytes());
        snap.push(5);
        snap.extend_from_slice(&0u16.to_le_bytes());
        snap.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_packet(&wrap_event(&snap)), Err(NetError::BadEvent));

        let mut uttered = vec![1u8];
        uttered.extend_from_slice(&0u64.to_le_bytes());
        uttered.push(12);
        uttered.extend_from_slice(&0u128.to_le_bytes());
        uttered.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode_packet(&wrap_event(&uttered)),
            Err(NetError::BadEvent)
        );
    }

    #[test]
    fn unknown_packet_tag_error() {
        assert_eq!(decode_packet(&[99]), Err(NetError::UnknownTag));
        assert_eq!(decode_packet(&[0]), Err(NetError::UnknownTag));
        assert_eq!(decode_packet(&[9]), Err(NetError::UnknownTag));
        assert_eq!(decode_packet(&[TAG_INTEREST]), Err(NetError::Truncated));
        assert_eq!(decode_packet(&[TAG_POSE_DELTA]), Err(NetError::Truncated));
        assert_eq!(decode_packet(&[TAG_RESYNC]), Err(NetError::Truncated));
    }

    #[test]
    fn no_predicted_in_packets() {
        // Exhaustive match: a new variant is a compile error.
        let pkt = Packet::Nack {
            tick: Tick(0),
            reason: RejectReason::Budget,
        };
        match pkt {
            Packet::Hello { .. }
            | Packet::Intent { .. }
            | Packet::TraceDelta { .. }
            | Packet::Nack { .. }
            | Packet::Snapshot { .. }
            | Packet::Interest { .. }
            | Packet::PoseDelta { .. }
            | Packet::Resync { .. } => {}
        }
        assert_eq!(
            encode_packet(&Packet::Nack {
                tick: Tick(0),
                reason: RejectReason::Budget,
            })
            .unwrap()[0],
            4
        );
        assert_eq!(
            encode_packet(&Packet::Hello {
                canon_hash: Hash::ZERO,
                epoch: Epoch::ZERO,
                build: CompilerStamp::current(),
                verifying_key: [0; 32],
                slot: PlayerId(0),
                intent_hz: 20,
            })
            .unwrap()[0],
            1
        );
        assert_eq!(
            encode_packet(&Packet::Interest {
                interest_gen: 0,
                places: vec![],
                sigils: vec![],
            })
            .unwrap()[0],
            6
        );
        assert_eq!(
            encode_packet(&Packet::PoseDelta {
                tick: Tick(0),
                interest_gen: 0,
                block: PoseBlock::Delta(vec![]),
            })
            .unwrap()[0],
            7
        );
        assert_eq!(
            encode_packet(&Packet::Resync {
                tick: Tick(0),
                epoch: Epoch::ZERO,
                prefix: Hash::ZERO,
            })
            .unwrap()[0],
            8
        );
    }

    #[test]
    fn intent_le_not_serde() {
        let bytes = encode_player_intent(&look()).unwrap();
        assert_eq!(bytes[0], INTENT_VERSION);
        assert_eq!(bytes[1], 1);
        assert_eq!(&bytes[2..10], &3u64.to_le_bytes());
        assert_eq!(decode_player_intent(&bytes).unwrap(), look());
    }

    #[test]
    fn truncated_key_is_error() {
        let mut bytes = encode_packet(&Packet::Hello {
            canon_hash: Hash::ZERO,
            epoch: Epoch::ZERO,
            build: CompilerStamp::current(),
            verifying_key: [1; 32],
            slot: PlayerId(0),
            intent_hz: 20,
        })
        .unwrap();
        bytes.pop();
        assert_eq!(decode_packet(&bytes), Err(NetError::Truncated));
    }
}
