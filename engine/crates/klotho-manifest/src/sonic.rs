//! Sonic presentation. Trace events are the cue list; this is the presenter buffer.

use klotho_core::{BlobId, Epoch, IVec3, Tick};

/// One-shot grain voice, header-validated before decode (PR 14).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct GrainVoice {
    /// CAS grain blob.
    pub blob: BlobId,
    /// Tick the Trace event that cued this grain was committed.
    pub at: Tick,
    /// Gain, 0..=1000 (milli).
    pub gain_milli: u16,
    /// Spatial position. `None` is non-spatial (UI / bed-adjacent).
    pub pos: Option<IVec3>,
    /// Occlusion stub. Extract leaves `false` unless a hull test ran.
    pub occluded: bool,
}

/// One ambience bed. v1 has at most one.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct BedRef {
    /// CAS grain blob (loop).
    pub blob: BlobId,
    /// Gain, 0..=1000.
    pub gain_milli: u16,
}

/// Semantic music cue. Mix follows committed Trace; this is presentation.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum MusicCue {
    /// Default exploration bed.
    Explore = 0,
    /// Combat stem.
    Combat = 1,
    /// Quiet / dialogue.
    Quiet = 2,
    /// One-shot stinger.
    Stinger = 3,
}

/// Adaptive-music mix state. Stem gains are milli; the mixer never writes Trace.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct AdaptiveMusic {
    /// Cue selected from committed events.
    pub cue: MusicCue,
    /// Explore / combat / quiet / stinger stem gains, 0..=1000.
    pub stem_gains_milli: [u16; 4],
}

impl AdaptiveMusic {
    /// Explore at full, other stems silent.
    #[must_use]
    pub const fn explore() -> Self {
        Self {
            cue: MusicCue::Explore,
            stem_gains_milli: [1_000, 0, 0, 0],
        }
    }
}

/// Dumb sonic buffer. No runtime music LM.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SonicManifest {
    /// Cook / hull epoch.
    pub epoch: Epoch,
    /// One-shot grains cued from Trace this extract.
    pub grains: Vec<GrainVoice>,
    /// At most one ambience bed.
    pub bed: Option<BedRef>,
    /// Adaptive music, if a semantic cue selected a stem set.
    pub music: Option<AdaptiveMusic>,
}

impl SonicManifest {
    /// Empty buffer, no bed.
    #[must_use]
    pub const fn empty(epoch: Epoch) -> Self {
        Self {
            epoch,
            grains: Vec::new(),
            bed: None,
            music: None,
        }
    }

    /// `true` if no one-shot grain or bed would emit. Music stems are separate.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.grains.is_empty() && self.bed.is_none()
    }

    /// Build from grains + optional bed via the crate-private SoA.
    #[must_use]
    pub fn from_voices(
        epoch: Epoch,
        grains: impl IntoIterator<Item = GrainVoice>,
        bed: Option<BedRef>,
    ) -> Self {
        Self::from_voices_music(epoch, grains, bed, None)
    }

    /// Build from grains, bed, and optional adaptive music.
    #[must_use]
    pub fn from_voices_music(
        epoch: Epoch,
        grains: impl IntoIterator<Item = GrainVoice>,
        bed: Option<BedRef>,
        music: Option<AdaptiveMusic>,
    ) -> Self {
        let mut t = crate::tables::SonicTables::new();
        for g in grains {
            t.push_grain(g);
        }
        if let Some(b) = bed {
            t.set_bed(b);
        }
        if let Some(m) = music {
            t.set_music(m);
        }
        t.extract(epoch)
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{BlobId, Epoch, Tick};

    use super::*;
    use crate::tables::SonicTables;

    fn blob(n: u8) -> BlobId {
        let mut b = [0u8; 32];
        b[0] = n;
        BlobId::from_bytes(b)
    }

    #[test]
    fn empty_is_silent() {
        assert!(SonicManifest::empty(Epoch::ZERO).is_silent());
    }

    #[test]
    fn extract_keeps_one_bed() {
        let mut t = SonicTables::new();
        t.set_bed(BedRef {
            blob: blob(1),
            gain_milli: 400,
        });
        t.push_grain(GrainVoice {
            blob: blob(2),
            at: Tick(3),
            gain_milli: 1000,
            pos: None,
            occluded: false,
        });
        t.set_bed(BedRef {
            blob: blob(9),
            gain_milli: 200,
        });
        let s = t.extract(Epoch(1));
        assert_eq!(s.grains.len(), 1);
        assert_eq!(s.bed.unwrap().blob, blob(9));
        assert!(!s.is_silent());
    }
}
