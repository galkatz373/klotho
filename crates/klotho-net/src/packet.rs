//! Frozen v1 packets. Canonical little-endian; hashed/signed bytes never go
//! through serde.

use klotho_core::{
    AffordanceId, Hash, LawId, PlayerId, RejectReason, ResourceId, Sigil, Tick, YawMd,
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

/// ASCII token whose blake3 is [`CompilerStamp::current`].
pub const STAMP_TOKEN: &[u8] = b"klotho-net/0.1.0";

const TAG_HELLO: u8 = 1;
const TAG_INTENT: u8 = 2;
const TAG_TRACE_DELTA: u8 = 3;
const TAG_NACK: u8 = 4;
const TAG_SNAPSHOT: u8 = 5;

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

/// blake3 of [`STAMP_TOKEN`]. Hello mismatch on stamp or canon_hash disconnects.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct CompilerStamp(pub Hash);

impl CompilerStamp {
    /// Stamp for this crate version.
    #[must_use]
    pub fn current() -> Self {
        Self(hash_bytes(STAMP_TOKEN))
    }
}

/// Opaque join/resync blob. v1 reconstruction uses TraceDelta, not this payload.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct SnapshotBlob(pub Vec<u8>);

/// Frozen listen-server packet.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum Packet {
    /// Join advertisement. `slot` is 0 on the client request; host replies with 1.
    Hello {
        /// Cooked Canon identity. Mismatch → disconnect (no replay).
        canon_hash: Hash,
        /// [`CompilerStamp::current`]. Mismatch → disconnect (no replay).
        build: CompilerStamp,
        /// 32-byte ed25519 verifying key. Truncated or invalid → error, not a default.
        verifying_key: [u8; 32],
        /// Assigned [`PlayerId`] (host reply) or 0 (client request).
        slot: PlayerId,
    },
    /// Signed PlayerIntent. Host verifies then keeps the latest unconsumed slot.
    Intent {
        /// Signature over the canonical LE intent bytes.
        signed: Signed<PlayerIntent>,
    },
    /// Events committed since `from`. Rejects travel as [`Packet::Nack`].
    TraceDelta {
        /// Parent tick the receiver must currently be at.
        from: Tick,
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
        /// Canon identity.
        canon_hash: Hash,
        /// Trace prefix at `tick`.
        trace_prefix_hash: Hash,
        /// Capped opaque bytes; empty is valid.
        blob: SnapshotBlob,
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
            build,
            verifying_key,
            slot,
        } => {
            b.u8(TAG_HELLO);
            b.hash(*canon_hash);
            b.hash(build.0);
            b.bytes.extend_from_slice(verifying_key);
            b.u8(slot.0);
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
        Packet::TraceDelta { from, events } => {
            if events.len() > MAX_EVENTS {
                return Err(NetError::Oversize);
            }
            b.u8(TAG_TRACE_DELTA);
            b.u64_le(from.0);
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
            canon_hash,
            trace_prefix_hash,
            blob,
        } => {
            if blob.0.len() > MAX_BLOB {
                return Err(NetError::Oversize);
            }
            b.u8(TAG_SNAPSHOT);
            b.u64_le(tick.0);
            b.hash(*canon_hash);
            b.hash(*trace_prefix_hash);
            b.u32_le(blob.0.len() as u32);
            b.bytes.extend_from_slice(&blob.0);
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
            let build = CompilerStamp(r.hash()?);
            let mut verifying_key = [0u8; 32];
            verifying_key.copy_from_slice(r.take(32)?);
            let slot = PlayerId(r.u8()?);
            Packet::Hello {
                canon_hash,
                build,
                verifying_key,
                slot,
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
            let n = r.u32_capped(MAX_EVENTS)?;
            let mut events = Vec::new();
            for _ in 0..n {
                let m = r.u32_capped(MAX_PACKET)?;
                let ev = r.take(m)?;
                let e = decode_event(ev).map_err(|_| NetError::BadEvent)?;
                events.push(e);
            }
            Packet::TraceDelta { from, events }
        }
        TAG_NACK => {
            let tick = Tick(r.u64_le()?);
            let reason = decode_reject(&mut r)?;
            Packet::Nack { tick, reason }
        }
        TAG_SNAPSHOT => {
            let tick = Tick(r.u64_le()?);
            let canon_hash = r.hash()?;
            let trace_prefix_hash = r.hash()?;
            let n = r.u32_capped(MAX_BLOB)?;
            let blob = SnapshotBlob(r.take(n)?.to_vec());
            Packet::Snapshot {
                tick,
                canon_hash,
                trace_prefix_hash,
                blob,
            }
        }
        _ => return Err(NetError::UnknownTag),
    };
    if r.pos != r.bytes.len() {
        return Err(NetError::Truncated);
    }
    Ok(pkt)
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
    use klotho_core::{LocusKind, Mm, PoseMm, Vel3};
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

    #[test]
    fn packet_round_trip_every_variant() {
        let stamp = CompilerStamp::current();
        round_trip(Packet::Hello {
            canon_hash: Hash::from_bytes([0x11; 32]),
            build: stamp,
            verifying_key: [0x22; 32],
            slot: PlayerId(1),
        });
        round_trip(Packet::Intent {
            signed: Signed {
                signature: [0x33; 64],
                value: look(),
            },
        });
        let actor = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
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
        round_trip(Packet::Snapshot {
            tick: Tick(2),
            canon_hash: Hash::from_bytes([0x44; 32]),
            trace_prefix_hash: Hash::from_bytes([0x55; 32]),
            blob: SnapshotBlob(vec![1, 2, 3]),
        });
        round_trip(Packet::Snapshot {
            tick: Tick(0),
            canon_hash: Hash::ZERO,
            trace_prefix_hash: Hash::ZERO,
            blob: SnapshotBlob(Vec::new()),
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
    fn truncated_oversize_event_count_error() {
        assert_eq!(decode_packet(&[]), Err(NetError::Truncated));
        assert_eq!(decode_packet(&[TAG_HELLO]), Err(NetError::Truncated));
        let mut hello = vec![TAG_HELLO];
        hello.extend_from_slice(&[0u8; 32 + 32 + 31]);
        assert_eq!(decode_packet(&hello), Err(NetError::Truncated));

        let mut oversize_count = vec![TAG_TRACE_DELTA];
        oversize_count.extend_from_slice(&0u64.to_le_bytes());
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
                canon_hash: Hash::ZERO,
                trace_prefix_hash: Hash::ZERO,
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
        blob_len.extend_from_slice(&[0u8; 32]);
        blob_len.extend_from_slice(&[0u8; 32]);
        blob_len.extend_from_slice(&((MAX_BLOB as u32) + 1).to_le_bytes());
        assert_eq!(decode_packet(&blob_len), Err(NetError::Oversize));
    }

    #[test]
    fn inner_event_count_oversize_is_error() {
        fn wrap_event(ev: &[u8]) -> Vec<u8> {
            let mut pkt = vec![TAG_TRACE_DELTA];
            pkt.extend_from_slice(&0u64.to_le_bytes());
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
        assert_eq!(decode_packet(&[6]), Err(NetError::UnknownTag));
    }

    #[test]
    fn no_predicted_in_packets() {
        // Exhaustive match: a new variant is a compile error. Tags 1..=5 only.
        let pkt = Packet::Nack {
            tick: Tick(0),
            reason: RejectReason::Budget,
        };
        match pkt {
            Packet::Hello { .. }
            | Packet::Intent { .. }
            | Packet::TraceDelta { .. }
            | Packet::Nack { .. }
            | Packet::Snapshot { .. } => {}
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
                build: CompilerStamp::current(),
                verifying_key: [0; 32],
                slot: PlayerId(0),
            })
            .unwrap()[0],
            1
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
            build: CompilerStamp::current(),
            verifying_key: [1; 32],
            slot: PlayerId(0),
        })
        .unwrap();
        bytes.pop();
        assert_eq!(decode_packet(&bytes), Err(NetError::Truncated));
    }
}
