//! Localization, subtitle, CC, and VO presentation descriptors (KAI-15).
//!
//! These are disposable Manifest cues. Authoritative branching lives in
//! compiled Knows / Rites; locale text never feeds simulation.

use klotho_core::{BlobId, Epoch, Tick};

/// One subtitle aligned to a compiled line key.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct SubtitleCue {
    /// Stable localization key.
    pub key: String,
    /// Speaker label.
    pub speaker: String,
    /// Localized body.
    pub body: String,
    /// Start tick.
    pub start: Tick,
    /// Duration ticks.
    pub duration: Tick,
    /// True when the body includes SDH non-speech.
    pub sdh: bool,
}

/// Closed-caption / SDH cue. Distinct from the subtitle body when SDH adds
/// non-speech descriptions.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct ClosedCaptionCue {
    /// Stable localization key.
    pub key: String,
    /// Speaker label.
    pub speaker: String,
    /// Caption body.
    pub body: String,
    /// SDH non-speech present.
    pub sdh: bool,
}

/// Voice-over grain cue. Presentation only; never a sim input.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct VoCue {
    /// Stable localization key.
    pub key: String,
    /// Approved grain blob.
    pub blob: BlobId,
    /// Start tick.
    pub start: Tick,
    /// Duration ticks.
    pub duration: Tick,
}

/// Disposable loc / subtitle / VO buffer extracted for presenters.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LocManifest {
    /// Cook / hull epoch.
    pub epoch: Epoch,
    /// Bound locale id (`en`, `ja`, `ar`).
    pub locale: String,
    /// Subtitles in draw / time order.
    pub subtitles: Vec<SubtitleCue>,
    /// Closed captions.
    pub captions: Vec<ClosedCaptionCue>,
    /// VO grains.
    pub vo: Vec<VoCue>,
}

impl LocManifest {
    /// Empty buffer for `locale`.
    #[must_use]
    pub fn empty(epoch: Epoch, locale: impl Into<String>) -> Self {
        Self {
            epoch,
            locale: locale.into(),
            subtitles: Vec::new(),
            captions: Vec::new(),
            vo: Vec::new(),
        }
    }

    /// Build from cues. Order is preserved.
    #[must_use]
    pub fn from_cues(
        epoch: Epoch,
        locale: impl Into<String>,
        subtitles: impl IntoIterator<Item = SubtitleCue>,
        captions: impl IntoIterator<Item = ClosedCaptionCue>,
        vo: impl IntoIterator<Item = VoCue>,
    ) -> Self {
        Self {
            epoch,
            locale: locale.into(),
            subtitles: subtitles.into_iter().collect(),
            captions: captions.into_iter().collect(),
            vo: vo.into_iter().collect(),
        }
    }

    /// `true` when nothing would present.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.subtitles.is_empty() && self.captions.is_empty() && self.vo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::Epoch;

    #[test]
    fn from_cues_preserves_order() {
        let loc = LocManifest::from_cues(
            Epoch::ZERO,
            "en",
            [SubtitleCue {
                key: "mira.greet".into(),
                speaker: "mira".into(),
                body: "Hello".into(),
                start: Tick(0),
                duration: Tick(8),
                sdh: false,
            }],
            [ClosedCaptionCue {
                key: "mira.greet".into(),
                speaker: "mira".into(),
                body: "Hello".into(),
                sdh: true,
            }],
            Vec::new(),
        );
        assert_eq!(loc.subtitles.len(), 1);
        assert!(loc.captions[0].sdh);
        assert!(loc.vo.is_empty());
    }
}
