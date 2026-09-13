//! Conversation journeys over compiled dialogue (KAI-15).
//!
//! The host talks through the public input path (`Verb::Talk`). Locale text is
//! presentation; Knows grants are the authoritative branch.

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::{Hash, PlayerId};
use klotho_dialogue::{DialogueModule, NarrativeProject, walk};
use klotho_ir::{Analog, IntentTarget, Name, Verb};
use klotho_prove::hash_bytes;

use crate::error::EvalError;
use crate::evidence::EvidenceContext;
use crate::host::{JourneyHost, StepOutcome};
use crate::ids::JourneyId;
use crate::journey::{
    CapturePoint, DeviceAction, JourneyAssertion, JourneySpec, JourneyStep, StartStateRef,
};

/// Scripted observatory conversation: greet, then accept the offer.
#[must_use]
pub fn conversation_journey(project: &NarrativeProject) -> JourneySpec {
    let mut spec = JourneySpec::new("dialogue.observatory", 32);
    spec.start = StartStateRef {
        name: Name::from("plates_decoded"),
    };
    spec.steps = vec![JourneyStep::Fixture {
        player: PlayerId(0),
        verb: Verb::Talk,
        target: IntentTarget::Name(Name::from("yes")),
        analog: Analog::default(),
    }];
    spec.assertions.push(JourneyAssertion::Knows {
        mind: Name::from("player"),
        fact: Name::from("observatory_open"),
        present: true,
    });
    spec.anchors.push(project.dialogue.anchor);
    spec.modules.push(project.anchor);
    spec
}

/// Headless conversation host. Locale is presentation-only.
#[derive(Clone, Debug)]
pub struct ConversationHost {
    module: DialogueModule,
    knows: Vec<Name>,
    played: Vec<Name>,
    ticks: u32,
    last_state: String,
    blocked: String,
    captures: BTreeSet<String>,
    saves: BTreeMap<String, Vec<Name>>,
    project_hash: Hash,
    lowered_hash: Hash,
}

impl ConversationHost {
    /// Start with the project's dialogue and an initial Knows set.
    pub fn from_project(project: &NarrativeProject, initial: &[Name]) -> Result<Self, EvalError> {
        project
            .validate()
            .map_err(|e| EvalError::Host(e.to_string()))?;
        let lowered = project
            .lower()
            .map_err(|e| EvalError::Host(e.to_string()))?;
        Ok(Self {
            module: project.dialogue.clone(),
            knows: initial.to_vec(),
            played: Vec::new(),
            ticks: 0,
            last_state: project.dialogue.entry.as_str().to_owned(),
            blocked: String::new(),
            captures: BTreeSet::new(),
            saves: BTreeMap::new(),
            project_hash: hash_bytes(project.id.as_str().as_bytes()),
            lowered_hash: lowered.branch_hash(),
        })
    }

    /// Observatory fixture with plates already decoded.
    pub fn observatory() -> Result<Self, EvalError> {
        let p = klotho_dialogue::observatory();
        Self::from_project(&p, &[Name::from("plates_decoded")])
    }

    /// Knows facts in canonical order.
    #[must_use]
    pub fn knows(&self) -> &[Name] {
        &self.knows
    }

    fn outcome(&self) -> StepOutcome {
        StepOutcome {
            ticks: self.ticks,
            last_state: self.last_state.clone(),
            blocked: self.blocked.clone(),
        }
    }

    fn talk(&mut self, choice: &Name) -> Result<StepOutcome, EvalError> {
        match walk(&self.module, &self.knows, std::slice::from_ref(choice)) {
            Ok((played, knows)) => {
                self.played = played;
                self.knows = knows;
                self.last_state = self
                    .played
                    .last()
                    .map(|n| n.as_str().to_owned())
                    .unwrap_or_else(|| self.last_state.clone());
                self.blocked.clear();
                self.ticks = self.ticks.saturating_add(1);
                Ok(self.outcome())
            }
            Err(e) => {
                self.blocked = choice.as_str().to_owned();
                Err(EvalError::Unreachable {
                    journey: JourneyId::from("dialogue.walk"),
                    last_state: self.last_state.clone(),
                    blocked: e.to_string(),
                })
            }
        }
    }
}

impl JourneyHost for ConversationHost {
    fn apply_device(&mut self, action: &DeviceAction) -> Result<StepOutcome, EvalError> {
        match &action.target {
            IntentTarget::Name(choice) => self.talk(choice),
            IntentTarget::None | IntentTarget::Sigil(_) => {
                self.ticks = self.ticks.saturating_add(1);
                Ok(self.outcome())
            }
        }
    }

    fn apply_fixture(
        &mut self,
        _player: PlayerId,
        verb: Verb,
        target: IntentTarget,
        _analog: Analog,
    ) -> Result<StepOutcome, EvalError> {
        if verb != Verb::Talk {
            self.ticks = self.ticks.saturating_add(1);
            return Ok(self.outcome());
        }
        match target {
            IntentTarget::Name(choice) => self.talk(&choice),
            IntentTarget::None | IntentTarget::Sigil(_) => {
                self.ticks = self.ticks.saturating_add(1);
                Ok(self.outcome())
            }
        }
    }

    fn wait(&mut self, ticks: u32) -> Result<StepOutcome, EvalError> {
        self.ticks = self.ticks.saturating_add(ticks);
        Ok(self.outcome())
    }

    fn camera(&mut self, _name: &Name) -> Result<StepOutcome, EvalError> {
        Ok(self.outcome())
    }

    fn save(&mut self, slot: &Name) -> Result<(), EvalError> {
        self.saves
            .insert(slot.as_str().to_owned(), self.knows.clone());
        Ok(())
    }

    fn load(&mut self, slot: &Name) -> Result<(), EvalError> {
        let Some(k) = self.saves.get(slot.as_str()).cloned() else {
            return Err(EvalError::Unknown(slot.as_str().to_owned()));
        };
        self.knows = k;
        Ok(())
    }

    fn capture(&mut self, point: &CapturePoint) -> Result<Hash, EvalError> {
        self.captures.insert(point.name.as_str().to_owned());
        Ok(hash_bytes(point.name.as_str().as_bytes()))
    }

    fn check(&self, assertion: &JourneyAssertion) -> Result<(), EvalError> {
        match assertion {
            JourneyAssertion::Knows { fact, present, .. } => {
                let has = self.knows.iter().any(|k| k == fact);
                if has != *present {
                    return Err(EvalError::Unreachable {
                        journey: JourneyId::from("dialogue.knows"),
                        last_state: self.last_state.clone(),
                        blocked: fact.as_str().to_owned(),
                    });
                }
                Ok(())
            }
            JourneyAssertion::Trace { contains } => {
                if self.played.iter().any(|p| p.as_str().contains(contains)) {
                    Ok(())
                } else {
                    Err(EvalError::Unreachable {
                        journey: JourneyId::from("dialogue.trace"),
                        last_state: self.last_state.clone(),
                        blocked: contains.clone(),
                    })
                }
            }
            JourneyAssertion::Capture { point } => {
                if self.captures.contains(point.as_str()) {
                    Ok(())
                } else {
                    Err(EvalError::Unknown(point.as_str().to_owned()))
                }
            }
            JourneyAssertion::Qty { .. }
            | JourneyAssertion::Rel { .. }
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
            project_hash: self.project_hash,
            toolchain_hash: hash_bytes(b"kai-15-conversation-host"),
            expanded_ir_hash: self.lowered_hash,
            canon_hash: self.lowered_hash,
            cas_root: hash_bytes(b"kai-15-cas"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::run_journey;
    use klotho_dialogue::{SHIPPING_LOCALES, observatory, replay_locales};
    use klotho_ir::Name;

    #[test]
    fn observatory_conversation_journey_grants_knows() {
        let p = observatory();
        let mut host = ConversationHost::from_project(&p, &[Name::from("plates_decoded")]).unwrap();
        run_journey(
            &mut host,
            &conversation_journey(&p),
            Hash::from_bytes([15; 32]),
        )
        .unwrap();
        assert!(
            host.knows()
                .iter()
                .any(|k| k.as_str() == "observatory_open")
        );
    }

    #[test]
    fn ten_locale_replay_is_identical_on_the_host_path() {
        let p = observatory();
        let lowered = p.lower().unwrap();
        let replay = replay_locales(
            &p.dialogue,
            &lowered,
            &p.locales,
            &[Name::from("plates_decoded")],
            &[Name::from("yes")],
        )
        .unwrap();
        assert_eq!(replay.presentation.len(), SHIPPING_LOCALES.len());
    }
}
