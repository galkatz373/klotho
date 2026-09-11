//! Journey DSL. Steps are public inputs; assertions query semantic facts.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use klotho_core::PlayerId;
use klotho_input::Button;
use klotho_ir::{Analog, AnchorId, Cmp, IntentTarget, Name, Rel, Verb};

use crate::ids::JourneyId;

/// Named start fixture. Hosts resolve it to a seed.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartStateRef {
    /// Fixture name (`door-closed`).
    pub name: Name,
}

/// Capture class. Pixel/audio lanes land later; the marker is first-class now.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureKind {
    /// Snapshot / semantic hashes.
    Semantic,
    /// Presentation capture reserved for later PRs.
    Presentation,
}

/// Named capture after a step index.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturePoint {
    /// Capture name.
    pub name: Name,
    /// 0-based step index after which to capture. `u32::MAX` means end.
    pub after_step: u32,
    /// Capture class.
    pub kind: CaptureKind,
}

/// Public device action. No [`klotho_ir::Agency`] field.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceAction {
    /// Local player slot.
    pub player: PlayerId,
    /// Buttons held.
    pub buttons: BTreeSet<Button>,
    /// Stick X.
    pub stick_x: i16,
    /// Stick Z.
    pub stick_z: i16,
    /// Look yaw delta, millidegrees.
    pub look_yaw: i32,
    /// Look pitch delta, millidegrees.
    pub look_pitch: i32,
    /// WAIT phase, per-mille.
    pub phase: u16,
    /// Intent target.
    pub target: IntentTarget,
}

impl DeviceAction {
    /// Empty sample for `player`.
    #[must_use]
    pub fn new(player: PlayerId) -> Self {
        Self {
            player,
            buttons: BTreeSet::new(),
            stick_x: 0,
            stick_z: 0,
            look_yaw: 0,
            look_pitch: 0,
            phase: 0,
            target: IntentTarget::None,
        }
    }

    /// Hold `button` targeting `target`.
    #[must_use]
    pub fn press(player: PlayerId, button: Button, target: IntentTarget) -> Self {
        let mut a = Self::new(player);
        a.buttons.insert(button);
        a.target = target;
        a
    }
}

/// One journey step. There is no Projection-write variant.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum JourneyStep {
    /// Device sample through the public input path.
    Device {
        /// Sample.
        action: DeviceAction,
    },
    /// Pre-authored verb fixture. Agency is stamped by the trusted adapter.
    Fixture {
        /// Local player.
        player: PlayerId,
        /// Verb.
        verb: Verb,
        /// Target.
        target: IntentTarget,
        /// Analog extras.
        analog: Analog,
    },
    /// Advance time with no new player packet.
    Wait {
        /// Ticks to step.
        ticks: u32,
    },
    /// Presentation-only camera move.
    Camera {
        /// Camera rig or shot name.
        name: Name,
    },
    /// Save under a slot name.
    Save {
        /// Slot.
        slot: Name,
    },
    /// Load a slot.
    Load {
        /// Slot.
        slot: Name,
    },
}

/// Assertion over public semantic facts. Not Projection columns.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum JourneyAssertion {
    /// Admitted Trace body contains this escaped token.
    Trace {
        /// Substring of a Trace body display.
        contains: String,
    },
    /// Quantity comparison.
    Qty {
        /// Locus name.
        locus: Name,
        /// Resource name.
        resource: Name,
        /// Comparison.
        cmp: Cmp,
        /// Right-hand value.
        value: i32,
    },
    /// Relation triple.
    Rel {
        /// Subject.
        a: Name,
        /// Relation.
        rel: Rel,
        /// Object.
        b: Name,
        /// Whether the edge must be present.
        present: bool,
    },
    /// Knows fact.
    Knows {
        /// Mind locus.
        mind: Name,
        /// Fact name.
        fact: Name,
        /// Whether the mind must know it.
        present: bool,
    },
    /// Place residency (`Rel::In`).
    Place {
        /// Locus.
        locus: Name,
        /// Place.
        place: Name,
    },
    /// A capture point was recorded.
    Capture {
        /// Capture name.
        point: Name,
    },
}

/// Authorable journey.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JourneySpec {
    /// Identity.
    pub id: JourneyId,
    /// Start fixture.
    pub start: StartStateRef,
    /// Ordered steps.
    pub steps: Vec<JourneyStep>,
    /// Assertions checked after steps (and at capture points).
    pub assertions: Vec<JourneyAssertion>,
    /// Capture markers.
    pub capture_points: Vec<CapturePoint>,
    /// Hard tick cap.
    pub max_ticks: u32,
    /// Journeys that must run before this one. Dependents close over selection.
    pub depends_on: Vec<JourneyId>,
    /// Anchors this journey covers.
    pub anchors: Vec<AnchorId>,
    /// Modules this journey covers.
    pub modules: Vec<AnchorId>,
}

impl JourneySpec {
    /// Empty journey with `id` and `max_ticks`.
    #[must_use]
    pub fn new(id: impl Into<JourneyId>, max_ticks: u32) -> Self {
        Self {
            id: id.into(),
            start: StartStateRef {
                name: Name::from("default"),
            },
            steps: Vec::new(),
            assertions: Vec::new(),
            capture_points: Vec::new(),
            max_ticks,
            depends_on: Vec::new(),
            anchors: Vec::new(),
            modules: Vec::new(),
        }
    }
}
