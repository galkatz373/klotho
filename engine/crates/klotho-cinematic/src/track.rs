//! Observer-track validation and sampling.

use core::fmt;

use klotho_ir::{Beat, LocusKind, Name};
use klotho_manifest::{Observer, Sigil, Tick};

/// What the host does with player-driven Phys while this Beat is active.
///
/// The choice is fixed for the whole track. This is host policy, not a pose
/// written into Projection.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum PlayerPhys {
    /// The kernel keeps stepping and player Phys proposals remain enabled.
    KeepStepping,
    /// The host suppresses player Phys proposals; the kernel clock still runs.
    StopPlayer,
}

/// One camera sample at an offset from the Beat's global start tick.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Keyframe {
    /// Tick offset. The first key must be [`Tick::ZERO`].
    pub at: Tick,
    /// Disposable presenter camera at this key.
    pub observer: Observer,
}

/// Disposable output for one cinematic presentation frame.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct CinematicManifest {
    /// Active author-facing Beat id.
    pub beat: Name,
    /// Observer locus represented by [`Self::observer`].
    pub observer_locus: Sigil,
    /// Sampled camera for render and audio presentation.
    pub observer: Observer,
    /// Global simulation tick used to sample the track.
    pub tick: Tick,
    /// Manifest-only HUD visibility flag.
    pub hide_hud: bool,
    /// Per-Beat player Phys policy.
    pub player_phys: PlayerPhys,
    /// True once the global clock reaches the final key.
    pub finished: bool,
}

/// Invalid Observer-track authoring data.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum TrackError {
    /// Camera identity must be a `LocusKind::Observer` Sigil.
    NotObserver(Sigil),
    /// A track needs at least one key.
    NoKeyframes,
    /// The first key must be at offset zero.
    FirstKeyNotZero(Tick),
    /// Key offsets must be strictly increasing.
    KeysNotIncreasing {
        /// Earlier offset.
        previous: Tick,
        /// Later offset.
        next: Tick,
    },
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotObserver(id) => write!(f, "cinematic camera {id} is not an Observer locus"),
            Self::NoKeyframes => f.write_str("cinematic track has no keyframes"),
            Self::FirstKeyNotZero(at) => write!(f, "first cinematic key is at {at}, not t0"),
            Self::KeysNotIncreasing { previous, next } => {
                write!(f, "cinematic keys are not increasing: {previous}, {next}")
            }
        }
    }
}

impl std::error::Error for TrackError {}

/// A validated camera track bound to one author-facing Beat.
///
/// The track is immutable. Call [`Self::sample`] with the current global tick;
/// there is deliberately no `advance`, time scale, or world handle.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ObserverTrack {
    beat: Name,
    observer_locus: Sigil,
    keys: Vec<Keyframe>,
    hide_hud: bool,
    player_phys: PlayerPhys,
}

impl ObserverTrack {
    /// Validate and bind a track to `beat`.
    pub fn new(
        beat: &Beat,
        observer_locus: Sigil,
        keys: Vec<Keyframe>,
        hide_hud: bool,
        player_phys: PlayerPhys,
    ) -> Result<Self, TrackError> {
        if observer_locus.kind() != Some(LocusKind::Observer) {
            return Err(TrackError::NotObserver(observer_locus));
        }
        let Some(first) = keys.first() else {
            return Err(TrackError::NoKeyframes);
        };
        if first.at != Tick::ZERO {
            return Err(TrackError::FirstKeyNotZero(first.at));
        }
        for pair in keys.windows(2) {
            if pair[0].at >= pair[1].at {
                return Err(TrackError::KeysNotIncreasing {
                    previous: pair[0].at,
                    next: pair[1].at,
                });
            }
        }
        Ok(Self {
            beat: beat.id.clone(),
            observer_locus,
            keys,
            hide_hud,
            player_phys,
        })
    }

    /// Beat id this track presents.
    #[must_use]
    pub fn beat(&self) -> &Name {
        &self.beat
    }

    /// Sample against the global clock.
    ///
    /// Returns `None` before `started_at` or when a different Beat is active.
    /// After the final key, the camera clamps to it and `finished` is true.
    #[must_use]
    pub fn sample(&self, active: &Beat, started_at: Tick, now: Tick) -> Option<CinematicManifest> {
        if active.id != self.beat || now < started_at {
            return None;
        }
        let elapsed = Tick(now - started_at);
        let last = self.keys.last().expect("validated non-empty track");
        let observer = match self.keys.partition_point(|key| key.at <= elapsed) {
            0 => self.keys[0].observer,
            upper if upper == self.keys.len() => last.observer,
            upper => interpolate(self.keys[upper - 1], self.keys[upper], elapsed),
        };
        Some(CinematicManifest {
            beat: self.beat.clone(),
            observer_locus: self.observer_locus,
            observer,
            tick: now,
            hide_hud: self.hide_hud,
            player_phys: self.player_phys,
            finished: elapsed >= last.at,
        })
    }
}

fn interpolate(a: Keyframe, b: Keyframe, at: Tick) -> Observer {
    let numerator = at - a.at;
    let denominator = b.at - a.at;
    let mut eye = a.observer.eye;
    eye.x.0 = lerp_i32(eye.x.0, b.observer.eye.x.0, numerator, denominator);
    eye.y.0 = lerp_i32(eye.y.0, b.observer.eye.y.0, numerator, denominator);
    eye.z.0 = lerp_i32(eye.z.0, b.observer.eye.z.0, numerator, denominator);
    eye.yaw.0 = lerp_angle(eye.yaw.0, b.observer.eye.yaw.0, numerator, denominator);
    eye.pitch.0 = lerp_angle(eye.pitch.0, b.observer.eye.pitch.0, numerator, denominator);
    eye.roll.0 = lerp_angle(eye.roll.0, b.observer.eye.roll.0, numerator, denominator);
    Observer {
        eye,
        pitch_md: lerp_i32(
            a.observer.pitch_md,
            b.observer.pitch_md,
            numerator,
            denominator,
        )
        .clamp(Observer::PITCH_MIN_MD, Observer::PITCH_MAX_MD),
    }
}

fn lerp_i32(a: i32, b: i32, numerator: u64, denominator: u64) -> i32 {
    let delta = i128::from(b) - i128::from(a);
    let step = delta * i128::from(numerator) / i128::from(denominator);
    i32::try_from(i128::from(a) + step).unwrap_or(if step.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

fn lerp_angle(a: i32, b: i32, numerator: u64, denominator: u64) -> i32 {
    const FULL: i32 = 360_000;
    const HALF: i32 = 180_000;
    let a = a.rem_euclid(FULL);
    let mut delta = b.rem_euclid(FULL) - a;
    if delta > HALF {
        delta -= FULL;
    } else if delta <= -HALF {
        delta += FULL;
    }
    lerp_i32(a, a + delta, numerator, denominator).rem_euclid(FULL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(id: &str) -> Beat {
        Beat {
            id: Name::from(id),
            notes: String::new(),
        }
    }

    fn camera() -> Sigil {
        Sigil::pack(LocusKind::Observer, 0, 1).unwrap()
    }

    fn observer(x: i32, yaw: i32) -> Observer {
        let mut observer = Observer::origin();
        observer.eye.x.0 = x;
        observer.eye.yaw.0 = yaw;
        observer
    }

    #[test]
    fn samples_global_tick_without_owning_a_clock() {
        let beat = beat("arrival");
        let track = ObserverTrack::new(
            &beat,
            camera(),
            vec![
                Keyframe {
                    at: Tick(0),
                    observer: observer(0, 350_000),
                },
                Keyframe {
                    at: Tick(10),
                    observer: observer(1_000, 10_000),
                },
            ],
            true,
            PlayerPhys::StopPlayer,
        )
        .unwrap();

        assert!(track.sample(&beat, Tick(100), Tick(99)).is_none());
        let frame = track.sample(&beat, Tick(100), Tick(105)).unwrap();
        assert_eq!(frame.tick, Tick(105));
        assert_eq!(frame.observer.eye.x.0, 500);
        assert_eq!(frame.observer.eye.yaw.0, 0);
        assert!(frame.hide_hud);
        assert_eq!(frame.player_phys, PlayerPhys::StopPlayer);
        assert!(!frame.finished);

        let end = track.sample(&beat, Tick(100), Tick(110)).unwrap();
        assert_eq!(end.observer, observer(1_000, 10_000));
        assert!(end.finished);
    }

    #[test]
    fn only_the_bound_beat_drives_the_track() {
        let arrival = beat("arrival");
        let other = beat("combat");
        let track = ObserverTrack::new(
            &arrival,
            camera(),
            vec![Keyframe {
                at: Tick::ZERO,
                observer: observer(0, 0),
            }],
            false,
            PlayerPhys::KeepStepping,
        )
        .unwrap();
        assert!(track.sample(&other, Tick::ZERO, Tick::ZERO).is_none());
    }

    #[test]
    fn rejects_non_observer_and_ambiguous_key_order() {
        let beat = beat("arrival");
        let actor = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        assert_eq!(
            ObserverTrack::new(&beat, actor, vec![], false, PlayerPhys::KeepStepping),
            Err(TrackError::NotObserver(actor))
        );
        assert_eq!(
            ObserverTrack::new(&beat, camera(), vec![], false, PlayerPhys::KeepStepping),
            Err(TrackError::NoKeyframes)
        );
        assert!(matches!(
            ObserverTrack::new(
                &beat,
                camera(),
                vec![Keyframe {
                    at: Tick(1),
                    observer: observer(0, 0),
                }],
                false,
                PlayerPhys::KeepStepping,
            ),
            Err(TrackError::FirstKeyNotZero(Tick(1)))
        ));
        assert!(matches!(
            ObserverTrack::new(
                &beat,
                camera(),
                vec![
                    Keyframe {
                        at: Tick::ZERO,
                        observer: observer(0, 0),
                    },
                    Keyframe {
                        at: Tick::ZERO,
                        observer: observer(1, 0),
                    },
                ],
                false,
                PlayerPhys::KeepStepping,
            ),
            Err(TrackError::KeysNotIncreasing { .. })
        ));
    }
}
