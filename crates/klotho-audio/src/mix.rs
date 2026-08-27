//! Integer mix of [`SonicManifest`] to i16 stereo PCM. Header-validate first.

use std::collections::BTreeMap;

use klotho_compile::{DecodedGrain, GRAIN_HZ, decode_grain};
use klotho_core::{BlobId, IVec3, Tick};
use klotho_manifest::{Observer, SonicManifest};

/// v1 host tick rate. Mix quantum is [`GRAIN_HZ`] / this.
pub const TICK_HZ: u32 = 60;
/// PCM frames per sim tick (800 at 48 kHz / 60 Hz).
pub const SAMPLES_PER_TICK: u32 = GRAIN_HZ / TICK_HZ;

const _: () = assert!(GRAIN_HZ % TICK_HZ == 0);
const _: () = assert!(SAMPLES_PER_TICK == 800);

/// Millimetres of observer-relative X that maps to a full left/right pan.
const PAN_FULL_MM: i32 = 4_000;

/// Per-quantum mix caps (HLD: render-thread audio ≤ 0.7 ms). Gates, not proofs.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct MixBudget {
    /// Mix wall time, microseconds. Hearth target is 700.
    pub us_mix: u32,
    /// One-shot voices mixed this quantum. Exceeding is drop-later, not a reject.
    pub max_voices: u16,
    /// Skip a grain whose decoded frame count exceeds this.
    pub max_grain_frames: u32,
}

impl MixBudget {
    /// Hearth desktop defaults.
    pub const HEARTH: Self = Self {
        us_mix: 700,
        max_voices: 32,
        max_grain_frames: GRAIN_HZ,
    };
}

impl Default for MixBudget {
    fn default() -> Self {
        Self::HEARTH
    }
}

/// One mix quantum of interleaved stereo i16 at [`GRAIN_HZ`].
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct MixFrame {
    /// Interleaved L,R samples. Length is `2 * ticks * `[`SAMPLES_PER_TICK`].
    pub pcm: Vec<i16>,
    /// One-shot voices actually mixed (after skip / clamp).
    pub voices: u16,
}

impl MixFrame {
    /// Stereo frames in this buffer.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.pcm.len() / 2
    }

    /// `true` if every sample is 0.
    #[must_use]
    pub fn is_silence(&self) -> bool {
        self.pcm.iter().all(|&s| s == 0)
    }
}

/// [`IntegerMixer`] and [`NullMixer`] share this trait.
pub trait Mixer: Send {
    /// Mix `sonic` at `now`. Grain bytes are mixer-owned.
    fn mix(
        &mut self,
        sonic: &SonicManifest,
        observer: Observer,
        now: Tick,
        budget: MixBudget,
    ) -> MixFrame;
}

/// Integer mixer. Holds raw grain bytes; headers are validated at mix.
#[derive(Clone, Debug, Default)]
pub struct IntegerMixer {
    raw: BTreeMap<BlobId, Vec<u8>>,
}

impl IntegerMixer {
    /// Empty mixer, no grains bound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind grain bytes. Invalid headers are kept and skipped at mix.
    pub fn insert(&mut self, id: BlobId, bytes: &[u8]) {
        self.raw.insert(id, bytes.to_vec());
    }

    /// Mix `ticks` quanta covering `[now, now + ticks)`.
    #[must_use]
    pub fn mix_n(
        &self,
        sonic: &SonicManifest,
        observer: Observer,
        now: Tick,
        budget: MixBudget,
        ticks: u32,
    ) -> MixFrame {
        mix_n(sonic, observer, now, budget, ticks, &self.raw)
    }
}

impl Mixer for IntegerMixer {
    fn mix(
        &mut self,
        sonic: &SonicManifest,
        observer: Observer,
        now: Tick,
        budget: MixBudget,
    ) -> MixFrame {
        self.mix_n(sonic, observer, now, budget, 1)
    }
}

/// Records peak / nonzero. No device.
#[derive(Clone, Debug, Default)]
pub struct NullMixer {
    inner: IntegerMixer,
    /// How many times [`Mixer::mix`] ran.
    pub mixes: u32,
    /// Max absolute sample on the last mix.
    pub last_peak: i32,
    /// Non-zero interleaved samples on the last mix.
    pub last_nonzero: u32,
}

impl NullMixer {
    /// Empty mixer, no grains bound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind grain bytes. Forwarded to the inner integer mixer.
    pub fn insert(&mut self, id: BlobId, bytes: &[u8]) {
        self.inner.insert(id, bytes);
    }
}

impl Mixer for NullMixer {
    fn mix(
        &mut self,
        sonic: &SonicManifest,
        observer: Observer,
        now: Tick,
        budget: MixBudget,
    ) -> MixFrame {
        let frame = self.inner.mix(sonic, observer, now, budget);
        self.mixes = self.mixes.saturating_add(1);
        self.last_peak = frame
            .pcm
            .iter()
            .fold(0i32, |m, &s| m.max((s as i32).unsigned_abs() as i32));
        self.last_nonzero = frame.pcm.iter().filter(|&&s| s != 0).count() as u32;
        frame
    }
}

/// Mix one tick of stereo PCM. Header-invalid grains are skipped.
#[must_use]
pub fn mix(
    sonic: &SonicManifest,
    observer: Observer,
    now: Tick,
    budget: MixBudget,
    bytes: &BTreeMap<BlobId, Vec<u8>>,
) -> MixFrame {
    mix_n(sonic, observer, now, budget, 1, bytes)
}

/// Mix `ticks` quanta of stereo PCM covering `[now, now + ticks)`.
#[must_use]
pub fn mix_n(
    sonic: &SonicManifest,
    observer: Observer,
    now: Tick,
    budget: MixBudget,
    ticks: u32,
    bytes: &BTreeMap<BlobId, Vec<u8>>,
) -> MixFrame {
    let n_frames = (ticks as usize).saturating_mul(SAMPLES_PER_TICK as usize);
    let mut acc = vec![0i32; n_frames.saturating_mul(2)];
    if n_frames == 0 {
        return MixFrame {
            pcm: Vec::new(),
            voices: 0,
        };
    }

    let mut decoded: BTreeMap<BlobId, DecodedGrain> = BTreeMap::new();
    for g in &sonic.grains {
        decode_into(&mut decoded, g.blob, bytes);
    }
    if let Some(bed) = &sonic.bed {
        decode_into(&mut decoded, bed.blob, bytes);
    }

    let window_end = now.0.saturating_add(u64::from(ticks));
    let mut voices = 0u16;
    for g in &sonic.grains {
        if voices >= budget.max_voices {
            break;
        }
        let Some(grain) = decoded.get(&g.blob) else {
            continue;
        };
        if grain.info.frames == 0 || grain.info.frames > budget.max_grain_frames {
            continue;
        }
        if g.at.0 >= window_end {
            continue;
        }
        let dest =
            g.at.0
                .saturating_sub(now.0)
                .saturating_mul(u64::from(SAMPLES_PER_TICK));
        let playhead = now
            .0
            .saturating_sub(g.at.0)
            .saturating_mul(u64::from(SAMPLES_PER_TICK));
        if playhead >= u64::from(grain.info.frames) {
            continue;
        }
        let (left, right) = pan_milli(g.pos, observer);
        mix_oneshot(
            &mut acc,
            &grain.pcm,
            dest as usize,
            playhead as usize,
            g.gain_milli,
            left,
            right,
        );
        voices = voices.saturating_add(1);
    }

    if let Some(bed) = &sonic.bed {
        if let Some(grain) = decoded.get(&bed.blob) {
            if grain.info.frames > 0 && grain.info.frames <= budget.max_grain_frames {
                let playhead = now.0.saturating_mul(u64::from(SAMPLES_PER_TICK))
                    % u64::from(grain.info.frames);
                mix_loop(
                    &mut acc,
                    &grain.pcm,
                    playhead as usize,
                    bed.gain_milli,
                    1000,
                    1000,
                );
            }
        }
    }

    MixFrame {
        pcm: acc.into_iter().map(sat_i16).collect(),
        voices,
    }
}

fn decode_into(
    decoded: &mut BTreeMap<BlobId, DecodedGrain>,
    id: BlobId,
    bytes: &BTreeMap<BlobId, Vec<u8>>,
) {
    if decoded.contains_key(&id) {
        return;
    }
    if let Some(raw) = bytes.get(&id) {
        if let Ok(g) = decode_grain(raw) {
            decoded.insert(id, g);
        }
    }
}

fn pan_milli(pos: Option<IVec3>, observer: Observer) -> (i32, i32) {
    let Some(p) = pos else {
        return (1000, 1000);
    };
    let dx = p.x.saturating_sub(observer.eye.x.0);
    let pan = if dx <= -PAN_FULL_MM {
        -1000
    } else if dx >= PAN_FULL_MM {
        1000
    } else {
        (dx.saturating_mul(1000)) / PAN_FULL_MM
    };
    (1000 - pan.max(0), 1000 + pan.min(0))
}

fn mix_oneshot(
    acc: &mut [i32],
    pcm: &[i16],
    dest: usize,
    playhead: usize,
    gain: u16,
    left: i32,
    right: i32,
) {
    let n_frames = acc.len() / 2;
    if dest >= n_frames {
        return;
    }
    for i in dest..n_frames {
        let src = playhead.saturating_add(i - dest);
        if src >= pcm.len() {
            break;
        }
        add_sample(acc, i, pcm[src], gain, left, right);
    }
}

fn mix_loop(acc: &mut [i32], pcm: &[i16], playhead: usize, gain: u16, left: i32, right: i32) {
    let n_frames = acc.len() / 2;
    let len = pcm.len();
    if len == 0 {
        return;
    }
    let mut src = playhead % len;
    for i in 0..n_frames {
        add_sample(acc, i, pcm[src], gain, left, right);
        src += 1;
        if src == len {
            src = 0;
        }
    }
}

fn add_sample(acc: &mut [i32], frame: usize, s: i16, gain: u16, left: i32, right: i32) {
    let g = (s as i32).saturating_mul(i32::from(gain)) / 1000;
    let i = frame.saturating_mul(2);
    acc[i] = acc[i].saturating_add(g.saturating_mul(left) / 1000);
    acc[i + 1] = acc[i + 1].saturating_add(g.saturating_mul(right) / 1000);
}

fn sat_i16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[cfg(test)]
mod tests {
    use klotho_compile::{MAGIC, VERSION};
    use klotho_core::{BlobId, Epoch, Mm, PoseMm, Tick, YawMd};
    use klotho_manifest::{BedRef, GrainVoice, Observer, SonicManifest};

    use super::*;

    fn blob(n: u8) -> BlobId {
        let mut b = [0u8; 32];
        b[0] = n;
        BlobId::from_bytes(b)
    }

    fn grain_blob(hz: u32, channels: u8, pcm: &[i16]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&MAGIC);
        b.push(VERSION);
        b.push(3); // ArtifactKind::Grain
        b.push(0);
        b.push(0);
        b.extend_from_slice(&hz.to_le_bytes());
        b.push(channels);
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        for s in pcm {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    fn valid_pcm(pcm: &[i16]) -> Vec<u8> {
        grain_blob(GRAIN_HZ, 1, pcm)
    }

    fn bytes_map(id: BlobId, bytes: Vec<u8>) -> BTreeMap<BlobId, Vec<u8>> {
        let mut m = BTreeMap::new();
        m.insert(id, bytes);
        m
    }

    fn mix_map(
        sonic: &SonicManifest,
        now: Tick,
        budget: MixBudget,
        bytes: &BTreeMap<BlobId, Vec<u8>>,
    ) -> MixFrame {
        mix(sonic, Observer::origin(), now, budget, bytes)
    }

    fn voice(id: BlobId, at: Tick, pos: Option<IVec3>) -> GrainVoice {
        GrainVoice {
            blob: id,
            at,
            gain_milli: 1000,
            pos,
            occluded: false,
        }
    }

    #[test]
    fn hearth_budget_matches_hld() {
        assert_eq!(MixBudget::HEARTH.us_mix, 700);
        assert_eq!(MixBudget::HEARTH.max_voices, 32);
        assert_eq!(SAMPLES_PER_TICK, 800);
        assert_eq!(
            mix_n(
                &SonicManifest::empty(Epoch::ZERO),
                Observer::origin(),
                Tick(0),
                MixBudget::HEARTH,
                1,
                &BTreeMap::new(),
            )
            .pcm
            .len(),
            1600
        );
    }

    #[test]
    fn silent_manifest_is_digital_silence() {
        let frame = mix(
            &SonicManifest::empty(Epoch::ZERO),
            Observer::origin(),
            Tick(0),
            MixBudget::HEARTH,
            &BTreeMap::new(),
        );
        assert_eq!(frame.pcm.len(), 1600);
        assert!(frame.is_silence());
        assert_eq!(frame.voices, 0);
    }

    #[test]
    fn bad_magic_is_not_mixed() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), None)], None);
        let bytes = bytes_map(id, b"XXXX".to_vec());
        let frame = mix_map(&sonic, Tick(0), MixBudget::HEARTH, &bytes);
        assert!(frame.is_silence());
        assert_eq!(frame.voices, 0);
    }

    #[test]
    fn wrong_hz_is_not_mixed() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), None)], None);
        let bytes = bytes_map(id, grain_blob(44_100, 1, &[8_000; 16]));
        let frame = mix_map(&sonic, Tick(0), MixBudget::HEARTH, &bytes);
        assert!(frame.is_silence());
    }

    #[test]
    fn wrong_channels_is_not_mixed() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), None)], None);
        let bytes = bytes_map(id, grain_blob(GRAIN_HZ, 2, &[8_000; 16]));
        let frame = mix_map(&sonic, Tick(0), MixBudget::HEARTH, &bytes);
        assert!(frame.is_silence());
    }

    #[test]
    fn truncated_payload_is_not_mixed() {
        let id = blob(1);
        let mut raw = valid_pcm(&[8_000; 32]);
        raw.pop();
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), None)], None);
        let bytes = bytes_map(id, raw);
        let frame = mix_map(&sonic, Tick(0), MixBudget::HEARTH, &bytes);
        assert!(frame.is_silence());
    }

    #[test]
    fn oversize_frames_skipped() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), None)], None);
        let bytes = bytes_map(id, valid_pcm(&[1_000; 16]));
        let mut budget = MixBudget::HEARTH;
        budget.max_grain_frames = 8;
        let frame = mix_map(&sonic, Tick(0), budget, &bytes);
        assert!(frame.is_silence());
        budget.max_grain_frames = 16;
        let frame = mix_map(&sonic, Tick(0), budget, &bytes);
        assert!(!frame.is_silence());
        assert_eq!(frame.voices, 1);
        assert_eq!(frame.pcm[0], 1_000);
        assert_eq!(frame.pcm[1], 1_000);
    }

    #[test]
    fn playhead_past_end_is_dropped() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), None)], None);
        let bytes = bytes_map(id, valid_pcm(&[9_000; 100]));
        let live = mix_map(&sonic, Tick(0), MixBudget::HEARTH, &bytes);
        assert_eq!(live.pcm[0], 9_000);
        assert_eq!(live.voices, 1);
        let done = mix_map(&sonic, Tick(1), MixBudget::HEARTH, &bytes);
        assert!(done.is_silence());
        assert_eq!(done.voices, 0);
    }

    #[test]
    fn max_voices_keeps_trace_order() {
        let a = blob(1);
        let b = blob(2);
        let sonic = SonicManifest::from_voices(
            Epoch::ZERO,
            [voice(a, Tick(0), None), voice(b, Tick(0), None)],
            None,
        );
        let mut bytes = BTreeMap::new();
        bytes.insert(a, valid_pcm(&[3_000; 8]));
        bytes.insert(b, valid_pcm(&[7_000; 8]));
        let mut budget = MixBudget::HEARTH;
        budget.max_voices = 1;
        let frame = mix_map(&sonic, Tick(0), budget, &bytes);
        assert_eq!(frame.voices, 1);
        assert_eq!(frame.pcm[0], 3_000);
    }

    #[test]
    fn spatial_pan_uses_observer_x() {
        let id = blob(1);
        let right = Some(IVec3 {
            x: PAN_FULL_MM,
            y: 0,
            z: 0,
        });
        let left = Some(IVec3 {
            x: -PAN_FULL_MM,
            y: 0,
            z: 0,
        });
        let bytes = bytes_map(id, valid_pcm(&[4_000; 4]));
        let sonic_r = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), right)], None);
        let sonic_l = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), left)], None);
        let sonic_c = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), None)], None);
        let fr = mix_map(&sonic_r, Tick(0), MixBudget::HEARTH, &bytes);
        let fl = mix_map(&sonic_l, Tick(0), MixBudget::HEARTH, &bytes);
        let fc = mix_map(&sonic_c, Tick(0), MixBudget::HEARTH, &bytes);
        assert_eq!(fr.pcm[0], 0);
        assert_eq!(fr.pcm[1], 4_000);
        assert_eq!(fl.pcm[0], 4_000);
        assert_eq!(fl.pcm[1], 0);
        assert_eq!(fc.pcm[0], 4_000);
        assert_eq!(fc.pcm[1], 4_000);
    }

    #[test]
    fn bed_loops_and_is_center() {
        let id = blob(9);
        let sonic = SonicManifest::from_voices(
            Epoch::ZERO,
            [],
            Some(BedRef {
                blob: id,
                gain_milli: 1000,
            }),
        );
        let bytes = bytes_map(id, valid_pcm(&[2_000, 0]));
        let frame = mix_map(&sonic, Tick(0), MixBudget::HEARTH, &bytes);
        assert_eq!(frame.voices, 0);
        assert_eq!(frame.pcm[0], 2_000);
        assert_eq!(frame.pcm[1], 2_000);
        assert_eq!(frame.pcm[2], 0);
        assert_eq!(frame.pcm[3], 0);
        assert_eq!(frame.pcm[4], 2_000);
        let later = mix_map(&sonic, Tick(1), MixBudget::HEARTH, &bytes);
        assert_eq!(later.pcm[0], 2_000);
        assert_eq!(later.pcm[2], 0);
    }

    #[test]
    fn future_oneshot_is_silent_this_tick() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(1), None)], None);
        let bytes = bytes_map(id, valid_pcm(&[9_000; 8]));
        let frame = mix_map(&sonic, Tick(0), MixBudget::HEARTH, &bytes);
        assert!(frame.is_silence());
        assert_eq!(frame.voices, 0);
    }

    #[test]
    fn mix_n_places_oneshot_inside_window() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(1), None)], None);
        let bytes = bytes_map(id, valid_pcm(&[5_000; 4]));
        let frame = mix_n(
            &sonic,
            Observer::origin(),
            Tick(0),
            MixBudget::HEARTH,
            2,
            &bytes,
        );
        assert_eq!(frame.pcm.len(), 3200);
        assert_eq!(frame.voices, 1);
        assert!(frame.pcm[..1600].iter().all(|&s| s == 0));
        assert_eq!(frame.pcm[1600], 5_000);
        assert_eq!(frame.pcm[1601], 5_000);
    }

    #[test]
    fn gain_milli_scales() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(
            Epoch::ZERO,
            [GrainVoice {
                blob: id,
                at: Tick(0),
                gain_milli: 500,
                pos: None,
                occluded: false,
            }],
            None,
        );
        let bytes = bytes_map(id, valid_pcm(&[2_000; 4]));
        let frame = mix_map(&sonic, Tick(0), MixBudget::HEARTH, &bytes);
        assert_eq!(frame.pcm[0], 1_000);
    }

    #[test]
    fn null_mixer_records_peak() {
        let id = blob(1);
        let mut n = NullMixer::new();
        n.insert(id, &valid_pcm(&[1_234; 8]));
        let silent = n.mix(
            &SonicManifest::empty(Epoch::ZERO),
            Observer::origin(),
            Tick(0),
            MixBudget::HEARTH,
        );
        assert!(silent.is_silence());
        assert_eq!(n.last_peak, 0);
        assert_eq!(n.last_nonzero, 0);
        let sonic = SonicManifest::from_voices(Epoch::ZERO, [voice(id, Tick(0), None)], None);
        let _ = n.mix(&sonic, Observer::origin(), Tick(0), MixBudget::HEARTH);
        assert_eq!(n.mixes, 2);
        assert_eq!(n.last_peak, 1_234);
        assert!(n.last_nonzero > 0);
    }

    #[test]
    fn observer_offset_shifts_pan() {
        let id = blob(1);
        let sonic = SonicManifest::from_voices(
            Epoch::ZERO,
            [voice(id, Tick(0), Some(IVec3 { x: 0, y: 0, z: 0 }))],
            None,
        );
        let bytes = bytes_map(id, valid_pcm(&[4_000; 4]));
        let eye = Observer::from_look(PoseMm::new(Mm(PAN_FULL_MM), Mm(0), Mm(0), YawMd::ZERO), 0);
        let frame = mix(&sonic, eye, Tick(0), MixBudget::HEARTH, &bytes);
        // grain at 0, observer at +PAN_FULL → grain is full left
        assert_eq!(frame.pcm[0], 4_000);
        assert_eq!(frame.pcm[1], 0);
    }
}
