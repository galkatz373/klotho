//! Bounded buffer, coyote, curves, aim-assist, and latency capture (KAI-10).

use std::collections::VecDeque;

use klotho_core::{PlayerId, Tick, YawMd};
use klotho_ir::{
    AimAssistContract, Analog, FeelContract, IntentTarget, PlayerIntent, TickWindow, Verb,
};

use crate::{BindTable, DeviceSample, InputMapper};

/// Pinned first-title wired-controller / reference-display lane.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct DeviceLane {
    /// Stable lane id.
    pub id: &'static str,
    /// Device poll rate.
    pub polling_hz: u16,
    /// Reference display refresh.
    pub display_hz: u16,
    /// Authoritative tick length, microseconds (`AaaAdventure` = 30 Hz).
    pub sim_tick_us: u32,
    /// Input sample → local presentation p95, microseconds.
    pub present_p95_us: u32,
    /// Extra microseconds allowed on top of one sim tick for authority p95.
    pub extra_authority_us: u32,
}

impl DeviceLane {
    /// First-title wired controller on the pinned 1080p60 reference display.
    pub const FIRST_TITLE_WIRED: Self = Self {
        id: "first-title-wired-controller",
        polling_hz: 125,
        display_hz: 60,
        sim_tick_us: 33_333,
        present_p95_us: 25_000,
        extra_authority_us: 8_000,
    };

    /// Input sample → authoritative response p95.
    #[must_use]
    pub const fn authority_p95_us(self) -> u32 {
        self.sim_tick_us.saturating_add(self.extra_authority_us)
    }
}

/// One captured latency triple. Timestamps are caller-supplied microseconds.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct LatencySample {
    /// Device sample time.
    pub sample_us: u64,
    /// Mapper produced [`PlayerIntent`].
    pub intent_us: u64,
    /// Local presentation responded.
    pub present_us: u64,
    /// Kernel admitted the packet (or equivalent authority edge).
    pub admit_us: u64,
}

impl LatencySample {
    /// Sample → presentation.
    #[must_use]
    pub const fn present_latency_us(self) -> u64 {
        self.present_us.saturating_sub(self.sample_us)
    }

    /// Sample → authority.
    #[must_use]
    pub const fn authority_latency_us(self) -> u64 {
        self.admit_us.saturating_sub(self.sample_us)
    }
}

/// Ordered latency log. p95 is nearest-rank on the sorted samples.
#[derive(Clone, Debug, Default)]
pub struct LatencyLog {
    samples: Vec<LatencySample>,
}

impl LatencyLog {
    /// Empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one measured triple.
    pub fn push(&mut self, sample: LatencySample) {
        self.samples.push(sample);
    }

    /// Recorded samples, capture order.
    #[must_use]
    pub fn samples(&self) -> &[LatencySample] {
        &self.samples
    }

    /// Nearest-rank p95 of `f`, or 0 when empty.
    #[must_use]
    pub fn p95_us(&self, f: impl Fn(LatencySample) -> u64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut v: Vec<u64> = self.samples.iter().copied().map(f).collect();
        v.sort_unstable();
        let idx = ((v.len() * 95).div_ceil(100))
            .saturating_sub(1)
            .min(v.len() - 1);
        v[idx]
    }

    /// Fail closed against `lane`. Empty logs fail: no evidence is not a pass.
    pub fn meets(&self, lane: DeviceLane) -> Result<(), FeelGate> {
        if self.samples.is_empty() {
            return Err(FeelGate::NoSamples);
        }
        let present = self.p95_us(LatencySample::present_latency_us);
        if present > u64::from(lane.present_p95_us) {
            return Err(FeelGate::PresentP95 {
                us: present,
                cap: lane.present_p95_us,
            });
        }
        let authority = self.p95_us(LatencySample::authority_latency_us);
        if authority > u64::from(lane.authority_p95_us()) {
            return Err(FeelGate::AuthorityP95 {
                us: authority,
                cap: lane.authority_p95_us(),
            });
        }
        Ok(())
    }
}

/// Why a feel/latency gate failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum FeelGate {
    /// No samples were recorded.
    NoSamples,
    /// Presentation p95 missed the lane.
    PresentP95 {
        /// Observed microseconds.
        us: u64,
        /// Lane cap.
        cap: u32,
    },
    /// Authority p95 missed the lane.
    AuthorityP95 {
        /// Observed microseconds.
        us: u64,
        /// Lane cap.
        cap: u32,
    },
    /// Pending buffer exceeded the authored window (unexplained queue).
    UnexplainedQueue {
        /// Authored buffer ticks.
        authored: u8,
        /// Observed pending age in ticks.
        pending_age: u8,
    },
}

impl core::fmt::Display for FeelGate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSamples => f.write_str("feel gate has no latency samples"),
            Self::PresentP95 { us, cap } => {
                write!(f, "presentation p95 {us}us exceeds {cap}us")
            }
            Self::AuthorityP95 { us, cap } => {
                write!(f, "authority p95 {us}us exceeds {cap}us")
            }
            Self::UnexplainedQueue {
                authored,
                pending_age,
            } => write!(
                f,
                "input queue age {pending_age} exceeds authored buffer {authored}"
            ),
        }
    }
}

impl std::error::Error for FeelGate {}

/// Pull analog look toward a target yaw error. Never writes Projection.
#[must_use]
pub fn apply_aim_assist(analog: Analog, yaw_error_md: i32, contract: AimAssistContract) -> Analog {
    let abs_err = yaw_error_md.unsigned_abs();
    if abs_err > contract.cone_md || contract.magnet_permille == 0 {
        return analog;
    }
    let pull = (i64::from(yaw_error_md) * i64::from(contract.magnet_permille)) / 1000;
    let capped = pull.clamp(
        -i64::from(contract.max_correction_md),
        i64::from(contract.max_correction_md),
    ) as i32;
    Analog {
        look_yaw: analog.look_yaw.wrapping_add(YawMd(capped)),
        ..analog
    }
}

/// Mapper that applies a [`FeelContract`] around [`InputMapper`].
pub struct FeelMapper {
    inner: InputMapper,
    contract: FeelContract,
    pending: VecDeque<(Tick, Verb)>,
    last_grounded: Option<Tick>,
    action_start: Option<Tick>,
    toggle_down: bool,
}

impl FeelMapper {
    /// Wrap `inner` with `contract`. Validates the contract.
    pub fn new(inner: InputMapper, contract: FeelContract) -> Result<Self, klotho_ir::IrError> {
        contract.validate()?;
        Ok(Self {
            inner,
            contract,
            pending: VecDeque::new(),
            last_grounded: None,
            action_start: None,
            toggle_down: false,
        })
    }

    /// Hearth binds plus the Spindle Use suite.
    pub fn spindle() -> Result<Self, klotho_ir::IrError> {
        Self::new(InputMapper::hearth(), FeelContract::spindle_use())
    }

    /// Contract in use.
    #[must_use]
    pub fn contract(&self) -> &FeelContract {
        &self.contract
    }

    /// Bind table in use.
    #[must_use]
    pub fn table(&self) -> &BindTable {
        self.inner.table()
    }

    /// Pending discrete verbs, oldest first.
    pub fn pending(&self) -> impl Iterator<Item = (Tick, Verb)> + '_ {
        self.pending.iter().copied()
    }

    /// Map a sample. Curves, buffer, coyote, cancel/combo, and aim-assist apply.
    pub fn map(&mut self, sample: &DeviceSample) -> Result<PlayerIntent, FeelGate> {
        if sample.grounded {
            self.last_grounded = Some(sample.tick);
        }
        self.expire(sample.tick)?;
        let mut intent = self.inner.map(sample);
        if sample.stick_x == 0 && sample.stick_z == 0 {
            intent.analog.stick_x = self.contract.decel_curve.apply_i16(sample.stick_x);
            intent.analog.stick_z = self.contract.decel_curve.apply_i16(sample.stick_z);
        } else {
            intent.analog.stick_x = self.contract.accel_curve.apply_i16(sample.stick_x);
            intent.analog.stick_z = self.contract.accel_curve.apply_i16(sample.stick_z);
        }
        if self.contract.accessibility.hold_to_toggle {
            intent = self.apply_toggle(sample, intent);
        }
        if let Some(assist) = self.contract.aim_assist {
            intent.analog = apply_aim_assist(intent.analog, sample.aim_yaw_error_md, assist);
        }
        let discrete = is_discrete(intent.verb);
        if discrete {
            self.pending.push_back((sample.tick, intent.verb));
        }
        if let Some(verb) = self.release(sample.tick) {
            intent.verb = verb;
            if self.action_start.is_none() {
                self.action_start = Some(sample.tick);
            }
        } else if discrete {
            intent.verb = if sample.stick_x != 0 || sample.stick_z != 0 {
                Verb::Move
            } else {
                Verb::Look
            };
        }
        Ok(intent)
    }

    /// Stamp a verb through the inner table. Feel windows still apply.
    pub fn stamp(
        &mut self,
        player: PlayerId,
        tick: Tick,
        verb: Verb,
        target: IntentTarget,
        analog: Analog,
        grounded: bool,
    ) -> Result<PlayerIntent, FeelGate> {
        let mut sample = DeviceSample::new(player, tick);
        sample.grounded = grounded;
        sample.target = target;
        sample.stick_x = analog.stick_x;
        sample.stick_z = analog.stick_z;
        sample.look_yaw = analog.look_yaw;
        sample.look_pitch = analog.look_pitch;
        sample.phase = analog.phase;
        if is_discrete(verb) {
            // Reconstruct a button so the inner mapper emits `verb`.
            if let Some(bind) = self
                .inner
                .table()
                .bindings()
                .iter()
                .find(|b| b.verb == verb)
            {
                sample.buttons.insert(bind.button);
            }
        }
        self.map(&sample)
    }

    fn apply_toggle(&mut self, sample: &DeviceSample, mut intent: PlayerIntent) -> PlayerIntent {
        let held = is_discrete(intent.verb);
        if held && !self.toggle_down {
            self.toggle_down = true;
        } else if held && self.toggle_down {
            intent.verb = Verb::Look;
        } else {
            self.toggle_down = false;
        }
        let _ = sample;
        intent
    }

    fn expire(&mut self, now: Tick) -> Result<(), FeelGate> {
        let cap = u64::from(self.contract.input_buffer_ticks);
        if let Some((at, _)) = self.pending.front() {
            let age = now.0.saturating_sub(at.0);
            if age > cap {
                return Err(FeelGate::UnexplainedQueue {
                    authored: self.contract.input_buffer_ticks,
                    pending_age: u8::try_from(age).unwrap_or(u8::MAX),
                });
            }
        }
        Ok(())
    }

    fn coyote(&self, now: Tick) -> bool {
        match self.last_grounded {
            Some(at) => now.0.saturating_sub(at.0) <= u64::from(self.contract.coyote_ticks),
            None => false,
        }
    }

    fn release(&mut self, now: Tick) -> Option<Verb> {
        let elapsed = self
            .action_start
            .map(|start| u8::try_from(now.0.saturating_sub(start.0)).unwrap_or(u8::MAX));
        let idx = self.pending.iter().position(|(_, verb)| {
            self.window_allows(*verb, elapsed) && self.verb_allowed_now(*verb, now)
        })?;
        let (_, verb) = self.pending.remove(idx)?;
        Some(verb)
    }

    fn window_allows(&self, verb: Verb, elapsed: Option<u8>) -> bool {
        let Some(elapsed) = elapsed else {
            return true;
        };
        if self
            .contract
            .cancel_windows
            .iter()
            .any(|w: &TickWindow| w.verb == verb && w.contains(elapsed))
        {
            return true;
        }
        if self
            .contract
            .combo_windows
            .iter()
            .any(|w| w.verb == verb && w.contains(elapsed))
        {
            return true;
        }
        elapsed == 0
    }

    fn verb_allowed_now(&self, verb: Verb, now: Tick) -> bool {
        if matches!(verb, Verb::Move | Verb::Look | Verb::Steer) {
            return true;
        }
        // Grounded actions (Use/Open) honour coyote. Ungrounded Fire still fires.
        if matches!(verb, Verb::Use | Verb::Open | Verb::Carry | Verb::Drop) {
            return self.coyote(now) || self.last_grounded == Some(now);
        }
        true
    }
}

fn is_discrete(verb: Verb) -> bool {
    !matches!(verb, Verb::Move | Verb::Look | Verb::Steer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Button, DeviceSample};
    use klotho_core::{PlayerId, Tick};
    use klotho_ir::{FeelAccessibility, FeelContract, IntentTarget, Verb};

    #[test]
    fn first_title_lane_is_25ms_present_and_tick_plus_8ms_authority() {
        let lane = DeviceLane::FIRST_TITLE_WIRED;
        assert_eq!(lane.present_p95_us, 25_000);
        assert_eq!(lane.authority_p95_us(), 33_333 + 8_000);
    }

    #[test]
    fn latency_p95_meets_lane() {
        let mut log = LatencyLog::new();
        for i in 0..20 {
            log.push(LatencySample {
                sample_us: i * 1_000,
                intent_us: i * 1_000 + 1_000,
                present_us: i * 1_000 + 8_000,
                admit_us: i * 1_000 + 20_000,
            });
        }
        log.meets(DeviceLane::FIRST_TITLE_WIRED).unwrap();
    }

    #[test]
    fn latency_p95_fails_when_present_is_late() {
        let mut log = LatencyLog::new();
        log.push(LatencySample {
            sample_us: 0,
            intent_us: 1_000,
            present_us: 40_000,
            admit_us: 10_000,
        });
        assert!(matches!(
            log.meets(DeviceLane::FIRST_TITLE_WIRED),
            Err(FeelGate::PresentP95 { .. })
        ));
    }

    #[test]
    fn empty_log_fails_closed() {
        assert_eq!(
            LatencyLog::new().meets(DeviceLane::FIRST_TITLE_WIRED),
            Err(FeelGate::NoSamples)
        );
    }

    #[test]
    fn coyote_keeps_use_after_leaving_support() {
        let mut mapper = FeelMapper::spindle().unwrap();
        let mut grounded = DeviceSample::new(PlayerId(0), Tick(0));
        grounded.grounded = true;
        assert_eq!(mapper.map(&grounded).unwrap().verb, Verb::Look);

        let mut air = DeviceSample::new(PlayerId(0), Tick(1));
        air.grounded = false;
        air.buttons.insert(Button::KeyE);
        let coyote = mapper.map(&air).unwrap();
        assert_eq!(coyote.verb, Verb::Use);
    }

    #[test]
    fn coyote_expires() {
        let mut mapper = FeelMapper::spindle().unwrap();
        let mut grounded = DeviceSample::new(PlayerId(0), Tick(0));
        grounded.grounded = true;
        mapper.map(&grounded).unwrap();
        let mut air = DeviceSample::new(PlayerId(0), Tick(3));
        air.buttons.insert(Button::KeyE);
        let late = mapper.map(&air).unwrap();
        assert_eq!(late.verb, Verb::Look);
    }

    #[test]
    fn buffer_older_than_authored_window_is_unexplained_queue() {
        let mut mapper = FeelMapper::spindle().unwrap();
        let mut s = DeviceSample::new(PlayerId(0), Tick(0));
        s.buttons.insert(Button::KeyE);
        // Not grounded and no coyote: Use is buffered, not released.
        assert_eq!(mapper.map(&s).unwrap().verb, Verb::Look);
        let later = DeviceSample::new(PlayerId(0), Tick(3));
        assert!(matches!(
            mapper.map(&later),
            Err(FeelGate::UnexplainedQueue { .. })
        ));
    }

    #[test]
    fn linear_curve_preserves_full_stick() {
        let mut mapper = FeelMapper::spindle().unwrap();
        let mut s = DeviceSample::new(PlayerId(0), Tick(0));
        s.stick_z = 32_767;
        let p = mapper.map(&s).unwrap();
        assert_eq!(p.verb, Verb::Move);
        assert_eq!(p.analog.stick_z, 32_767);
    }

    #[test]
    fn aim_assist_pulls_inside_cone() {
        let analog = Analog {
            look_yaw: YawMd(0),
            ..Analog::default()
        };
        let out = apply_aim_assist(
            analog,
            4_000,
            AimAssistContract {
                magnet_permille: 500,
                cone_md: 8_000,
                max_correction_md: 3_000,
            },
        );
        assert_eq!(out.look_yaw, YawMd(2_000));
    }

    #[test]
    fn aim_assist_ignores_outside_cone() {
        let analog = Analog::default();
        let out = apply_aim_assist(
            analog,
            20_000,
            AimAssistContract {
                magnet_permille: 500,
                cone_md: 8_000,
                max_correction_md: 3_000,
            },
        );
        assert_eq!(out.look_yaw, YawMd(0));
    }

    #[test]
    fn hold_to_toggle_emits_once() {
        let mut contract = FeelContract::spindle_use();
        contract.accessibility = FeelAccessibility {
            hold_to_toggle: true,
            ..FeelAccessibility::default()
        };
        let mut mapper = FeelMapper::new(InputMapper::hearth(), contract).unwrap();
        let mut s = DeviceSample::new(PlayerId(0), Tick(0));
        s.grounded = true;
        s.buttons.insert(Button::KeyE);
        assert_eq!(mapper.map(&s).unwrap().verb, Verb::Use);
        s.tick = Tick(1);
        assert_eq!(mapper.map(&s).unwrap().verb, Verb::Look);
    }

    #[test]
    fn cancel_window_releases_drop() {
        let mut mapper = FeelMapper::spindle().unwrap();
        let mut use_s = DeviceSample::new(PlayerId(0), Tick(0));
        use_s.grounded = true;
        use_s.buttons.insert(Button::KeyE);
        assert_eq!(mapper.map(&use_s).unwrap().verb, Verb::Use);
        let mut drop_s = DeviceSample::new(PlayerId(0), Tick(1));
        drop_s.grounded = true;
        drop_s.buttons.insert(Button::KeyG);
        drop_s.target = IntentTarget::None;
        assert_eq!(mapper.map(&drop_s).unwrap().verb, Verb::Drop);
    }
}
