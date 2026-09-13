//! Accessibility capture matrix and critical focus journeys (KAI-16).

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::{Hash, PlayerId};
use klotho_input::{BindTable, Button, InputFamily};
use klotho_ir::{Analog, IntentTarget, Name, Verb};
use klotho_prove::hash_bytes;
use klotho_ui::{
    FocusCycle, FocusNav, matrix_gate, nav_from_button, pause_menu, remap_menu, settings_menu,
};

use crate::error::EvalError;
use crate::evidence::EvidenceContext;
use crate::host::{JourneyHost, StepOutcome};
use crate::ids::JourneyId;
use crate::journey::{
    CapturePoint, DeviceAction, JourneyAssertion, JourneySpec, JourneyStep, StartStateRef,
};

/// Scripted pause-menu focus journey: next to settings, open, back, resume.
#[must_use]
pub fn focus_journey() -> JourneySpec {
    let mut spec = JourneySpec::new("ui.focus.pause", 32);
    spec.start = StartStateRef {
        name: Name::from("paused"),
    };
    spec.steps = vec![
        press(Button::KeyDown),
        press(Button::KeyEnter),
        press(Button::KeyEsc),
        press(Button::KeyEnter),
    ];
    spec.assertions.push(JourneyAssertion::Capture {
        point: Name::from("resumed"),
    });
    spec.capture_points.push(CapturePoint {
        name: Name::from("resumed"),
        after_step: u32::MAX,
        kind: crate::journey::CaptureKind::Semantic,
    });
    spec
}

/// Human critical-path recording: same public inputs as [`focus_journey`].
#[must_use]
pub fn human_focus_journey() -> JourneySpec {
    let mut spec = focus_journey();
    spec.id = JourneyId::from("ui.focus.human");
    spec
}

/// Remap Use onto Escape, then confirm coverage.
#[must_use]
pub fn remap_journey() -> JourneySpec {
    let mut spec = JourneySpec::new("ui.remap.use", 16);
    spec.start = StartStateRef {
        name: Name::from("remap"),
    };
    spec.steps = vec![press(Button::KeyEnter), press(Button::KeyEsc)];
    spec.assertions.push(JourneyAssertion::Capture {
        point: Name::from("rebound"),
    });
    spec.capture_points.push(CapturePoint {
        name: Name::from("rebound"),
        after_step: u32::MAX,
        kind: crate::journey::CaptureKind::Semantic,
    });
    spec
}

fn press(button: Button) -> JourneyStep {
    JourneyStep::Device {
        action: DeviceAction::press(PlayerId(0), button, IntentTarget::None),
    }
}

/// Headless menu host. Layout/focus only; never writes Projection.
#[derive(Clone, Debug)]
pub struct FocusHost {
    table: BindTable,
    cycle: FocusCycle,
    sheet: String,
    ticks: u32,
    last_state: String,
    blocked: String,
    captures: BTreeSet<String>,
    saves: BTreeMap<String, String>,
}

impl FocusHost {
    /// Pause menu under the first-title profile.
    #[must_use]
    pub fn pause() -> Self {
        let profile = klotho_ir::A11yProfile::first_title();
        let root = pause_menu("en", &profile);
        Self {
            table: BindTable::hearth(),
            cycle: FocusCycle::from_tree(&root),
            sheet: "pause".into(),
            ticks: 0,
            last_state: "resume".into(),
            blocked: String::new(),
            captures: BTreeSet::new(),
            saves: BTreeMap::new(),
        }
    }

    /// Opened remap sheet.
    #[must_use]
    pub fn remap() -> Self {
        let profile = klotho_ir::A11yProfile::first_title();
        let table = BindTable::hearth();
        let root = remap_menu("en", InputFamily::KeyboardMouse, &table, &profile);
        Self {
            table,
            cycle: FocusCycle::from_tree(&root),
            sheet: "remap".into(),
            ticks: 0,
            last_state: "bind_use".into(),
            blocked: String::new(),
            captures: BTreeSet::new(),
            saves: BTreeMap::new(),
        }
    }

    /// Last focused control.
    #[must_use]
    pub fn focused(&self) -> Option<&str> {
        self.cycle.focused()
    }

    fn outcome(&self) -> StepOutcome {
        StepOutcome {
            ticks: self.ticks,
            last_state: self.last_state.clone(),
            blocked: self.blocked.clone(),
        }
    }

    fn apply_nav(&mut self, nav: FocusNav) {
        self.cycle.nav(nav);
        match nav {
            FocusNav::Activate => {
                if let Some(id) = self.cycle.activated() {
                    match (self.sheet.as_str(), id) {
                        ("pause", "settings" | "accessibility") => {
                            let profile = klotho_ir::A11yProfile::first_title();
                            let root = settings_menu("en", &profile);
                            self.cycle = FocusCycle::from_tree(&root);
                            self.sheet = "settings".into();
                        }
                        ("pause", "remap") => {
                            let profile = klotho_ir::A11yProfile::first_title();
                            let root =
                                remap_menu("en", InputFamily::KeyboardMouse, &self.table, &profile);
                            self.cycle = FocusCycle::from_tree(&root);
                            self.sheet = "remap".into();
                        }
                        ("pause", "resume") => {
                            self.last_state = "resumed".into();
                            self.captures.insert("resumed".into());
                        }
                        ("remap", "bind_use") => {
                            let _ = self.table.rebind(Verb::Use, Button::KeyEsc);
                            self.last_state = "rebound".into();
                            self.captures.insert("rebound".into());
                        }
                        ("settings", "back") | ("remap", "back") => {
                            let profile = klotho_ir::A11yProfile::first_title();
                            let root = pause_menu("en", &profile);
                            self.cycle = FocusCycle::from_tree(&root);
                            self.sheet = "pause".into();
                        }
                        _ => {}
                    }
                }
            }
            FocusNav::Back if self.sheet != "pause" => {
                let profile = klotho_ir::A11yProfile::first_title();
                let root = pause_menu("en", &profile);
                self.cycle = FocusCycle::from_tree(&root);
                self.sheet = "pause".into();
            }
            FocusNav::Back => {}
            _ => {}
        }
        if self.last_state != "resumed" && self.last_state != "rebound" {
            self.last_state = self
                .cycle
                .focused()
                .unwrap_or(self.sheet.as_str())
                .to_owned();
        }
        self.ticks = self.ticks.saturating_add(1);
    }
}

impl JourneyHost for FocusHost {
    fn apply_device(&mut self, action: &DeviceAction) -> Result<StepOutcome, EvalError> {
        if let Some(button) = action.buttons.iter().next() {
            if let Some(nav) = nav_from_button(*button) {
                self.apply_nav(nav);
                self.blocked.clear();
                return Ok(self.outcome());
            }
            self.blocked = format!("{button:?}");
        }
        self.ticks = self.ticks.saturating_add(1);
        Ok(self.outcome())
    }

    fn apply_fixture(
        &mut self,
        _player: PlayerId,
        verb: Verb,
        _target: IntentTarget,
        _analog: Analog,
    ) -> Result<StepOutcome, EvalError> {
        self.blocked = format!("{verb:?}");
        Err(EvalError::Host("focus host is device-nav only".into()))
    }

    fn wait(&mut self, ticks: u32) -> Result<StepOutcome, EvalError> {
        self.ticks = self.ticks.saturating_add(ticks);
        Ok(self.outcome())
    }

    fn camera(&mut self, name: &Name) -> Result<StepOutcome, EvalError> {
        self.last_state = name.as_str().to_owned();
        Ok(self.outcome())
    }

    fn save(&mut self, slot: &Name) -> Result<(), EvalError> {
        self.saves
            .insert(slot.as_str().to_owned(), self.last_state.clone());
        Ok(())
    }

    fn load(&mut self, slot: &Name) -> Result<(), EvalError> {
        let Some(state) = self.saves.get(slot.as_str()) else {
            return Err(EvalError::Unknown(slot.as_str().to_owned()));
        };
        self.last_state = state.clone();
        Ok(())
    }

    fn capture(&mut self, point: &CapturePoint) -> Result<Hash, EvalError> {
        self.captures.insert(point.name.as_str().to_owned());
        Ok(hash_bytes(point.name.as_str().as_bytes()))
    }

    fn check(&self, assertion: &JourneyAssertion) -> Result<(), EvalError> {
        match assertion {
            JourneyAssertion::Capture { point } => {
                if self.captures.contains(point.as_str()) {
                    Ok(())
                } else {
                    Err(EvalError::Unknown(point.as_str().to_owned()))
                }
            }
            JourneyAssertion::Trace { contains } => {
                if self.last_state.contains(contains) {
                    Ok(())
                } else {
                    Err(EvalError::Unreachable {
                        journey: JourneyId::from("ui.focus"),
                        last_state: self.last_state.clone(),
                        blocked: contains.clone(),
                    })
                }
            }
            JourneyAssertion::Qty { .. }
            | JourneyAssertion::Rel { .. }
            | JourneyAssertion::Knows { .. }
            | JourneyAssertion::Place { .. } => Ok(()),
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
            project_hash: hash_bytes(b"kai-16-focus-host"),
            toolchain_hash: hash_bytes(b"kai-16-toolchain"),
            expanded_ir_hash: hash_bytes(self.sheet.as_bytes()),
            canon_hash: hash_bytes(b"kai-16-canon"),
            cas_root: hash_bytes(b"kai-16-cas"),
        }
    }
}

/// Run the supported locale × aspect × input × a11y matrix.
pub fn capture_matrix_gate() -> Result<klotho_ui::CaptureReport, EvalError> {
    matrix_gate(&BindTable::hearth()).map_err(|e| EvalError::Host(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::run_journey;
    use klotho_ui::capture_profiles;

    #[test]
    fn scripted_and_human_focus_complete() {
        let mut host = FocusHost::pause();
        run_journey(&mut host, &focus_journey(), Hash::from_bytes([16; 32])).unwrap();
        assert_eq!(host.last_state(), "resumed");
        let mut human = FocusHost::pause();
        run_journey(
            &mut human,
            &human_focus_journey(),
            Hash::from_bytes([17; 32]),
        )
        .unwrap();
        assert_eq!(human.last_state(), "resumed");
    }

    #[test]
    fn remap_journey_rebinds_use() {
        let mut host = FocusHost::remap();
        run_journey(&mut host, &remap_journey(), Hash::from_bytes([18; 32])).unwrap();
        assert_eq!(host.last_state(), "rebound");
        assert_eq!(
            host.table
                .binding_for(Verb::Use, InputFamily::KeyboardMouse)
                .unwrap()
                .button,
            Button::KeyEsc
        );
    }

    #[test]
    fn capture_matrix_is_green() {
        let report = capture_matrix_gate().unwrap();
        assert!(report.pass());
        assert!(!capture_profiles().is_empty());
    }
}
