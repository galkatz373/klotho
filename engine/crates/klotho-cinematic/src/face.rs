//! Lip-sync sampling on a cinematic track (KAI-17). Presentation only.

use klotho_anim::FaceTrack;
use klotho_manifest::Tick;

/// One viseme sample at a global tick. 50 ms per tick matches clip cook.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct FaceSample {
    /// Sampled tick.
    pub tick: Tick,
    /// Closed viseme id.
    pub shape: u8,
}

/// Sample `track` at `tick`. 50 ms/tick; empty tracks are rest.
#[must_use]
pub fn sample_face(track: &FaceTrack, tick: Tick) -> FaceSample {
    let now_ms =
        u16::try_from(tick.0.saturating_mul(50).min(u64::from(u16::MAX))).unwrap_or(u16::MAX);
    FaceSample {
        tick,
        shape: track.sample(now_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_anim::Viseme;

    #[test]
    fn rest_track_is_shape_zero() {
        let s = sample_face(&FaceTrack::rest(), Tick(3));
        assert_eq!(s.shape, 0);
        assert_eq!(s.tick, Tick(3));
    }

    #[test]
    fn keys_hold_across_ticks() {
        let track = FaceTrack {
            visemes: vec![
                Viseme { at_ms: 0, shape: 2 },
                Viseme {
                    at_ms: 100,
                    shape: 7,
                },
            ],
        };
        assert_eq!(sample_face(&track, Tick(0)).shape, 2);
        assert_eq!(sample_face(&track, Tick(1)).shape, 2);
        assert_eq!(sample_face(&track, Tick(2)).shape, 7);
    }
}
