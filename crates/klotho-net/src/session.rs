//! 2-player listen-server. Host runs the only kernel; clients send signed intent.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::rc::Rc;

use ed25519_dalek::VerifyingKey;
use klotho_core::{Hash, PlayerId, RejectReason, Tick};
use klotho_ir::PlayerIntent;
use klotho_trace::{ProposalKind, TraceEvent, fold_prefix, genesis_hash};

use crate::error::NetError;
use crate::overlay::Overlay;
use crate::packet::{
    CompilerStamp, Packet, SnapshotBlob, decode_frame, decode_packet, encode_frame, encode_packet,
};
use crate::replay::write_replay;
use crate::sign::{Keypair, Signed, sign_intent, verify_intent, verifying_key_from_bytes};

/// Whether this side may ingest mind and infer proposals.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Role {
    /// Mind and infer ingest allowed.
    Host,
    /// Overlay only; never hashed. Mind and infer ingest refused.
    Client,
}

impl Role {
    /// Host-only proposers.
    #[must_use]
    pub fn allows_mind(self) -> bool {
        matches!(self, Self::Host)
    }

    /// Host-only proposers.
    #[must_use]
    pub fn allows_infer(self) -> bool {
        matches!(self, Self::Host)
    }
}

/// Why the session ended.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum DisconnectReason {
    /// Hello canon_hash or CompilerStamp mismatch. No replay file.
    HelloMismatch,
    /// Trace prefix / ancestry mismatch. Write a replay.
    Desync,
}

/// In-memory duplex. Each queued item is one length-prefixed frame.
#[derive(Clone)]
pub struct Wire {
    inbound: Rc<RefCell<VecDeque<Vec<u8>>>>,
    outbound: Rc<RefCell<VecDeque<Vec<u8>>>>,
}

impl Wire {
    /// Two connected ends.
    #[must_use]
    pub fn pair() -> (Self, Self) {
        let a = Rc::new(RefCell::new(VecDeque::new()));
        let b = Rc::new(RefCell::new(VecDeque::new()));
        (
            Self {
                inbound: Rc::clone(&a),
                outbound: Rc::clone(&b),
            },
            Self {
                inbound: b,
                outbound: a,
            },
        )
    }

    /// Encode and queue a packet.
    pub fn send(&self, pkt: &Packet) -> Result<(), NetError> {
        let payload = encode_packet(pkt)?;
        let frame = encode_frame(&payload)?;
        self.outbound.borrow_mut().push_back(frame);
        Ok(())
    }

    /// Pop one framed packet.
    pub fn recv(&self) -> Result<Option<Packet>, NetError> {
        let frame = match self.inbound.borrow_mut().pop_front() {
            Some(f) => f,
            None => return Ok(None),
        };
        let payload = decode_frame(&frame)?;
        Ok(Some(decode_packet(payload)?))
    }
}

/// Host side of a 2-player listen-server.
pub struct Host {
    canon_hash: Hash,
    stamp: CompilerStamp,
    keys: BTreeMap<PlayerId, VerifyingKey>,
    slots: BTreeMap<PlayerId, Option<PlayerIntent>>,
    ingested: Vec<PlayerIntent>,
    dropped_older: Vec<PlayerIntent>,
    dropped_verify: usize,
    remote: Option<PlayerId>,
    kp: Keypair,
    wire: Option<Wire>,
    disconnected: bool,
    disconnect_reason: Option<DisconnectReason>,
    tick: Tick,
    prefix: Hash,
}

impl Host {
    /// PlayerId 0 is the host. No remote until Hello.
    pub fn new(canon_hash: Hash) -> Result<Self, NetError> {
        let kp = Keypair::generate()?;
        let mut keys = BTreeMap::new();
        keys.insert(PlayerId(0), kp.verifying_key());
        let mut slots = BTreeMap::new();
        slots.insert(PlayerId(0), None);
        Ok(Self {
            canon_hash,
            stamp: CompilerStamp::current(),
            keys,
            slots,
            ingested: Vec::new(),
            dropped_older: Vec::new(),
            dropped_verify: 0,
            remote: None,
            kp,
            wire: None,
            disconnected: false,
            disconnect_reason: None,
            tick: Tick(0),
            prefix: genesis_hash(),
        })
    }

    /// Attach an in-memory wire.
    pub fn attach(&mut self, wire: Wire) {
        self.wire = Some(wire);
    }

    /// Host role.
    #[must_use]
    pub fn role(&self) -> Role {
        Role::Host
    }

    /// Remote slot after a successful join.
    #[must_use]
    pub fn remote(&self) -> Option<PlayerId> {
        self.remote
    }

    /// Current Trace prefix the host will put on Snapshot.
    #[must_use]
    pub fn prefix(&self) -> Hash {
        self.prefix
    }

    /// Set the prefix the next Snapshot advertises (caller folded host events).
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

    /// Intents consumed (host-accepted) this session.
    #[must_use]
    pub fn ingested(&self) -> &[PlayerIntent] {
        &self.ingested
    }

    /// Join a remote verifying key as PlayerId 1.
    pub fn accept_join(&mut self, vk_bytes: &[u8]) -> Result<PlayerId, NetError> {
        if self.disconnected {
            return Err(NetError::Disconnected);
        }
        if self.remote.is_some() {
            return Err(NetError::ThirdPlayer);
        }
        let vk = verifying_key_from_bytes(vk_bytes)?;
        let id = PlayerId(1);
        self.keys.insert(id, vk);
        self.slots.insert(id, None);
        self.remote = Some(id);
        Ok(id)
    }

    /// Queue a local (host) intent into PlayerId 0's latest-unconsumed slot.
    pub fn submit_local(&mut self, mut intent: PlayerIntent) {
        intent.player = PlayerId(0);
        self.queue(PlayerId(0), intent);
    }

    /// Verify a signed intent for `player` and store it as the latest unconsumed.
    ///
    /// `Ok(false)` means the packet was dropped (bad signature). The claimed
    /// `intent.player` is overwritten from the session map.
    pub fn ingest_signed(
        &mut self,
        player: PlayerId,
        signed: &Signed<PlayerIntent>,
    ) -> Result<bool, NetError> {
        if self.disconnected {
            return Err(NetError::Disconnected);
        }
        let Some(vk) = self.keys.get(&player).copied() else {
            self.dropped_verify += 1;
            return Ok(false);
        };
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

    /// Take 0 or 1 intent per player (BTreeMap order). Extra packets in the
    /// same tick were already dropped by latest-wins.
    pub fn consume(&mut self) -> Vec<PlayerIntent> {
        let mut out = Vec::new();
        let ids: Vec<PlayerId> = self.slots.keys().copied().collect();
        for id in ids {
            let Some(slot) = self.slots.get_mut(&id) else {
                continue;
            };
            if let Some(pi) = slot.take() {
                self.ingested.push(pi.clone());
                out.push(pi);
            }
        }
        out
    }

    /// Packets to broadcast after a host tick: one TraceDelta plus a Nack per reject.
    pub fn flush_delta(
        &mut self,
        from: Tick,
        events: Vec<TraceEvent>,
        rejects: &[(ProposalKind, RejectReason)],
    ) -> Result<Vec<Packet>, NetError> {
        self.prefix = fold_prefix(self.prefix, &events);
        self.tick = Tick(from.0.saturating_add(1));
        let mut pkts = vec![Packet::TraceDelta { from, events }];
        for (_, reason) in rejects {
            pkts.push(Packet::Nack {
                tick: self.tick,
                reason: *reason,
            });
        }
        if let Some(wire) = &self.wire {
            for p in &pkts {
                wire.send(p)?;
            }
        }
        Ok(pkts)
    }

    /// Join/resync snapshot (hashes plus a capped blob; empty is valid).
    #[must_use]
    pub fn snapshot_packet(&self, blob: SnapshotBlob) -> Packet {
        Packet::Snapshot {
            tick: self.tick,
            canon_hash: self.canon_hash,
            trace_prefix_hash: self.prefix,
            blob,
        }
    }

    /// Record a prefix mismatch reported by a client so a replay may be written.
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

    /// Handle one inbound packet. Hello mismatch disconnects without a replay.
    pub fn handle(&mut self, pkt: Packet) -> Result<Vec<Packet>, NetError> {
        if self.disconnected {
            return Err(NetError::Disconnected);
        }
        match pkt {
            Packet::Hello {
                canon_hash,
                build,
                verifying_key,
                slot: _,
            } => {
                if canon_hash != self.canon_hash || build != self.stamp {
                    self.disconnect(DisconnectReason::HelloMismatch);
                    return Err(NetError::HelloMismatch);
                }
                let id = self.accept_join(&verifying_key)?;
                let reply = Packet::Hello {
                    canon_hash: self.canon_hash,
                    build: self.stamp,
                    verifying_key: self.kp.verifying_bytes(),
                    slot: id,
                };
                Ok(vec![reply, self.snapshot_packet(SnapshotBlob(Vec::new()))])
            }
            Packet::Intent { signed } => {
                let player = self.remote.ok_or(NetError::NotJoined)?;
                let _queued = self.ingest_signed(player, &signed)?;
                Ok(Vec::new())
            }
            Packet::TraceDelta { .. } | Packet::Nack { .. } | Packet::Snapshot { .. } => {
                Ok(Vec::new())
            }
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
                Err(NetError::HelloMismatch) | Err(NetError::ThirdPlayer) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn queue(&mut self, player: PlayerId, intent: PlayerIntent) {
        let slot = self.slots.entry(player).or_insert(None);
        if let Some(old) = slot.replace(intent) {
            self.dropped_older.push(old);
        }
    }

    fn disconnect(&mut self, reason: DisconnectReason) {
        self.disconnected = true;
        self.disconnect_reason = Some(reason);
    }
}

/// Client side: overlay + prefix tracking. Does not run CommitKernel.
pub struct Client {
    canon_hash: Hash,
    stamp: CompilerStamp,
    kp: Keypair,
    player: Option<PlayerId>,
    overlay: Overlay,
    expected_prefix: Hash,
    last_tick: Tick,
    saw_snapshot: bool,
    wire: Option<Wire>,
    disconnected: bool,
    disconnect_reason: Option<DisconnectReason>,
}

impl Client {
    /// New client for `canon_hash`. Generates a join key.
    pub fn new(canon_hash: Hash) -> Result<Self, NetError> {
        Ok(Self {
            canon_hash,
            stamp: CompilerStamp::current(),
            kp: Keypair::generate()?,
            player: None,
            overlay: Overlay::new(),
            expected_prefix: genesis_hash(),
            last_tick: Tick(0),
            saw_snapshot: false,
            wire: None,
            disconnected: false,
            disconnect_reason: None,
        })
    }

    /// Attach an in-memory wire.
    pub fn attach(&mut self, wire: Wire) {
        self.wire = Some(wire);
    }

    /// Client role.
    #[must_use]
    pub fn role(&self) -> Role {
        Role::Client
    }

    /// Assigned slot after Hello.
    #[must_use]
    pub fn player(&self) -> Option<PlayerId> {
        self.player
    }

    /// Folded prefix of applied deltas / last snapshot.
    #[must_use]
    pub fn prefix(&self) -> Hash {
        self.expected_prefix
    }

    /// Client overlay (never hashed).
    #[must_use]
    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    /// Disconnect reason, if any.
    #[must_use]
    pub fn disconnect_reason(&self) -> Option<DisconnectReason> {
        self.disconnect_reason
    }

    /// True after a disconnect.
    #[must_use]
    pub fn disconnected(&self) -> bool {
        self.disconnected
    }

    /// Clients only deliver PlayerIntent.
    pub fn ingest_kind(&self, kind: ProposalKind) -> Result<(), NetError> {
        match kind {
            ProposalKind::Player => Ok(()),
            ProposalKind::Space
            | ProposalKind::Motion
            | ProposalKind::Mind
            | ProposalKind::Infer => Err(NetError::HostOnly),
        }
    }

    /// Hello request (slot 0 = unassigned).
    #[must_use]
    pub fn hello_packet(&self) -> Packet {
        Packet::Hello {
            canon_hash: self.canon_hash,
            build: self.stamp,
            verifying_key: self.kp.verifying_bytes(),
            slot: PlayerId(0),
        }
    }

    /// Sign and optionally send an intent as this client's assigned PlayerId.
    pub fn send_intent(&mut self, mut intent: PlayerIntent) -> Result<Packet, NetError> {
        if self.disconnected {
            return Err(NetError::Disconnected);
        }
        let player = self.player.ok_or(NetError::NotJoined)?;
        intent.player = player;
        let signed = sign_intent(&self.kp, &intent)?;
        let pkt = Packet::Intent { signed };
        if let Some(wire) = &self.wire {
            wire.send(&pkt)?;
        }
        Ok(pkt)
    }

    /// Apply a host packet. Prefix mismatch disconnects (caller writes replay).
    pub fn handle(&mut self, pkt: Packet) -> Result<(), NetError> {
        if self.disconnected {
            return Err(NetError::Disconnected);
        }
        match pkt {
            Packet::Hello {
                canon_hash,
                build,
                verifying_key: _,
                slot,
            } => {
                if canon_hash != self.canon_hash || build != self.stamp {
                    self.disconnect(DisconnectReason::HelloMismatch);
                    return Err(NetError::HelloMismatch);
                }
                self.player = Some(slot);
                Ok(())
            }
            Packet::Snapshot {
                tick,
                canon_hash,
                trace_prefix_hash,
                blob: _,
            } => {
                if canon_hash != self.canon_hash {
                    self.disconnect(DisconnectReason::Desync);
                    return Err(NetError::Desync);
                }
                if self.saw_snapshot && trace_prefix_hash != self.expected_prefix {
                    self.disconnect(DisconnectReason::Desync);
                    return Err(NetError::Desync);
                }
                self.expected_prefix = trace_prefix_hash;
                self.last_tick = tick;
                self.saw_snapshot = true;
                Ok(())
            }
            Packet::TraceDelta { from, events } => {
                if from != self.last_tick {
                    self.disconnect(DisconnectReason::Desync);
                    return Err(NetError::Desync);
                }
                self.overlay.apply_delta(&events);
                self.expected_prefix = fold_prefix(self.expected_prefix, &events);
                self.last_tick = Tick(from.0.saturating_add(1));
                Ok(())
            }
            Packet::Nack { .. } => Ok(()),
            Packet::Intent { .. } => Ok(()),
        }
    }

    /// Drain the wire into [`Self::handle`].
    pub fn pump(&mut self) -> Result<(), NetError> {
        let Some(wire) = self.wire.clone() else {
            return Ok(());
        };
        while let Some(pkt) = wire.recv()? {
            self.handle(pkt)?;
        }
        Ok(())
    }

    fn disconnect(&mut self, reason: DisconnectReason) {
        self.disconnected = true;
        self.disconnect_reason = Some(reason);
    }
}

/// Handshake an in-memory host+client pair (PlayerId 0 and 1).
pub fn memory_session(canon_hash: Hash) -> Result<(Host, Client), NetError> {
    let (hw, cw) = Wire::pair();
    let mut host = Host::new(canon_hash)?;
    let mut client = Client::new(canon_hash)?;
    host.attach(hw);
    client.attach(cw);
    if let Some(w) = &client.wire {
        w.send(&client.hello_packet())?;
    }
    host.pump()?;
    client.pump()?;
    if client.player() != Some(PlayerId(1)) {
        return Err(NetError::NotJoined);
    }
    Ok((host, client))
}

#[cfg(test)]
mod tests {
    use klotho_core::{LocusKind, Mm, Sigil, YawMd};
    use klotho_ir::{Agency, Analog, IntentTarget, Verb};
    use klotho_trace::{PoseReason, TraceBody, fold_prefix, genesis_hash};

    use super::*;
    use crate::packet::CompilerStamp;
    use crate::replay::load_replay_intents;
    use crate::sign::{Keypair, sign_intent};

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

    #[test]
    fn hello_canon_hash_mismatch_disconnects() {
        let mut host = Host::new(Hash::from_bytes([1; 32])).unwrap();
        let client = Client::new(Hash::from_bytes([2; 32])).unwrap();
        let err = host.handle(client.hello_packet()).unwrap_err();
        assert_eq!(err, NetError::HelloMismatch);
        assert_eq!(
            host.disconnect_reason(),
            Some(DisconnectReason::HelloMismatch)
        );
        assert!(host.ingested().is_empty());
        let path = std::env::temp_dir().join(format!(
            "klotho-net-replay-hello-{}-1.ron",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        assert_eq!(host.write_desync_replay(&path), Err(NetError::Disconnected));
        assert!(!path.exists());
    }

    #[test]
    fn hello_compiler_stamp_mismatch_disconnects() {
        let canon = Hash::from_bytes([3; 32]);
        let mut host = Host::new(canon).unwrap();
        let mut build = CompilerStamp::current();
        build.0.0[0] ^= 0xff;
        let kp = Keypair::generate().unwrap();
        let err = host
            .handle(Packet::Hello {
                canon_hash: canon,
                build,
                verifying_key: kp.verifying_bytes(),
                slot: PlayerId(0),
            })
            .unwrap_err();
        assert_eq!(err, NetError::HelloMismatch);
        assert_eq!(
            host.disconnect_reason(),
            Some(DisconnectReason::HelloMismatch)
        );
    }

    #[test]
    fn spoofed_intent_dropped() {
        let canon = Hash::from_bytes([4; 32]);
        let mut host = Host::new(canon).unwrap();
        let real = Keypair::generate().unwrap();
        host.accept_join(&real.verifying_bytes()).unwrap();
        let spoof = Keypair::generate().unwrap();
        let signed = sign_intent(&spoof, &look(1, 1)).unwrap();
        assert!(!host.ingest_signed(PlayerId(1), &signed).unwrap());
        assert_eq!(host.dropped_verify(), 1);
        assert!(host.consume().is_empty());
        assert!(host.ingested().is_empty());

        let mut flipped = sign_intent(&real, &look(1, 2)).unwrap();
        flipped.signature[0] ^= 0xff;
        assert!(!host.ingest_signed(PlayerId(1), &flipped).unwrap());
        assert_eq!(host.dropped_verify(), 2);
        assert!(host.consume().is_empty());
    }

    #[test]
    fn latest_unconsumed_intent_wins() {
        let mut host = Host::new(Hash::from_bytes([5; 32])).unwrap();
        let a = look(0, 1);
        let b = look(0, 2);
        host.submit_local(a.clone());
        host.submit_local(b.clone());
        assert_eq!(host.dropped_older(), std::slice::from_ref(&a));
        let got = host.consume();
        assert_eq!(got, vec![b.clone()]);
        assert_eq!(host.ingested(), std::slice::from_ref(&b));
        assert_eq!(got[0].analog.stick_x, 2);
    }

    #[test]
    fn empty_slot_consumes_zero() {
        let mut host = Host::new(Hash::from_bytes([6; 32])).unwrap();
        assert!(host.consume().is_empty());
        assert!(host.ingested().is_empty());
    }

    #[test]
    fn unknown_or_truncated_key_is_error() {
        let mut host = Host::new(Hash::ZERO).unwrap();
        assert_eq!(host.accept_join(&[0u8; 16]), Err(NetError::BadKey));
        assert_eq!(host.accept_join(&[0u8; 31]), Err(NetError::BadKey));
        assert_eq!(host.accept_join(&[0u8; 32]), Err(NetError::BadKey));
        let signed = sign_intent(&Keypair::generate().unwrap(), &look(1, 1)).unwrap();
        assert!(!host.ingest_signed(PlayerId(1), &signed).unwrap());
        assert_eq!(host.dropped_verify(), 1);
        assert!(host.consume().is_empty());
        assert_eq!(host.remote(), None);
    }

    #[test]
    fn third_player_join_refused() {
        let canon = Hash::from_bytes([7; 32]);
        let mut host = Host::new(canon).unwrap();
        let a = Keypair::generate().unwrap();
        let b = Keypair::generate().unwrap();
        host.handle(Packet::Hello {
            canon_hash: canon,
            build: CompilerStamp::current(),
            verifying_key: a.verifying_bytes(),
            slot: PlayerId(0),
        })
        .unwrap();
        assert_eq!(
            host.handle(Packet::Hello {
                canon_hash: canon,
                build: CompilerStamp::current(),
                verifying_key: b.verifying_bytes(),
                slot: PlayerId(0),
            })
            .unwrap_err(),
            NetError::ThirdPlayer
        );
        assert_eq!(host.disconnect_reason(), None);
        assert_eq!(host.remote(), Some(PlayerId(1)));
        let signed = sign_intent(&a, &look(1, 1)).unwrap();
        assert!(host.ingest_signed(PlayerId(1), &signed).unwrap());
        assert_eq!(host.consume().len(), 1);
    }

    #[test]
    fn client_refuses_mind_infer_ingest() {
        let client = Client::new(Hash::ZERO).unwrap();
        let host = Host::new(Hash::ZERO).unwrap();
        assert_eq!(client.role(), Role::Client);
        assert!(!client.role().allows_mind());
        assert!(!client.role().allows_infer());
        assert!(host.role().allows_mind());
        assert!(host.role().allows_infer());
        assert_eq!(
            client.ingest_kind(ProposalKind::Mind),
            Err(NetError::HostOnly)
        );
        assert_eq!(
            client.ingest_kind(ProposalKind::Infer),
            Err(NetError::HostOnly)
        );
        assert_eq!(
            client.ingest_kind(ProposalKind::Space),
            Err(NetError::HostOnly)
        );
        assert_eq!(
            client.ingest_kind(ProposalKind::Motion),
            Err(NetError::HostOnly)
        );
        client.ingest_kind(ProposalKind::Player).unwrap();
    }

    #[test]
    fn claimed_player_overwritten_from_session() {
        let canon = Hash::from_bytes([8; 32]);
        let mut host = Host::new(canon).unwrap();
        let kp = Keypair::generate().unwrap();
        host.accept_join(&kp.verifying_bytes()).unwrap();
        let signed = sign_intent(&kp, &look(0, 9)).unwrap();
        assert!(host.ingest_signed(PlayerId(1), &signed).unwrap());
        let got = host.consume();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].player, PlayerId(1));
        assert_eq!(got[0].analog.stick_x, 9);
    }

    #[test]
    fn desync_writes_replay_file() {
        let canon = Hash::from_bytes([9; 32]);
        let (mut host, mut client) = memory_session(canon).unwrap();
        host.submit_local(look(0, 1));
        let _ = client.send_intent(look(1, 2)).unwrap();
        host.pump().unwrap();
        let ingested = host.consume();
        assert_eq!(ingested.len(), 2);

        let err = client
            .handle(Packet::Snapshot {
                tick: Tick(0),
                canon_hash: canon,
                trace_prefix_hash: Hash::from_bytes([0xab; 32]),
                blob: SnapshotBlob(Vec::new()),
            })
            .unwrap_err();
        assert_eq!(err, NetError::Desync);
        assert_eq!(client.disconnect_reason(), Some(DisconnectReason::Desync));
        assert!(client.disconnected());

        let path = std::env::temp_dir().join(format!(
            "klotho-net-replay-desync-{}-{}.ron",
            std::process::id(),
            9
        ));
        assert_eq!(host.write_desync_replay(&path), Err(NetError::Disconnected));
        host.record_desync();
        host.write_desync_replay(&path).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 0);
        let parsed = load_replay_intents(&path).unwrap();
        assert_eq!(parsed, ingested);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn two_player_memory_session() {
        let canon = Hash::from_bytes([10; 32]);
        let (mut host, mut client) = memory_session(canon).unwrap();
        assert_eq!(host.role(), Role::Host);
        assert_eq!(client.player(), Some(PlayerId(1)));

        host.submit_local(look(0, 3));
        let _ = client.send_intent(look(1, 4)).unwrap();
        host.pump().unwrap();
        let got = host.consume();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].player, PlayerId(0));
        assert_eq!(got[1].player, PlayerId(1));

        let s = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        let events = vec![TraceEvent::new(
            Tick(1),
            TraceBody::PoseCommitted {
                s,
                xz: (Mm(5), Mm(6)),
                yaw: YawMd(0),
                reason: PoseReason::Land,
            },
        )];
        let expected = fold_prefix(genesis_hash(), &events);
        host.flush_delta(Tick(0), events, &[]).unwrap();
        client.pump().unwrap();
        assert_eq!(client.prefix(), expected);
        assert_eq!(client.overlay().pose(s).unwrap().x, Mm(5));
        assert!(!client.disconnected());
    }

    #[test]
    fn delta_ancestry_mismatch_is_desync() {
        let (host, mut client) = memory_session(Hash::from_bytes([11; 32])).unwrap();
        let _ = host;
        let err = client
            .handle(Packet::TraceDelta {
                from: Tick(99),
                events: vec![],
            })
            .unwrap_err();
        assert_eq!(err, NetError::Desync);
        assert!(client.disconnected());
        assert_eq!(client.disconnect_reason(), Some(DisconnectReason::Desync));
    }

    #[test]
    fn latest_unconsumed_intent_wins_on_wire() {
        let canon = Hash::from_bytes([12; 32]);
        let (mut host, mut client) = memory_session(canon).unwrap();
        let _ = client.send_intent(look(1, 1)).unwrap();
        let _ = client.send_intent(look(1, 2)).unwrap();
        host.pump().unwrap();
        let got = host.consume();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].analog.stick_x, 2);
        assert_eq!(host.dropped_older().len(), 1);
        assert_eq!(host.dropped_older()[0].analog.stick_x, 1);
        assert_eq!(host.ingested(), got.as_slice());
    }
}
