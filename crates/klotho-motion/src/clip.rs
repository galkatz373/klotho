//! Cooked verb→clip table. v1 is one clip per `(Verb, grounded)`; not motion matching.

use klotho_core::{IVec3, Tick};
use klotho_ir::Verb;
use serde::{Deserialize, Serialize};

/// Walk root used by [`ClipSet::hearth`]. Same millimetres-per-tick as space tests.
pub const WALK_MM_PER_TICK: i32 = 20;

/// One looping or one-shot clip. Samples are local millimetre root deltas (+Z forward).
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    /// Opaque table index carried on `MotionDelta::clip`.
    pub id: u16,
    /// Verb this clip is selected for.
    pub verb: Verb,
    /// Grounded selector. v1 Hearth is always grounded.
    pub grounded: bool,
    /// Loop (walk) vs one-shot (Use). One-shots clamp to the last sample.
    pub looping: bool,
    /// Per-tick root translation, clip-local millimetres.
    pub samples: Vec<IVec3>,
}

impl Clip {
    /// Root delta at `tick`. Empty clips are zero. Debug T-pose is a zero sample.
    #[must_use]
    pub fn sample(&self, tick: Tick) -> IVec3 {
        if self.samples.is_empty() {
            return IVec3::ZERO;
        }
        let n = self.samples.len();
        let i = if self.looping {
            (tick.0 as usize) % n
        } else {
            (tick.0 as usize).min(n - 1)
        };
        self.samples[i]
    }

    /// Debug T-pose: identity, no root.
    #[must_use]
    pub fn tpose(id: u16, verb: Verb) -> Self {
        Self {
            id,
            verb,
            grounded: true,
            looping: true,
            samples: vec![IVec3::ZERO],
        }
    }
}

/// Cooked `ArtifactKind::ClipSet`. Selection is deterministic `(verb, grounded)`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipSet {
    /// Clips in table order. Lookup scans; v1 tables are tiny.
    pub clips: Vec<Clip>,
}

impl ClipSet {
    /// Hearth biped: T-pose idle/Use, +Z walk [`WALK_MM_PER_TICK`].
    #[must_use]
    pub fn hearth() -> Self {
        Self {
            clips: vec![
                Clip::tpose(0, Verb::Look),
                Clip {
                    id: 1,
                    verb: Verb::Move,
                    grounded: true,
                    looping: true,
                    samples: vec![IVec3 {
                        x: 0,
                        y: 0,
                        z: WALK_MM_PER_TICK,
                    }],
                },
                Clip {
                    id: 2,
                    verb: Verb::Use,
                    grounded: true,
                    looping: false,
                    samples: vec![IVec3::ZERO],
                },
            ],
        }
    }

    /// Parse the Hearth fixture RON (lockstep with [`Self::hearth`]).
    /// Direct `ron` — not [`klotho_ir::from_ron`], which rewrites rite sugar.
    pub fn from_ron(src: &str) -> Result<Self, String> {
        ron::from_str(src).map_err(|e| e.to_string())
    }

    /// One looping walk clip with a constant +Z root. Tests use this for long sweeps.
    #[must_use]
    pub fn walk_mm(mm: i32) -> Self {
        let mut s = Self::hearth();
        s.clips[1].samples = vec![IVec3 { x: 0, y: 0, z: mm }];
        s
    }

    /// Deterministic `(verb, grounded)` lookup. Missing keys fall back to grounded Look (T-pose).
    #[must_use]
    pub fn lookup(&self, verb: Verb, grounded: bool) -> Option<&Clip> {
        self.clips
            .iter()
            .find(|c| c.verb == verb && c.grounded == grounded)
            .or_else(|| {
                self.clips
                    .iter()
                    .find(|c| c.verb == Verb::Look && c.grounded)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_matches_hearth() {
        let src = include_str!("../fixtures/hearth_biped.ron");
        let parsed = ClipSet::from_ron(src).expect("fixture");
        assert_eq!(parsed, ClipSet::hearth());
    }

    #[test]
    fn looping_wraps() {
        let c = &ClipSet::hearth().clips[1];
        assert_eq!(c.sample(Tick(0)).z, WALK_MM_PER_TICK);
        assert_eq!(c.sample(Tick(1)).z, WALK_MM_PER_TICK);
    }

    #[test]
    fn missing_verb_is_tpose() {
        let s = ClipSet::hearth();
        let c = s.lookup(Verb::Fire, true).unwrap();
        assert_eq!(c.id, 0);
        assert_eq!(c.sample(Tick(0)), IVec3::ZERO);
    }
}
