//! Deterministic name-addressed journey fixture. Not a runtime World.

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::{Hash, PlayerId};
use klotho_input::Button;
use klotho_ir::{Analog, Cmp, IntentDoc, IntentTarget, Name, Rel, SeedFact, Verb, to_ron};
use klotho_prove::hash_bytes;

use crate::error::EvalError;
use crate::evidence::EvidenceContext;
use crate::host::{JourneyHost, StepOutcome};
use crate::journey::{CapturePoint, DeviceAction, JourneyAssertion};

/// Closed door/key fixture used by eval goldens.
#[derive(Clone, Debug)]
pub struct ScriptHost {
    player: String,
    door: String,
    key: String,
    place: String,
    rels: BTreeSet<(String, u8, String)>,
    qty: BTreeMap<(String, String), i32>,
    knows: BTreeSet<(String, String)>,
    events: Vec<String>,
    saves: BTreeMap<String, ScriptSnap>,
    captures: BTreeSet<String>,
    ticks: u32,
    last_state: String,
    blocked: String,
    project_hash: Hash,
    toolchain_hash: Hash,
    canon_hash: Hash,
    checkpoint: String,
}

#[derive(Clone, Debug)]
struct ScriptSnap {
    rels: BTreeSet<(String, u8, String)>,
    qty: BTreeMap<(String, String), i32>,
    knows: BTreeSet<(String, String)>,
    ticks: u32,
}

impl ScriptHost {
    /// Door locked, key present, player in the place.
    #[must_use]
    pub fn door_key() -> Self {
        let mut h = Self {
            player: "player".into(),
            door: "door".into(),
            key: "key".into(),
            place: "hall".into(),
            rels: BTreeSet::new(),
            qty: BTreeMap::new(),
            knows: BTreeSet::new(),
            events: Vec::new(),
            saves: BTreeMap::new(),
            captures: BTreeSet::new(),
            ticks: 0,
            last_state: "door-locked".into(),
            blocked: String::new(),
            project_hash: hash_bytes(b"script-project"),
            toolchain_hash: hash_bytes(b"script-toolchain"),
            canon_hash: hash_bytes(b"script-canon"),
            checkpoint: String::new(),
        };
        h.add_rel(&h.door.clone(), Rel::LockedBy, &h.door.clone());
        h.add_rel(&h.player.clone(), Rel::In, &h.place.clone());
        h.add_rel(&h.key.clone(), Rel::In, &h.place.clone());
        h.add_rel(&h.door.clone(), Rel::KeyedBy, &h.key.clone());
        h.qty.insert((h.player.clone(), "stamina".into()), 10);
        h
    }

    /// Build the deterministic semantic fixture from an expanded authoring
    /// document. The named player, passage, key, and Place select the bounded
    /// interaction under test; all seed relations and quantities remain exact.
    pub fn from_intent(
        doc: &IntentDoc,
        player: &Name,
        door: &Name,
        key: &Name,
        place: &Name,
    ) -> Result<Self, EvalError> {
        let encoded = to_ron(doc).map_err(|error| EvalError::Host(error.to_string()))?;
        let mut host = Self {
            player: player.0.clone(),
            door: door.0.clone(),
            key: key.0.clone(),
            place: place.0.clone(),
            rels: BTreeSet::new(),
            qty: BTreeMap::new(),
            knows: BTreeSet::new(),
            events: Vec::new(),
            saves: BTreeMap::new(),
            captures: BTreeSet::new(),
            ticks: 0,
            last_state: "door-locked".into(),
            blocked: String::new(),
            project_hash: hash_bytes(encoded.as_bytes()),
            toolchain_hash: hash_bytes(b"intent-script-toolchain-v1"),
            canon_hash: hash_bytes(encoded.as_bytes()),
            checkpoint: String::new(),
        };
        for fact in &doc.seed {
            match fact {
                SeedFact::Rel { a, rel, b } => {
                    host.add_rel(a.as_str(), *rel, b.as_str());
                    if *rel == Rel::Knows {
                        host.knows
                            .insert((a.as_str().to_owned(), b.as_str().to_owned()));
                    }
                }
                SeedFact::Qty { of, res, value } => {
                    host.qty
                        .insert((of.as_str().to_owned(), res.as_str().to_owned()), *value);
                }
                SeedFact::Locus { .. }
                | SeedFact::Pose { .. }
                | SeedFact::Physics { .. }
                | SeedFact::ContactTrack { .. } => {}
            }
        }
        if !host.has_rel(door.as_str(), Rel::LockedBy, door.as_str()) {
            host.last_state = "door-open".into();
        }
        Ok(host)
    }

    /// Name the checkpoint relic used by Use on a save marker.
    #[must_use]
    pub fn with_checkpoint(mut self, checkpoint: &Name) -> Self {
        self.checkpoint = checkpoint.0.clone();
        self
    }

    fn edge(a: &str, rel: Rel, b: &str) -> (String, u8, String) {
        (a.to_owned(), rel.as_u8(), b.to_owned())
    }

    fn add_rel(&mut self, a: &str, rel: Rel, b: &str) {
        self.rels.insert(Self::edge(a, rel, b));
    }

    fn has_rel(&self, a: &str, rel: Rel, b: &str) -> bool {
        self.rels.contains(&Self::edge(a, rel, b))
    }

    fn outcome(&self) -> StepOutcome {
        StepOutcome {
            ticks: self.ticks,
            last_state: self.last_state.clone(),
            blocked: self.blocked.clone(),
        }
    }

    fn target_name(target: &IntentTarget) -> Option<String> {
        match target {
            IntentTarget::Name(n) => Some(n.as_str().to_owned()),
            IntentTarget::None | IntentTarget::Sigil(_) => None,
        }
    }

    fn has_key(&self) -> bool {
        self.has_rel(&self.key, Rel::OwnedBy, &self.player)
    }

    fn key_fits(&self) -> bool {
        self.has_rel(&self.door, Rel::KeyedBy, &self.key)
    }

    fn use_on(&mut self, target: &str) {
        if target == self.key {
            self.add_rel(&self.key.clone(), Rel::OwnedBy, &self.player.clone());
            self.events.push("PickedKey".into());
            self.last_state = "has-key".into();
            self.blocked.clear();
            return;
        }
        if target == self.door {
            if self.has_key() && self.key_fits() {
                self.rels
                    .remove(&Self::edge(&self.door, Rel::LockedBy, &self.door));
                self.events.push("Unlocked".into());
                self.last_state = "door-open".into();
                self.blocked.clear();
            } else {
                self.blocked = "Openable".into();
                self.last_state = "door-locked".into();
            }
            return;
        }
        if !self.checkpoint.is_empty() && target == self.checkpoint {
            self.events.push("Checkpoint".into());
            self.last_state = "checkpoint".into();
            self.blocked.clear();
            let slot = Name::from(self.checkpoint.as_str());
            let _ = self.save(&slot);
        }
    }

    fn apply_verb(&mut self, verb: Verb, target: &IntentTarget) {
        self.ticks = self.ticks.saturating_add(1);
        match verb {
            Verb::Use | Verb::Open => {
                if let Some(t) = Self::target_name(target) {
                    self.use_on(&t);
                }
            }
            Verb::Fire => {
                if let Some(t) = Self::target_name(target) {
                    self.add_rel(&t, Rel::Dead, &t);
                    self.events.push("Hit".into());
                    self.last_state = "foe-down".into();
                    self.blocked.clear();
                }
            }
            Verb::Look | Verb::Move => {
                self.last_state = if self
                    .rels
                    .iter()
                    .any(|(_, rel, _)| *rel == Rel::Dead.as_u8())
                {
                    "foe-down".into()
                } else if self.has_key() {
                    "has-key".into()
                } else if self.has_rel(&self.door, Rel::LockedBy, &self.door) {
                    "door-locked".into()
                } else {
                    "door-open".into()
                };
            }
            _ => {}
        }
    }

    fn cmp(cmp: Cmp, got: i32, want: i32) -> bool {
        match cmp {
            Cmp::Lt => got < want,
            Cmp::Le => got <= want,
            Cmp::Eq => got == want,
            Cmp::Ge => got >= want,
            Cmp::Gt => got > want,
        }
    }

    fn fail(&self, blocked: &str) -> EvalError {
        let blocked = if self.blocked.is_empty() {
            blocked.to_owned()
        } else {
            self.blocked.clone()
        };
        EvalError::Unreachable {
            journey: crate::ids::JourneyId::from("script"),
            last_state: self.last_state.clone(),
            blocked,
        }
    }
}

impl JourneyHost for ScriptHost {
    fn apply_device(&mut self, action: &DeviceAction) -> Result<StepOutcome, EvalError> {
        let verb = if action.buttons.contains(&Button::KeyR) {
            Verb::Fire
        } else if action.buttons.contains(&Button::KeyE)
            || action.buttons.contains(&Button::MouseLeft)
            || action.buttons.contains(&Button::PadSouth)
        {
            Verb::Use
        } else if action.stick_x != 0 || action.stick_z != 0 {
            Verb::Move
        } else {
            Verb::Look
        };
        self.apply_verb(verb, &action.target);
        Ok(self.outcome())
    }

    fn apply_fixture(
        &mut self,
        _player: PlayerId,
        verb: Verb,
        target: IntentTarget,
        _analog: Analog,
    ) -> Result<StepOutcome, EvalError> {
        self.apply_verb(verb, &target);
        Ok(self.outcome())
    }

    fn wait(&mut self, ticks: u32) -> Result<StepOutcome, EvalError> {
        self.ticks = self.ticks.saturating_add(ticks);
        Ok(self.outcome())
    }

    fn camera(&mut self, name: &Name) -> Result<StepOutcome, EvalError> {
        self.events.push(format!("Camera({})", name.as_str()));
        Ok(self.outcome())
    }

    fn save(&mut self, slot: &Name) -> Result<(), EvalError> {
        self.saves.insert(
            slot.as_str().to_owned(),
            ScriptSnap {
                rels: self.rels.clone(),
                qty: self.qty.clone(),
                knows: self.knows.clone(),
                ticks: self.ticks,
            },
        );
        Ok(())
    }

    fn load(&mut self, slot: &Name) -> Result<(), EvalError> {
        let snap = self
            .saves
            .get(slot.as_str())
            .cloned()
            .ok_or_else(|| EvalError::Unknown(slot.as_str().to_owned()))?;
        self.rels = snap.rels;
        self.qty = snap.qty;
        self.knows = snap.knows;
        self.ticks = snap.ticks;
        Ok(())
    }

    fn capture(&mut self, point: &CapturePoint) -> Result<Hash, EvalError> {
        self.captures.insert(point.name.as_str().to_owned());
        Ok(hash_bytes(point.name.as_str().as_bytes()))
    }

    fn check(&self, assertion: &JourneyAssertion) -> Result<(), EvalError> {
        match assertion {
            JourneyAssertion::Trace { contains } => {
                if self.events.iter().any(|e| e.contains(contains)) {
                    Ok(())
                } else {
                    Err(self.fail("Trace"))
                }
            }
            JourneyAssertion::Qty {
                locus,
                resource,
                cmp,
                value,
            } => {
                let got = self
                    .qty
                    .get(&(locus.as_str().to_owned(), resource.as_str().to_owned()))
                    .copied()
                    .unwrap_or(0);
                if Self::cmp(*cmp, got, *value) {
                    Ok(())
                } else {
                    Err(self.fail("Qty"))
                }
            }
            JourneyAssertion::Rel { a, rel, b, present } => {
                let has = self.has_rel(a.as_str(), *rel, b.as_str());
                if has == *present {
                    Ok(())
                } else {
                    Err(self.fail(&format!("{rel:?}")))
                }
            }
            JourneyAssertion::Knows {
                mind,
                fact,
                present,
            } => {
                let has = self
                    .knows
                    .contains(&(mind.as_str().to_owned(), fact.as_str().to_owned()));
                if has == *present {
                    Ok(())
                } else {
                    Err(self.fail("Knows"))
                }
            }
            JourneyAssertion::Place { locus, place } => {
                if self.has_rel(locus.as_str(), Rel::In, place.as_str()) {
                    Ok(())
                } else {
                    Err(self.fail("Place"))
                }
            }
            JourneyAssertion::Capture { point } => {
                if self.captures.contains(point.as_str()) {
                    Ok(())
                } else {
                    Err(self.fail("Capture"))
                }
            }
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
            toolchain_hash: self.toolchain_hash,
            expanded_ir_hash: hash_bytes(b"script-ir"),
            canon_hash: self.canon_hash,
            cas_root: hash_bytes(b"script-cas"),
        }
    }
}
