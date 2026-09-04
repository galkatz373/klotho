//! Read-only anti-cheat inspect. Never writes Projection or emits proposals.

use std::collections::{BTreeMap, VecDeque};

use klotho_core::{PlayerId, Tick, YawMd};
use klotho_ir::{Analog, PlayerIntent, Verb};

/// Stick analog bound (inclusive). `i16` can exceed this.
pub const STICK_MAX: i16 = 1_000;
/// Look-yaw delta bound, millidegrees (inclusive).
pub const LOOK_YAW_MAX_MD: i32 = 180_000;
/// Look-pitch delta bound, millidegrees (inclusive).
pub const LOOK_PITCH_MAX_MD: i32 = 70_000;
/// Ticks treated as one second of cmd-rate (shooter 60 Hz).
const CMD_WINDOW_TICKS: u64 = 60;

/// Why inspect flagged an intent.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub enum SidecarFlag {
    /// In range, on time, under cmd-rate.
    #[default]
    None,
    /// Stick or look exceeded a documented bound; analog was clamped.
    AnalogRange,
    /// More intents in the 60-tick window than `intent_hz`.
    CmdRate,
    /// Fire/Use `at` older than `now - rewind_ticks`.
    StaleFire,
}

/// Clamped analog plus a flag. Disconnect is a recommendation only.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct SidecarReport {
    /// Analog after clamp.
    pub analog: Analog,
    /// First matching flag.
    pub flag: SidecarFlag,
}

impl SidecarReport {
    /// Cmd-rate and analog-range may disconnect. Stale Fire is a nack.
    #[must_use]
    pub fn recommend_disconnect(self) -> bool {
        matches!(self.flag, SidecarFlag::AnalogRange | SidecarFlag::CmdRate)
    }
}

/// Reads intents. Does not take a world or ingest proposals.
#[derive(Clone, Debug)]
pub struct Sidecar {
    intent_hz: u8,
    rewind_ticks: u16,
    recent: BTreeMap<PlayerId, VecDeque<Tick>>,
}

impl Sidecar {
    /// `intent_hz` from Hello; `rewind_ticks` from the sim budget.
    #[must_use]
    pub fn new(intent_hz: u8, rewind_ticks: u16) -> Self {
        Self {
            intent_hz: intent_hz.max(1),
            rewind_ticks,
            recent: BTreeMap::new(),
        }
    }

    /// Advertised intent rate.
    #[must_use]
    pub fn intent_hz(&self) -> u8 {
        self.intent_hz
    }

    /// Rewind cap used for the too-old Fire check.
    #[must_use]
    pub fn rewind_ticks(&self) -> u16 {
        self.rewind_ticks
    }

    /// Update the rewind cap (dedicated server budget).
    pub fn set_rewind_ticks(&mut self, rewind_ticks: u16) {
        self.rewind_ticks = rewind_ticks;
    }

    /// Clamp analog to the documented bounds.
    #[must_use]
    pub fn clamp_analog(a: Analog) -> Analog {
        Analog {
            phase: a.phase,
            stick_x: a.stick_x.clamp(-STICK_MAX, STICK_MAX),
            stick_z: a.stick_z.clamp(-STICK_MAX, STICK_MAX),
            look_yaw: YawMd(a.look_yaw.0.clamp(-LOOK_YAW_MAX_MD, LOOK_YAW_MAX_MD)),
            look_pitch: a.look_pitch.clamp(-LOOK_PITCH_MAX_MD, LOOK_PITCH_MAX_MD),
        }
    }

    /// Inspect one intent at sim `now`. Never writes world.
    pub fn inspect(&mut self, intent: &PlayerIntent, now: Tick) -> SidecarReport {
        let analog = Self::clamp_analog(intent.analog);
        let analog_flag = analog != intent.analog;
        let q = self.recent.entry(intent.player).or_default();
        q.push_back(now);
        let lo = now.0.saturating_sub(CMD_WINDOW_TICKS.saturating_sub(1));
        while q.front().is_some_and(|t| t.0 < lo) {
            q.pop_front();
        }
        let cmd_flag = q.len() > usize::from(self.intent_hz);
        let stale = self.rewind_ticks > 0
            && matches!(intent.verb, Verb::Fire | Verb::Use)
            && now.0.saturating_sub(intent.at.0) > u64::from(self.rewind_ticks);
        let flag = if cmd_flag {
            SidecarFlag::CmdRate
        } else if analog_flag {
            SidecarFlag::AnalogRange
        } else if stale {
            SidecarFlag::StaleFire
        } else {
            SidecarFlag::None
        };
        SidecarReport { analog, flag }
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{PlayerId, Tick, YawMd};
    use klotho_ir::{Agency, Analog, IntentTarget, PlayerIntent, Verb};

    use super::*;

    fn intent(verb: Verb, at: Tick, analog: Analog) -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(0),
            at,
            verb,
            target: IntentTarget::None,
            analog,
            agency: Agency::none(),
        }
    }

    #[test]
    fn analog_over_clamp_flags_and_clamps() {
        let mut s = Sidecar::new(20, 12);
        let raw = Analog {
            stick_x: 4_000,
            stick_z: -4_000,
            look_yaw: YawMd(200_000),
            look_pitch: 90_000,
            phase: 0,
        };
        let r = s.inspect(&intent(Verb::Look, Tick(0), raw), Tick(0));
        assert_eq!(r.flag, SidecarFlag::AnalogRange);
        assert!(r.recommend_disconnect());
        assert_eq!(r.analog.stick_x, STICK_MAX);
        assert_eq!(r.analog.stick_z, -STICK_MAX);
        assert_eq!(r.analog.look_yaw, YawMd(LOOK_YAW_MAX_MD));
        assert_eq!(r.analog.look_pitch, LOOK_PITCH_MAX_MD);
    }

    #[test]
    fn too_old_fire_flags_stale() {
        let mut s = Sidecar::new(20, 12);
        let r = s.inspect(&intent(Verb::Fire, Tick(0), Analog::default()), Tick(20));
        assert_eq!(r.flag, SidecarFlag::StaleFire);
        assert!(!r.recommend_disconnect());
    }

    #[test]
    fn cmd_rate_burst_flags() {
        let mut s = Sidecar::new(3, 12);
        let pi = intent(Verb::Move, Tick(0), Analog::default());
        let mut last = SidecarFlag::None;
        for _ in 0..4 {
            last = s.inspect(&pi, Tick(0)).flag;
        }
        assert_eq!(last, SidecarFlag::CmdRate);
    }

    #[test]
    fn sidecar_source_has_no_write_path() {
        let src = include_str!("sidecar.rs");
        assert!(!src.contains(concat!("World", "Mut")));
        assert!(!src.contains(concat!("klotho", "_world")));
        assert!(!src.contains(concat!("Propo", "sal")));
        assert!(!src.contains(concat!("Commit", "Kernel")));
        assert!(!src.contains(concat!(".ing", "est")));
    }
}
