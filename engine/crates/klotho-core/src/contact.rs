//! Canon semantic trajectories, independent of presenter bone palettes.
use crate::{Hash, IVec3};
use serde::{Deserialize, Serialize};

/// Maximum authoritative boundaries in one action (including its endpoint).
pub const MAX_CONTACT_SAMPLES: usize = 65;
/// Maximum named sockets and sweep channels per track.
pub const MAX_CONTACT_CHANNELS: usize = 8;
/// Frozen retarget error envelope, in millimetres.
pub const CONTACT_ERROR_MM: i32 = 5;

/// One named semantic socket at each authoritative boundary.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct ContactSocket {
    /// Stable semantic name, independent of bone index.
    pub name: String,
    /// Root-local millimetre positions.
    pub samples: Vec<IVec3>,
}
/// A capsule trajectory; consecutive samples define the sweep.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct ContactSweep {
    /// Stable channel name.
    pub name: String,
    /// Semantic socket carrying this volume.
    pub socket: String,
    /// Capsule radius in millimetres.
    pub radius_mm: i32,
    /// Root-local capsule endpoints at each boundary.
    pub samples: Vec<[IVec3; 2]>,
}
/// Optional half-open foot-plant interval.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct FootPlant {
    /// Named foot socket.
    pub socket: String,
    /// First planted tick.
    pub start: u16,
    /// First unplanted tick.
    pub end: u16,
}
/// Canon-bound semantic motion artifact. Visual clip bytes are deliberately absent.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct ContactTrack {
    /// Approved skeleton identity.
    pub skeleton: Hash,
    /// Approved instrument binding identity.
    pub instrument: Hash,
    /// Semantic action identity.
    pub action: Hash,
    /// Canon Rite name.
    pub rite: String,
    /// Authoritative sampling rate, 30 or 60 Hz.
    pub tick_hz: u16,
    /// Exact labeled WAIT instruction in the Canon Rite.
    pub wait_pc: u16,
    /// Frozen player Agency channel wire tag (1–4); this declares no Agency.
    pub channel: u8,
    /// Active WAIT duration in authoritative ticks.
    pub wait_ticks: u16,
    /// Root-local absolute trajectory at tick boundaries.
    pub roots: Vec<IVec3>,
    /// Strictly name-ordered sockets.
    pub sockets: Vec<ContactSocket>,
    /// Strictly name-ordered swept capsules.
    pub sweeps: Vec<ContactSweep>,
    /// Strictly ordered (socket, start) plant intervals.
    pub plants: Vec<FootPlant>,
}
impl ContactTrack {
    /// Validate all bounds before cook or preview. Integer inputs cannot be non-finite.
    pub fn is_valid(&self) -> bool {
        fn name(s: &str) -> bool {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        }
        fn point(p: &IVec3) -> bool {
            [p.x, p.y, p.z]
                .iter()
                .all(|v| i64::from(*v).abs() <= 100_000)
        }
        let n = self.roots.len();
        self.skeleton != Hash::ZERO
            && self.instrument != Hash::ZERO
            && self.action != Hash::ZERO
            && name(&self.rite)
            && matches!(self.tick_hz, 30 | 60)
            && (1..=4).contains(&self.channel)
            && self.wait_ticks > 0
            && n == usize::from(self.wait_ticks) + 1
            && n <= MAX_CONTACT_SAMPLES
            && self.roots.iter().all(point)
            && !self.sockets.is_empty()
            && self.sockets.len() <= MAX_CONTACT_CHANNELS
            && !self.sweeps.is_empty()
            && self.sweeps.len() <= MAX_CONTACT_CHANNELS
            && self.sockets.windows(2).all(|w| w[0].name < w[1].name)
            && self.sweeps.windows(2).all(|w| w[0].name < w[1].name)
            && self
                .sockets
                .iter()
                .all(|s| name(&s.name) && s.samples.len() == n && s.samples.iter().all(point))
            && self.sweeps.iter().all(|s| {
                name(&s.name)
                    && self.sockets.iter().any(|p| p.name == s.socket)
                    && (1..=2000).contains(&s.radius_mm)
                    && s.samples.len() == n
                    && s.samples.iter().flatten().all(point)
            })
            && self.plants.len() <= 16
            && self.plants.iter().all(|p| {
                p.start < p.end
                    && p.end <= self.wait_ticks
                    && self.sockets.iter().any(|s| s.name == p.socket)
            })
            && self.plants.windows(2).all(|w| {
                (&w[0].socket, w[0].start) < (&w[1].socket, w[1].start)
                    && (w[0].socket != w[1].socket || w[0].end <= w[1].start)
            })
    }
}

/// Reproducible narrow-phase sample in a quantized semantic capsule interval.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct SweepSample {
    /// Temporal sample index; denominator is derived from Canon geometry.
    pub time: u16,
    /// Capsule centre-line sample index; denominator is derived from geometry.
    pub segment: u16,
}
