//! Adaptive music from committed semantic cues (KAI-17).
//!
//! The mix renderer follows Trace; it never writes Trace or Projection.

use klotho_compile::CastingConsent;
use klotho_ir::IrError;
use klotho_manifest::{AdaptiveMusic, MusicCue};
use klotho_trace::{RelTag, TraceBody, TraceEvent};

/// Select a music cue from committed events. Later combat wins over explore.
#[must_use]
pub fn cue_from_events(events: &[TraceEvent]) -> MusicCue {
    let mut cue = MusicCue::Explore;
    for ev in events {
        match &ev.body {
            TraceBody::RelAdd { rel, .. } if *rel == RelTag::DEAD => cue = MusicCue::Stinger,
            TraceBody::RiteBegan { .. } if cue != MusicCue::Stinger => cue = MusicCue::Combat,
            TraceBody::Uttered { .. } if cue == MusicCue::Explore => cue = MusicCue::Quiet,
            _ => {}
        }
    }
    cue
}

/// Deterministic stem gains for `cue`. Crossfade is a table, not a filter.
#[must_use]
pub fn stems_for(cue: MusicCue) -> AdaptiveMusic {
    let stem_gains_milli = match cue {
        MusicCue::Explore => [1_000, 0, 0, 0],
        MusicCue::Combat => [200, 1_000, 0, 0],
        MusicCue::Quiet => [0, 0, 1_000, 0],
        MusicCue::Stinger => [0, 400, 0, 1_000],
    };
    AdaptiveMusic {
        cue,
        stem_gains_milli,
    }
}

/// Generated VO/music ships only with a complete casting record.
pub fn may_ship_vo(consent: &CastingConsent) -> Result<(), IrError> {
    consent.validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{Hash, LocusKind, Sigil, Tick};
    use klotho_ir::Name;
    use klotho_trace::TraceEvent;

    fn actor() -> Sigil {
        Sigil::pack(LocusKind::Actor, 0, 1).unwrap()
    }

    #[test]
    fn combat_then_stinger() {
        let a = actor();
        let events = [
            TraceEvent::new(
                Tick(1),
                TraceBody::RiteBegan {
                    actor: a,
                    rite: 0,
                    target: None,
                },
            ),
            TraceEvent::new(
                Tick(2),
                TraceBody::RelAdd {
                    a,
                    rel: RelTag::DEAD,
                    b: a,
                },
            ),
        ];
        assert_eq!(cue_from_events(&events), MusicCue::Stinger);
        let music = stems_for(MusicCue::Combat);
        assert_eq!(music.stem_gains_milli[1], 1_000);
        assert_eq!(stems_for(MusicCue::Explore).stem_gains_milli[0], 1_000);
    }

    #[test]
    fn generated_vo_needs_consent() {
        may_ship_vo(&CastingConsent::first_title()).unwrap();
        let mut bad = CastingConsent::first_title();
        bad.consent = Hash::ZERO;
        assert!(may_ship_vo(&bad).is_err());
        let _ = Name::from("audio-lead");
    }
}
