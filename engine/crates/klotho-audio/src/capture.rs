//! Offline mix capture metrics (KAI-18). Integer-only; no Trace writes.

use crate::mix::{MixBudget, MixFrame};

/// Millibel-scale mix measurements from a captured PCM quantum.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct AudioStats {
    /// Peak absolute sample, per-mille of `i16::MAX` (`1000` = full scale).
    pub true_peak_milli: u16,
    /// Loudness range: max window RMS minus min window RMS, millibel of full scale.
    pub loudness_range_milli: u16,
    /// One-shot voices mixed in this quantum.
    pub voices: u16,
    /// Authored cues that had no grain.
    pub missing_cues: u32,
    /// Absolute subtitle/VO start delta, milliseconds.
    pub subtitle_alignment_ms: u32,
}

impl AudioStats {
    /// Measure a mix frame plus authored cue/subtitle facts.
    #[must_use]
    pub fn measure(frame: &MixFrame, missing_cues: u32, subtitle_alignment_ms: u32) -> Self {
        Self {
            true_peak_milli: true_peak_milli(&frame.pcm),
            loudness_range_milli: loudness_range_milli(&frame.pcm),
            voices: frame.voices,
            missing_cues,
            subtitle_alignment_ms,
        }
    }

    /// Hard gates for a first-title dialogue mix.
    #[must_use]
    pub fn dialogue_pass(&self, budget: MixBudget) -> bool {
        self.true_peak_milli <= 1_000
            && self.missing_cues == 0
            && self.voices <= budget.max_voices
            && self.subtitle_alignment_ms <= 80
    }
}

/// Peak absolute sample as per-mille of full scale.
#[must_use]
pub fn true_peak_milli(pcm: &[i16]) -> u16 {
    let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    u16::try_from((u32::from(peak) * 1_000) / 32_767).unwrap_or(1_000)
}

/// Windowed RMS range, millibel of full scale. Empty PCM is 0.
#[must_use]
pub fn loudness_range_milli(pcm: &[i16]) -> u16 {
    const WINDOW: usize = 400;
    if pcm.len() < WINDOW {
        return 0;
    }
    let mut min_rms = u64::MAX;
    let mut max_rms = 0u64;
    let mut i = 0;
    while i + WINDOW <= pcm.len() {
        let mut acc = 0u64;
        for sample in &pcm[i..i + WINDOW] {
            let v = i64::from(*sample);
            acc += (v * v) as u64;
        }
        let rms = acc / WINDOW as u64;
        min_rms = min_rms.min(rms);
        max_rms = max_rms.max(rms);
        i += WINDOW;
    }
    if max_rms == 0 {
        return 0;
    }
    let range = max_rms.saturating_sub(min_rms);
    u16::try_from((range * 1_000) / (32_767u64 * 32_767u64)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mix::MixFrame;

    #[test]
    fn missing_cue_and_peak_fail_dialogue_gate() {
        let frame = MixFrame {
            pcm: vec![i16::MAX, 0, 0, 0],
            voices: 1,
        };
        let ok = AudioStats::measure(
            &MixFrame {
                pcm: vec![1_000, -1_000],
                voices: 1,
            },
            0,
            0,
        );
        assert!(ok.dialogue_pass(MixBudget::HEARTH));
        let peak = AudioStats::measure(&frame, 0, 0);
        assert_eq!(peak.true_peak_milli, 1_000);
        let missing = AudioStats::measure(&frame, 1, 0);
        assert!(!missing.dialogue_pass(MixBudget::HEARTH));
        let late = AudioStats::measure(&frame, 0, 120);
        assert!(!late.dialogue_pass(MixBudget::HEARTH));
    }
}
