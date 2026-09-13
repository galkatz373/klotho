//! Facial viseme tracks. Presentation palettes; never Rite notifies.

/// One viseme key. Time is milliseconds from the VO grain start.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Viseme {
    /// Offset from VO start, milliseconds.
    pub at_ms: u16,
    /// Closed viseme id `0..=15` (rest … W).
    pub shape: u8,
}

/// Lip-sync / facial track sampled by audio and cinematic presenters.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct FaceTrack {
    /// Keys in increasing `at_ms` order.
    pub visemes: Vec<Viseme>,
}

impl FaceTrack {
    /// Rest pose, no keys.
    #[must_use]
    pub const fn rest() -> Self {
        Self {
            visemes: Vec::new(),
        }
    }

    /// Fail closed on unsorted keys or out-of-range shapes.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let mut prev = None;
        for v in &self.visemes {
            if v.shape > 15 {
                return false;
            }
            if let Some(p) = prev {
                if v.at_ms <= p {
                    return false;
                }
            }
            prev = Some(v.at_ms);
        }
        true
    }

    /// Last viseme whose `at_ms <= now_ms`. Empty is rest (0).
    #[must_use]
    pub fn sample(&self, now_ms: u16) -> u8 {
        let mut shape = 0u8;
        for v in &self.visemes {
            if v.at_ms > now_ms {
                break;
            }
            shape = v.shape;
        }
        shape
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_holds_last_key() {
        let t = FaceTrack {
            visemes: vec![
                Viseme { at_ms: 0, shape: 1 },
                Viseme {
                    at_ms: 80,
                    shape: 4,
                },
                Viseme {
                    at_ms: 160,
                    shape: 0,
                },
            ],
        };
        assert!(t.is_valid());
        assert_eq!(t.sample(0), 1);
        assert_eq!(t.sample(80), 4);
        assert_eq!(t.sample(120), 4);
        assert_eq!(t.sample(160), 0);
        assert_eq!(FaceTrack::rest().sample(50), 0);
    }

    #[test]
    fn unsorted_or_wide_shape_is_invalid() {
        let bad = FaceTrack {
            visemes: vec![
                Viseme {
                    at_ms: 10,
                    shape: 1,
                },
                Viseme {
                    at_ms: 10,
                    shape: 2,
                },
            ],
        };
        assert!(!bad.is_valid());
        let wide = FaceTrack {
            visemes: vec![Viseme {
                at_ms: 0,
                shape: 16,
            }],
        };
        assert!(!wide.is_valid());
    }
}
