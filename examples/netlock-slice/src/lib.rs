//! Netlock headless slice: eight-player dedicated server, 60 Hz signed
//! intent, unhashed pose overlay, bounded lag compensation, and desync replay.
//! The authoritative path uses the same [`CommitKernel`] as every other slice.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::Path;
use std::sync::Arc;

use klotho_canon::cook;
use klotho_commit::{CommitKernel, Proposal};
use klotho_core::{
    AabbMm, BlobId, Budget, Epoch, Hash, HullWitness, IVec3, KernelFault, LocusKind, Mm, PlayerId,
    PoseMm, Sigil, Tick, Vel3, YawMd,
};
use klotho_ir::{
    Agency, Analog, CanonDiff, IntentDoc, IntentTarget, Name, PlayerIntent, ProvenanceId, SeedFact,
    StyleIntent, Verb, from_ron,
};
use klotho_net::{Client, InterestDict, NetError, Packet, Server};
use klotho_trace::TraceDelta;
use klotho_world::World;

/// Number of players proven by the Netlock slice.
pub const PLAYER_COUNT: usize = 8;
/// Dedicated shooter intent rate.
pub const INTENT_HZ: u8 = 60;

const NETLOCK_CANON: &str = r#"[
    AddAffordance(Affordance(id: "Hittable", requires: [], grants: [], conflicts: [])),
    AddAffordance(Affordance(id: "Armed", requires: [], grants: ["Fire"], conflicts: [])),
    AddLaw(Law(
        id: "fire.hitscan",
        when: EqVerb(Fire),
        body: Pred(must: And(Affordance(Self, "Armed"), Qty(Self, "ammo", Ge, 1)), ought: None),
    )),
    AddRite(RiteGraph(id: "fire", cap_steps: 8, cap_ticks: 4, entry: 0, nodes: [
        { pc: 0, op: Bind(Self) },
        { pc: 1, op: Spend("ammo", 1, 4) },
        { pc: 2, op: Emit("Hit") },
        { pc: 3, op: Complete(Success) },
        { pc: 4, op: Complete(Fail) },
    ])),
    AddRite(RiteGraph(id: "apply_hit", cap_steps: 8, cap_ticks: 4, entry: 0, nodes: [
        { pc: 0, op: Spend("health", 25, 2) },
        { pc: 1, op: Complete(Success) },
        { pc: 2, op: Complete(Fail) },
    ])),
]"#;

/// A committed frame and the exact packets delivered to each client.
#[derive(Clone, Debug)]
pub struct NetlockFrame {
    /// Authoritative commit outcome.
    pub delta: TraceDelta,
    /// Trace/Nack packet followed by that client's pose packet.
    pub packets: Vec<Vec<Packet>>,
}

/// A Netlock runtime error.
#[derive(Debug)]
pub enum NetlockError {
    /// Packet, signature, session, or replay failure.
    Net(NetError),
    /// Commit-kernel invariant failure.
    Kernel(KernelFault),
}

impl core::fmt::Display for NetlockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Net(e) => write!(f, "net: {e}"),
            Self::Kernel(e) => write!(f, "kernel: {e}"),
        }
    }
}

impl core::error::Error for NetlockError {}

impl From<NetError> for NetlockError {
    fn from(value: NetError) -> Self {
        Self::Net(value)
    }
}

impl From<KernelFault> for NetlockError {
    fn from(value: KernelFault) -> Self {
        Self::Kernel(value)
    }
}

/// Eight clients around one dedicated authoritative kernel.
pub struct Netlock {
    kernel: CommitKernel,
    server: Server,
    clients: Vec<Client>,
    players: Vec<Sigil>,
    dummy: Sigil,
}

impl Netlock {
    /// Cook and join all eight players at 60 Hz.
    pub fn boot() -> Result<Self, NetlockError> {
        let doc = netlock_doc();
        let canon = cook(&doc).expect("Netlock Canon must cook");
        let mut kernel = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let armed = kernel.canon().affordance_id("Armed").expect("Armed");
        let hittable = kernel.canon().affordance_id("Hittable").expect("Hittable");
        let ammo = kernel.canon().resource_id("ammo").expect("ammo");
        let health = kernel.canon().resource_id("health").expect("health");
        let players: Vec<Sigil> = (0..PLAYER_COUNT)
            .map(|i| pin(&kernel, &format!("player_{i}")))
            .collect();
        let dummy = pin(&kernel, "dummy");
        {
            let mut world = kernel.world_mut();
            for (i, player) in players.iter().copied().enumerate() {
                world
                    .insert_locus(player, LocusKind::Actor)
                    .expect("player");
                world.set_affordance(player, armed, true).expect("Armed");
                world.set_qty(player, ammo, 64).expect("ammo");
                world
                    .set_hull(player, actor_hull(), BlobId::ZERO)
                    .expect("player hull");
                world
                    .set_pose(
                        player,
                        PoseMm::new(Mm((i as i32) * 2_000), Mm(0), Mm(0), YawMd::ZERO),
                    )
                    .expect("player pose");
            }
            world.insert_locus(dummy, LocusKind::Actor).expect("dummy");
            world
                .set_affordance(dummy, hittable, true)
                .expect("Hittable");
            world.set_qty(dummy, health, 100).expect("health");
            world
                .set_hull(dummy, actor_hull(), BlobId::ZERO)
                .expect("dummy hull");
            world.set_pose(dummy, on_ray()).expect("dummy pose");
        }

        let canon_hash = kernel.world().canon_hash();
        let mut server = Server::new(canon_hash, Epoch::ZERO, INTENT_HZ)?;
        server
            .sidecar_mut()
            .set_rewind_ticks(Budget::AAA_SHOOTER.rewind_ticks);
        let mut clients = Vec::with_capacity(PLAYER_COUNT);
        for (i, actor) in players.iter().copied().enumerate() {
            let mut client = Client::with_join(canon_hash, Epoch::ZERO, INTENT_HZ)?;
            for packet in server.handle(client.hello_packet())? {
                client.handle(packet)?;
            }
            let id = client.player().expect("Hello assigned a slot");
            assert_eq!(usize::from(id.0), i, "BTree player slots are stable");
            kernel.bind_player(id, actor);
            let interest = server.set_interest(
                id,
                InterestDict {
                    interest_gen: 1,
                    places: Vec::new(),
                    sigils: players
                        .iter()
                        .copied()
                        .chain(core::iter::once(dummy))
                        .collect(),
                },
            )?;
            client.handle(interest)?;
            client.overlay_mut().set_local(Some(actor));
            clients.push(client);
        }
        Ok(Self {
            kernel,
            server,
            clients,
            players,
            dummy,
        })
    }

    /// Authoritative kernel, read-only to slice callers.
    #[must_use]
    pub fn kernel(&self) -> &CommitKernel {
        &self.kernel
    }

    /// Dedicated server state.
    #[must_use]
    pub fn server(&self) -> &Server {
        &self.server
    }

    /// One overlay-only client.
    #[must_use]
    pub fn client(&self, index: usize) -> &Client {
        &self.clients[index]
    }

    /// Player actor by stable dedicated slot.
    #[must_use]
    pub fn player(&self, index: usize) -> Sigil {
        self.players[index]
    }

    /// Strafing target used by the lag-comp golden.
    #[must_use]
    pub fn dummy(&self) -> Sigil {
        self.dummy
    }

    /// Sign and queue a player intent through the dedicated ingress path.
    pub fn submit(&mut self, index: usize, intent: PlayerIntent) -> Result<(), NetlockError> {
        let packet = self.clients[index].send_intent(intent)?;
        let Packet::Intent { signed } = packet else {
            unreachable!("send_intent only returns Intent")
        };
        let id = self.clients[index].player().expect("joined client");
        let _accepted = self.server.ingest_signed(id, &signed)?;
        Ok(())
    }

    /// Enqueue an authoritative physics proposal for a mover. This is used to
    /// strafe the target; it does not expose a second world write path.
    pub fn propose_pose(&mut self, mover: Sigil, pose: PoseMm) {
        self.kernel.ingest(Proposal::PhysDelta {
            mover,
            pose,
            vel: Vel3::ZERO,
            yaw_rate: 0,
            pitch_rate: 0,
            roll_rate: 0,
            island: 0,
            sleep_ticks: 0,
            hull: BlobId::ZERO,
            witness: HullWitness::new(mover, pose, false),
            support: None,
        });
    }

    /// Commit one 60 Hz shooter tick and deliver authoritative and overlay
    /// packets to every client.
    pub fn step(&mut self) -> Result<NetlockFrame, NetlockError> {
        for intent in self.server.consume() {
            self.kernel.ingest(Proposal::Player(intent));
        }
        let from = self.kernel.world().tick();
        let delta = self.kernel.step(Tick(1), Budget::AAA_SHOOTER, &mut [])?;
        let trace_packets = self
            .server
            .flush_delta(from, delta.events.clone(), &delta.rejects)?;
        let per_client = 1 + delta.rejects.len();
        let poses = self.pose_rows();
        let mut packets = vec![Vec::new(); PLAYER_COUNT];
        for (index, chunk) in trace_packets.chunks(per_client).enumerate() {
            for packet in chunk {
                self.clients[index].handle(packet.clone())?;
                packets[index].push(packet.clone());
            }
            let id = self.clients[index].player().expect("joined client");
            let pose = self.server.flush_pose(id, delta.tick, &poses)?;
            self.clients[index].handle(pose.clone())?;
            packets[index].push(pose);
        }
        Ok(NetlockFrame { delta, packets })
    }

    /// Apply a packet to a client. A reported ancestry mismatch records the
    /// server-side desync so a replay can be written.
    pub fn apply_client_packet(
        &mut self,
        index: usize,
        packet: Packet,
    ) -> Result<(), NetlockError> {
        match self.clients[index].handle(packet) {
            Err(NetError::Desync) => {
                self.server.record_desync();
                Err(NetError::Desync.into())
            }
            other => other.map_err(Into::into),
        }
    }

    /// Write the consumed-intent replay after [`Self::apply_client_packet`]
    /// reported a desync.
    pub fn write_desync_replay(&self, path: &Path) -> Result<(), NetlockError> {
        self.server.write_desync_replay(path).map_err(Into::into)
    }

    fn pose_rows(&self) -> Vec<(Sigil, PoseMm, Vel3)> {
        let view = self.kernel.world().view();
        self.players
            .iter()
            .copied()
            .chain(core::iter::once(self.dummy))
            .filter_map(|s| {
                Some((
                    s,
                    view.pose(s)?,
                    view.vel(s).map(|(vel, _)| vel).unwrap_or(Vel3::ZERO),
                ))
            })
            .collect()
    }
}

/// A player input sampled for a particular authoritative tick.
#[must_use]
pub fn intent(verb: Verb, at: Tick) -> PlayerIntent {
    PlayerIntent {
        player: PlayerId(0),
        at,
        verb,
        target: IntentTarget::None,
        analog: Analog::default(),
        agency: Agency::none(),
    }
}

/// Target pose directly in player zero's forward ray.
#[must_use]
pub fn on_ray() -> PoseMm {
    PoseMm::new(Mm(0), Mm(0), Mm(3_000), YawMd::ZERO)
}

/// Target pose outside player zero's forward ray.
#[must_use]
pub fn off_ray() -> PoseMm {
    PoseMm::new(Mm(8_000), Mm(0), Mm(3_000), YawMd::ZERO)
}

fn netlock_doc() -> IntentDoc {
    let canon_diffs: Vec<CanonDiff> = from_ron(NETLOCK_CANON).expect("Netlock diffs");
    let mut seed = Vec::with_capacity(PLAYER_COUNT + 1);
    for i in 0..PLAYER_COUNT {
        let name = format!("player_{i}");
        seed.push(SeedFact::Locus {
            name: Name::from(name.as_str()),
            kind: LocusKind::Actor,
        });
    }
    seed.push(SeedFact::Locus {
        name: Name::from("dummy"),
        kind: LocusKind::Actor,
    });
    IntentDoc {
        style: StyleIntent::default(),
        canon_diffs,
        seed,
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    }
}

fn pin(kernel: &CommitKernel, name: &str) -> Sigil {
    kernel
        .canon()
        .pin(name)
        .unwrap_or_else(|| panic!("pin {name}"))
}

fn actor_hull() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -400,
            y: 0,
            z: -400,
        },
        IVec3 {
            x: 400,
            y: 1_800,
            z: 400,
        },
    )
}
