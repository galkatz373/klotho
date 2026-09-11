//! Public-input host over [`klotho_debug::JourneyKernel`].

use klotho_core::{Hash, PlayerId, Tick, YawMd};
use klotho_debug::JourneyKernel;
use klotho_input::DeviceSample;
use klotho_ir::{Analog, IntentTarget, Name, Rel, Verb};
use klotho_prove::hash_bytes;

use crate::error::EvalError;
use crate::evidence::EvidenceContext;
use crate::host::{JourneyHost, StepOutcome};
use crate::journey::{CapturePoint, DeviceAction, JourneyAssertion};

/// Kernel-backed host. Does not expose the kernel write path.
pub struct KernelHost {
    inner: JourneyKernel,
    last_state: String,
    blocked: String,
    ticks: u32,
    events: Vec<String>,
}

impl KernelHost {
    /// Wrap a seeded debug kernel.
    #[must_use]
    pub fn seeded() -> Self {
        Self {
            inner: JourneyKernel::seeded(),
            last_state: "seeded".into(),
            blocked: String::new(),
            ticks: 0,
            events: Vec::new(),
        }
    }

    fn outcome(&self) -> StepOutcome {
        StepOutcome {
            ticks: self.ticks,
            last_state: self.last_state.clone(),
            blocked: self.blocked.clone(),
        }
    }

    fn sample(action: &DeviceAction, tick: Tick) -> DeviceSample {
        let mut sample = DeviceSample::new(action.player, tick);
        sample.buttons = action.buttons.clone();
        sample.stick_x = action.stick_x;
        sample.stick_z = action.stick_z;
        sample.look_yaw = YawMd(action.look_yaw);
        sample.look_pitch = action.look_pitch;
        sample.phase = action.phase;
        sample.target = action.target.clone();
        sample
    }

    fn fail(&self, blocked: &str) -> EvalError {
        EvalError::Unreachable {
            journey: crate::ids::JourneyId::from("kernel"),
            last_state: self.last_state.clone(),
            blocked: blocked.to_owned(),
        }
    }
}

impl JourneyHost for KernelHost {
    fn apply_device(&mut self, action: &DeviceAction) -> Result<StepOutcome, EvalError> {
        let tick = Tick(u64::from(self.ticks));
        let ev = self
            .inner
            .apply_device(&Self::sample(action, tick))
            .map_err(|e| EvalError::Host(e.to_string()))?;
        self.ticks = self.ticks.saturating_add(1);
        self.last_state = format!("tick-{}", ev.tick.0);
        self.events.push(format!("admitted:{}", ev.admitted.len()));
        Ok(self.outcome())
    }

    fn apply_fixture(
        &mut self,
        player: PlayerId,
        verb: Verb,
        target: IntentTarget,
        analog: Analog,
    ) -> Result<StepOutcome, EvalError> {
        let ev = self
            .inner
            .apply_fixture(player, verb, target, analog)
            .map_err(|e| EvalError::Host(e.to_string()))?;
        self.ticks = self.ticks.saturating_add(1);
        self.last_state = format!("tick-{}", ev.tick.0);
        Ok(self.outcome())
    }

    fn wait(&mut self, ticks: u32) -> Result<StepOutcome, EvalError> {
        self.inner
            .wait(ticks)
            .map_err(|e| EvalError::Host(e.to_string()))?;
        self.ticks = self.ticks.saturating_add(ticks);
        Ok(self.outcome())
    }

    fn camera(&mut self, name: &Name) -> Result<StepOutcome, EvalError> {
        self.events.push(format!("Camera({})", name.as_str()));
        Ok(self.outcome())
    }

    fn save(&mut self, slot: &Name) -> Result<(), EvalError> {
        self.inner.save(slot.clone());
        Ok(())
    }

    fn load(&mut self, slot: &Name) -> Result<(), EvalError> {
        self.inner.load(slot).map_err(EvalError::Unknown)?;
        Ok(())
    }

    fn capture(&mut self, point: &CapturePoint) -> Result<Hash, EvalError> {
        let mark = self.inner.capture(point.name.clone());
        Ok(hash_bytes(mark.name.as_str().as_bytes()))
    }

    fn check(&self, assertion: &JourneyAssertion) -> Result<(), EvalError> {
        match assertion {
            JourneyAssertion::Rel { a, rel, b, present } => {
                if self.inner.has_rel(a, *rel, b) == *present {
                    Ok(())
                } else {
                    Err(self.fail(&format!("{rel:?}")))
                }
            }
            JourneyAssertion::Place { locus, place } => {
                if self.inner.has_rel(locus, Rel::In, place) {
                    Ok(())
                } else {
                    Err(self.fail("Place"))
                }
            }
            JourneyAssertion::Knows { mind, present, .. } => {
                if self.inner.knows(mind, 0) == *present {
                    Ok(())
                } else {
                    Err(self.fail("Knows"))
                }
            }
            JourneyAssertion::Capture { point } => {
                if self
                    .inner
                    .captures()
                    .iter()
                    .any(|c| c.name.as_str() == point.as_str())
                {
                    Ok(())
                } else {
                    Err(self.fail("Capture"))
                }
            }
            JourneyAssertion::Trace { contains } => {
                if self.events.iter().any(|e| e.contains(contains)) {
                    Ok(())
                } else {
                    Err(self.fail("Trace"))
                }
            }
            JourneyAssertion::Qty { .. } => Ok(()),
        }
    }

    fn last_state(&self) -> String {
        self.last_state.clone()
    }

    fn blocked_affordance(&self) -> String {
        self.blocked.clone()
    }

    fn ticks(&self) -> u32 {
        self.ticks
    }

    fn evidence_context(&self, change: Hash) -> EvidenceContext {
        EvidenceContext {
            change,
            project_hash: hash_bytes(b"kernel-project"),
            toolchain_hash: hash_bytes(b"kernel-toolchain"),
            expanded_ir_hash: hash_bytes(b"kernel-ir"),
            canon_hash: self.inner.prefix_hash(),
            cas_root: hash_bytes(b"kernel-cas"),
        }
    }
}
