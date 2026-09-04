//! Dedicated server: many clients, kernel on this process.

use std::collections::BTreeMap;
use std::path::Path;

use ed25519_dalek::VerifyingKey;
use klotho_core::{Epoch, Hash, PlayerId, PoseMm, RejectReason, Sigil, Tick, Vel3};
use klotho_ir::PlayerIntent;
use klotho_trace::{ProposalKind, TraceEvent, fold_prefix, genesis_hash};

use crate::error::NetError;
use crate::packet::{
    CompilerStamp, InterestDict, Packet, PoseBlock, PoseDeltaEntry, PoseFull, SnapshotBlob,
};
use crate::replay::write_replay;
use crate::session::{Client, DisconnectReason, Role, Wire};
use crate::sign::{Keypair, Signed, verify_intent, verifying_key_from_bytes};

/// Hard cap on joined dedicated clients.
pub const MAX_DEDICATED_PLAYERS: usize = 32;

struct Joined {
    vk: VerifyingKey,
    intent: Option<PlayerIntent>,
    interest: InterestDict,
    /// Last pose/vel flushed. Loss recovery is Resync/Full, not a PoseDelta ack.
    last_sent: BTreeMap<u16, (PoseMm, Vel3)>,
    send_full: bool,
}

/// Dedicated server: many clients, kernel on this process.
pub struct Server {
    canon_hash: Hash,
    epoch: Epoch,
    intent_hz: u8,
    stamp: CompilerStamp,
    joined: BTreeMap<PlayerId, Joined>,
    ingested: Vec<PlayerIntent>,
    dropped_older: Vec<PlayerIntent>,
    dropped_verify: usize,
    next_slot: u8,
    kp: Keypair,
    wire: Option<Wire>,
    wire_player: Option<PlayerId>,
    disconnected: bool,
    disconnect_reason: Option<DisconnectReason>,
    tick: Tick,
    prefix: Hash,
}

impl Server {
    /// Dedicated server at `epoch` advertising `intent_hz`.
    pub fn new(canon_hash: Hash, epoch: Epoch, intent_hz: u8) -> Result<Self, NetError> {
        Ok(Self {
            canon_hash,
            epoch,
            intent_hz,
            stamp: CompilerStamp::current(),
            joined: BTreeMap::new(),
            ingested: Vec::new(),
            dropped_older: Vec::new(),
            dropped_verify: 0,
            next_slot: 0,
            kp: Keypair::generate()?,
            wire: None,
            wire_player: None,
            disconnected: false,
            disconnect_reason: None,
            tick: Tick(0),
            prefix: genesis_hash(),
        })
    }

    /// Attach an in-memory wire used by [`dedicated_session`].
    pub fn attach(&mut self, wire: Wire) {
        self.wire = Some(wire);
    }

    /// Dedicated role.
    #[must_use]
    pub fn role(&self) -> Role {
        Role::Server
    }

    /// Cook / hull epoch advertised on Hello.
    #[must_use]
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// Advertised intent rate.
    #[must_use]
    pub fn intent_hz(&self) -> u8 {
        self.intent_hz
    }

    /// Joined player count.
    #[must_use]
    pub fn player_count(&self) -> usize {
        self.joined.len()
    }

    /// Current Trace prefix the next Snapshot advertises.
    #[must_use]
    pub fn prefix(&self) -> Hash {
        self.prefix
    }

    /// Set the prefix the next Snapshot advertises.
    pub fn set_prefix(&mut self, prefix: Hash) {
        self.prefix = prefix;
    }

    /// Disconnect reason, if any.
    #[must_use]
    pub fn disconnect_reason(&self) -> Option<DisconnectReason> {
        self.disconnect_reason
    }

    /// Older intents replaced in the latest-unconsumed slot this session.
    #[must_use]
    pub fn dropped_older(&self) -> &[PlayerIntent] {
        &self.dropped_older
    }

    /// Signed packets that failed verify and were not ingested.
    #[must_use]
    pub fn dropped_verify(&self) -> usize {
        self.dropped_verify
    }

    /// Intents consumed this session.
    #[must_use]
    pub fn ingested(&self) -> &[PlayerIntent] {
        &self.ingested
    }

    /// Join a verifying key. Extra joins past [`MAX_DEDICATED_PLAYERS`] are [`NetError::ServerFull`].
    pub fn accept_join(&mut self, vk_bytes: &[u8]) -> Result<PlayerId, NetError> {
        if self.disconnected {
            return Err(NetError::Disconnected);
        }
        if self.joined.len() >= MAX_DEDICATED_PLAYERS {
            return Err(NetError::ServerFull);
        }
        let vk = verifying_key_from_bytes(vk_bytes)?;
        let id = PlayerId(self.next_slot);
        self.next_slot = self.next_slot.saturating_add(1);
        self.joined.insert(
            id,
            Joined {
                vk,
                intent: None,
                interest: InterestDict::default(),
                last_sent: BTreeMap::new(),
                send_full: true,
            },
        );
        if self.wire.is_some() && self.wire_player.is_none() {
            self.wire_player = Some(id);
        }
        Ok(id)
    }

    /// Replace `player`'s Interest codebook. Next pose flush is Full.
    pub fn set_interest(
        &mut self,
        player: PlayerId,
        dict: InterestDict,
    ) -> Result<Packet, NetError> {
        let slot = self.joined.get_mut(&player).ok_or(NetError::NotJoined)?;
        if dict.places.len() > crate::packet::MAX_INTEREST
            || dict.sigils.len() > crate::packet::MAX_INTEREST
        {
            return Err(NetError::Oversize);
        }
        slot.send_full = true;
        slot.last_sent.clear();
        slot.interest = dict.clone();
        let pkt = Packet::Interest {
            interest_gen: dict.interest_gen,
            places: dict.places,
            sigils: dict.sigils,
        };
        self.send_if_wire(player, &pkt)?;
        Ok(pkt)
    }

    /// Verify a signed intent for `player` and store it as the latest unconsumed.
    pub fn ingest_signed(
        &mut self,
        player: PlayerId,
        signed: &Signed<PlayerIntent>,
    ) -> Result<bool, NetError> {
        if self.disconnected {
            return Err(NetError::Disconnected);
        }
        let Some(slot) = self.joined.get(&player) else {
            self.dropped_verify += 1;
            return Ok(false);
        };
        let vk = slot.vk;
        match verify_intent(&vk, signed) {
            Ok(mut intent) => {
                intent.player = player;
                self.queue(player, intent);
                Ok(true)
            }
            Err(NetError::BadSignature) => {
                self.dropped_verify += 1;
                Ok(false)
            }
            Err(e) => {
                self.dropped_verify += 1;
                Err(e)
            }
        }
    }

    /// Take 0 or 1 intent per player (BTreeMap order).
    pub fn consume(&mut self) -> Vec<PlayerIntent> {
        let mut out = Vec::new();
        let ids: Vec<PlayerId> = self.joined.keys().copied().collect();
        for id in ids {
            let Some(slot) = self.joined.get_mut(&id) else {
                continue;
            };
            if let Some(pi) = slot.intent.take() {
                self.ingested.push(pi.clone());
                out.push(pi);
            }
        }
        out
    }

    /// TraceDelta (current dictionary generation) plus Nacks for every joined client.
    pub fn flush_delta(
        &mut self,
        from: Tick,
        events: Vec<TraceEvent>,
        rejects: &[(ProposalKind, RejectReason)],
    ) -> Result<Vec<Packet>, NetError> {
        self.prefix = fold_prefix(self.prefix, &events);
        self.tick = Tick(from.0.saturating_add(1));
        let mut all = Vec::new();
        let ids: Vec<PlayerId> = self.joined.keys().copied().collect();
        for id in ids {
            let interest_gen = self
                .joined
                .get(&id)
                .map(|j| j.interest.interest_gen)
                .unwrap_or(0);
            let mut pkts = vec![Packet::TraceDelta {
                from,
                interest_gen,
                events: events.clone(),
            }];
            for (_, reason) in rejects {
                pkts.push(Packet::Nack {
                    tick: self.tick,
                    reason: *reason,
                });
            }
            for p in &pkts {
                self.send_if_wire(id, p)?;
            }
            all.extend(pkts);
        }
        Ok(all)
    }

    /// PoseDelta for `player`: Full on dictionary change / first tick, else Delta vs last sent.
    /// Loss recovery is Resync/Full, not a PoseDelta ack.
    pub fn flush_pose(
        &mut self,
        player: PlayerId,
        tick: Tick,
        poses: &[(Sigil, PoseMm, Vel3)],
    ) -> Result<Packet, NetError> {
        let slot = self.joined.get_mut(&player).ok_or(NetError::NotJoined)?;
        let block = build_pose_block(slot, poses);
        update_last_sent(slot, poses);
        slot.send_full = false;
        let pkt = Packet::PoseDelta {
            tick,
            interest_gen: slot.interest.interest_gen,
            block,
        };
        self.send_if_wire(player, &pkt)?;
        Ok(pkt)
    }

    /// Resync for `player`: `Resync` then the current Interest codebook. Next pose flush is Full.
    pub fn resync(&mut self, player: PlayerId) -> Result<Vec<Packet>, NetError> {
        let interest = {
            let slot = self.joined.get_mut(&player).ok_or(NetError::NotJoined)?;
            slot.send_full = true;
            slot.last_sent.clear();
            Packet::Interest {
                interest_gen: slot.interest.interest_gen,
                places: slot.interest.places.clone(),
                sigils: slot.interest.sigils.clone(),
            }
        };
        let pkts = vec![
            Packet::Resync {
                tick: self.tick,
                epoch: self.epoch,
                prefix: self.prefix,
            },
            interest,
        ];
        for p in &pkts {
            self.send_if_wire(player, p)?;
        }
        Ok(pkts)
    }

    /// Join/resync snapshot.
    #[must_use]
    pub fn snapshot_packet(&self, blob: SnapshotBlob) -> Packet {
        Packet::Snapshot {
            tick: self.tick,
            epoch: self.epoch,
            canon_hash: self.canon_hash,
            trace_prefix_hash: self.prefix,
            place: None,
            blob,
        }
    }

    /// Record a prefix mismatch so a replay may be written.
    pub fn record_desync(&mut self) {
        self.disconnect(DisconnectReason::Desync);
    }

    /// Write ingested intents after a desync disconnect.
    pub fn write_desync_replay(&self, path: &Path) -> Result<(), NetError> {
        if self.disconnect_reason != Some(DisconnectReason::Desync) {
            return Err(NetError::Disconnected);
        }
        write_replay(path, self.canon_hash, self.prefix, &self.ingested)
    }

    /// Handle one inbound packet.
    pub fn handle(&mut self, pkt: Packet) -> Result<Vec<Packet>, NetError> {
        if self.disconnected {
            return Err(NetError::Disconnected);
        }
        match pkt {
            Packet::Hello {
                canon_hash,
                epoch,
                build,
                verifying_key,
                slot: _,
                intent_hz: _,
            } => {
                if canon_hash != self.canon_hash || epoch != self.epoch || build != self.stamp {
                    return Err(NetError::HelloMismatch);
                }
                let id = self.accept_join(&verifying_key)?;
                let reply = Packet::Hello {
                    canon_hash: self.canon_hash,
                    epoch: self.epoch,
                    build: self.stamp,
                    verifying_key: self.kp.verifying_bytes(),
                    slot: id,
                    intent_hz: self.intent_hz,
                };
                let interest = Packet::Interest {
                    interest_gen: 0,
                    places: Vec::new(),
                    sigils: Vec::new(),
                };
                Ok(vec![
                    reply,
                    self.snapshot_packet(SnapshotBlob(Vec::new())),
                    interest,
                ])
            }
            Packet::Intent { signed } => {
                let player = self.wire_player.ok_or(NetError::NotJoined)?;
                let _queued = self.ingest_signed(player, &signed)?;
                Ok(Vec::new())
            }
            Packet::TraceDelta { .. }
            | Packet::Nack { .. }
            | Packet::Snapshot { .. }
            | Packet::Interest { .. }
            | Packet::PoseDelta { .. }
            | Packet::Resync { .. } => Ok(Vec::new()),
        }
    }

    /// Drain the wire into [`Self::handle`] and send replies.
    pub fn pump(&mut self) -> Result<(), NetError> {
        let Some(wire) = self.wire.clone() else {
            return Ok(());
        };
        while let Some(pkt) = wire.recv()? {
            match self.handle(pkt) {
                Ok(replies) => {
                    for r in replies {
                        wire.send(&r)?;
                    }
                }
                Err(NetError::HelloMismatch) | Err(NetError::ServerFull) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn send_if_wire(&self, player: PlayerId, pkt: &Packet) -> Result<(), NetError> {
        if self.wire_player == Some(player) {
            if let Some(wire) = &self.wire {
                wire.send(pkt)?;
            }
        }
        Ok(())
    }

    fn queue(&mut self, player: PlayerId, intent: PlayerIntent) {
        let Some(slot) = self.joined.get_mut(&player) else {
            return;
        };
        if let Some(old) = slot.intent.replace(intent) {
            self.dropped_older.push(old);
        }
    }

    fn disconnect(&mut self, reason: DisconnectReason) {
        self.disconnected = true;
        self.disconnect_reason = Some(reason);
    }
}

fn build_pose_block(slot: &Joined, poses: &[(Sigil, PoseMm, Vel3)]) -> PoseBlock {
    let mapped = map_poses(&slot.interest.sigils, poses);
    if slot.send_full || slot.last_sent.is_empty() {
        return full_block(&mapped);
    }
    let mut deltas = Vec::new();
    for (ix, pose, vel) in &mapped {
        match slot.last_sent.get(ix) {
            None => return full_block(&mapped),
            Some((lp, lv)) if lp == pose && lv == vel => {}
            Some((lp, _)) => match dpose6(lp, pose) {
                Some(d) => deltas.push(PoseDeltaEntry {
                    local_ix: *ix,
                    dpose: d,
                }),
                None => return full_block(&mapped),
            },
        }
    }
    PoseBlock::Delta(deltas)
}

fn full_block(mapped: &[(u16, PoseMm, Vel3)]) -> PoseBlock {
    PoseBlock::Full(
        mapped
            .iter()
            .map(|(ix, pose, vel)| PoseFull {
                local_ix: *ix,
                pose: *pose,
                vel: *vel,
            })
            .collect(),
    )
}

fn map_poses(dict: &[Sigil], poses: &[(Sigil, PoseMm, Vel3)]) -> Vec<(u16, PoseMm, Vel3)> {
    let mut ix_of = BTreeMap::new();
    for (i, s) in dict.iter().enumerate() {
        ix_of.entry(*s).or_insert(i as u16);
    }
    let mut out = Vec::new();
    for (s, pose, vel) in poses {
        if let Some(&ix) = ix_of.get(s) {
            out.push((ix, *pose, *vel));
        }
    }
    out
}

fn update_last_sent(slot: &mut Joined, poses: &[(Sigil, PoseMm, Vel3)]) {
    for (ix, pose, vel) in map_poses(&slot.interest.sigils, poses) {
        slot.last_sent.insert(ix, (pose, vel));
    }
}

fn dpose6(from: &PoseMm, to: &PoseMm) -> Option<[i16; 6]> {
    Some([
        i16::try_from(to.x.0.wrapping_sub(from.x.0)).ok()?,
        i16::try_from(to.y.0.wrapping_sub(from.y.0)).ok()?,
        i16::try_from(to.z.0.wrapping_sub(from.z.0)).ok()?,
        i16::try_from(to.yaw.0.wrapping_sub(from.yaw.0)).ok()?,
        i16::try_from(to.pitch.0.wrapping_sub(from.pitch.0)).ok()?,
        i16::try_from(to.roll.0.wrapping_sub(from.roll.0)).ok()?,
    ])
}

/// Handshake a dedicated Server + one Client over [`Wire`].
pub fn dedicated_session(canon_hash: Hash) -> Result<(Server, Client), NetError> {
    dedicated_session_at(canon_hash, Epoch::ZERO, 30)
}

/// Handshake with an explicit epoch and advertised intent_hz.
pub fn dedicated_session_at(
    canon_hash: Hash,
    epoch: Epoch,
    intent_hz: u8,
) -> Result<(Server, Client), NetError> {
    let (sw, cw) = Wire::pair();
    let mut server = Server::new(canon_hash, epoch, intent_hz)?;
    let mut client = Client::with_join(canon_hash, epoch, crate::session::LISTEN_INTENT_HZ)?;
    server.attach(sw);
    client.attach(cw);
    if let Some(w) = &client.wire {
        w.send(&client.hello_packet())?;
    }
    server.pump()?;
    client.pump()?;
    if client.player().is_none() {
        return Err(NetError::NotJoined);
    }
    Ok((server, client))
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, Mm, PoseMm, Sigil, Vel3, YawMd};

    use klotho_ir::{Agency, Analog, IntentTarget, Verb};

    use super::*;
    use crate::packet::{PoseBlock, encode_packet};
    use crate::session::{Host, LISTEN_INTENT_HZ};
    use crate::sign::{Keypair, sign_intent};

    fn actor() -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, 3).unwrap()
    }

    fn look(player: u8, stick: i16) -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(player),
            at: Tick(0),
            verb: Verb::Look,
            target: IntentTarget::None,
            analog: Analog {
                stick_x: stick,
                ..Analog::default()
            },
            agency: Agency::none(),
        }
    }

    fn pose(x: i32) -> PoseMm {
        PoseMm {
            x: Mm(x),
            y: Mm(20),
            z: Mm(30),
            yaw: YawMd(40),
            pitch: YawMd(50),
            roll: YawMd(60),
        }
    }

    #[test]
    fn dedicated_accepts_three_where_host_refuses_third() {
        let canon = Hash::from_bytes([20; 32]);
        let mut host = Host::new(canon).unwrap();
        let mut server = Server::new(canon, Epoch::ZERO, 30).unwrap();
        let a = Keypair::generate().unwrap();
        let b = Keypair::generate().unwrap();
        let c = Keypair::generate().unwrap();
        host.accept_join(&a.verifying_bytes()).unwrap();
        assert_eq!(
            host.accept_join(&b.verifying_bytes()).unwrap_err(),
            NetError::ThirdPlayer
        );
        server.accept_join(&a.verifying_bytes()).unwrap();
        server.accept_join(&b.verifying_bytes()).unwrap();
        server.accept_join(&c.verifying_bytes()).unwrap();
        assert_eq!(server.player_count(), 3);
        assert_eq!(server.role(), Role::Server);
        assert!(server.role().allows_mind());
        assert!(server.role().allows_infer());
    }

    #[test]
    fn dedicated_hello_stores_intent_hz_and_epoch() {
        let canon = Hash::from_bytes([21; 32]);
        let (_server, client) = dedicated_session_at(canon, Epoch::ZERO, 30).unwrap();
        assert_eq!(client.intent_hz(), 30);
        assert_eq!(client.epoch(), Epoch::ZERO);
        assert_eq!(client.interest_gen(), 0);
        assert_eq!(client.player(), Some(PlayerId(0)));
        assert_ne!(client.intent_hz(), LISTEN_INTENT_HZ);
    }

    #[test]
    fn dedicated_epoch_mismatch_refuses_join_not_server() {
        let canon = Hash::from_bytes([22; 32]);
        let mut server = Server::new(canon, Epoch(1), 30).unwrap();
        let client = Client::new(canon).unwrap();
        let err = server.handle(client.hello_packet()).unwrap_err();
        assert_eq!(err, NetError::HelloMismatch);
        assert_eq!(server.disconnect_reason(), None);
        assert_eq!(server.player_count(), 0);
    }

    #[test]
    fn hello_mismatch_leaves_other_slots() {
        let canon = Hash::from_bytes([25; 32]);
        let mut server = Server::new(canon, Epoch::ZERO, 30).unwrap();
        let a = Keypair::generate().unwrap();
        let b = Keypair::generate().unwrap();
        let pa = server.accept_join(&a.verifying_bytes()).unwrap();
        let pb = server.accept_join(&b.verifying_bytes()).unwrap();
        let bad = Client::with_join(canon, Epoch(1), 30).unwrap();
        assert_eq!(
            server.handle(bad.hello_packet()).unwrap_err(),
            NetError::HelloMismatch
        );
        assert_eq!(server.player_count(), 2);
        assert_eq!(server.disconnect_reason(), None);
        assert!(
            server
                .ingest_signed(pa, &sign_intent(&a, &look(0, 1)).unwrap())
                .unwrap()
        );
        assert!(
            server
                .ingest_signed(pb, &sign_intent(&b, &look(1, 2)).unwrap())
                .unwrap()
        );
        assert_eq!(server.consume().len(), 2);
    }

    #[test]
    fn flush_pose_full_then_delta_omits_idle() {
        let canon = Hash::from_bytes([23; 32]);
        let mut server = Server::new(canon, Epoch::ZERO, 30).unwrap();
        let kp = Keypair::generate().unwrap();
        let player = server.accept_join(&kp.verifying_bytes()).unwrap();
        let s = actor();
        server
            .set_interest(
                player,
                InterestDict {
                    interest_gen: 1,
                    places: vec![],
                    sigils: vec![s],
                },
            )
            .unwrap();
        let first = server
            .flush_pose(player, Tick(1), &[(s, pose(10), Vel3::ZERO)])
            .unwrap();
        match first {
            Packet::PoseDelta {
                interest_gen,
                block: PoseBlock::Full(entries),
                ..
            } => {
                assert_eq!(interest_gen, 1);
                assert_eq!(entries[0].pose, pose(10));
                let enc = encode_packet(&Packet::PoseDelta {
                    tick: Tick(1),
                    interest_gen: 1,
                    block: PoseBlock::Full(entries.clone()),
                })
                .unwrap();
                let raw = s.raw().to_le_bytes();
                assert!(
                    !enc.windows(16).any(|w| w == raw),
                    "Full payload uses local_ix, not Sigil"
                );
            }
            other => panic!("expected Full, got {other:?}"),
        }
        let idle = server
            .flush_pose(player, Tick(2), &[(s, pose(10), Vel3::ZERO)])
            .unwrap();
        match idle {
            Packet::PoseDelta {
                block: PoseBlock::Delta(d),
                ..
            } => assert!(d.is_empty()),
            other => panic!("expected idle Delta, got {other:?}"),
        }
        let moved = server
            .flush_pose(player, Tick(3), &[(s, pose(14), Vel3::ZERO)])
            .unwrap();
        match moved {
            Packet::PoseDelta {
                block: PoseBlock::Delta(ref d),
                ..
            } => {
                assert_eq!(d.len(), 1);
                assert_eq!(d[0].dpose[0], 4);
                let enc = encode_packet(&moved).unwrap();
                let raw = s.raw().to_le_bytes();
                assert!(!enc.windows(16).any(|w| w == raw));
            }
            other => panic!("expected Delta, got {other:?}"),
        }
    }

    #[test]
    fn server_full_at_cap() {
        let mut server = Server::new(Hash::ZERO, Epoch::ZERO, 30).unwrap();
        for _ in 0..MAX_DEDICATED_PLAYERS {
            let kp = Keypair::generate().unwrap();
            server.accept_join(&kp.verifying_bytes()).unwrap();
        }
        let extra = Keypair::generate().unwrap();
        assert_eq!(
            server.accept_join(&extra.verifying_bytes()).unwrap_err(),
            NetError::ServerFull
        );
    }

    #[test]
    fn gen_skip_resync_round_trip() {
        let canon = Hash::from_bytes([24; 32]);
        let (mut server, mut client) = dedicated_session(canon).unwrap();
        let player = client.player().unwrap();
        let s = actor();
        client
            .handle(Packet::TraceDelta {
                from: Tick(0),
                interest_gen: 2,
                events: vec![],
            })
            .unwrap();
        assert!(client.needs_resync());
        server
            .set_interest(
                player,
                InterestDict {
                    interest_gen: 2,
                    places: vec![],
                    sigils: vec![s],
                },
            )
            .unwrap();
        server.resync(player).unwrap();
        server
            .flush_pose(player, Tick(1), &[(s, pose(10), Vel3::ZERO)])
            .unwrap();
        client.pump().unwrap();
        assert!(!client.needs_resync());
        assert_eq!(client.interest_gen(), 2);
        assert_eq!(client.overlay().pose(s).unwrap().x, Mm(10));
    }
}
