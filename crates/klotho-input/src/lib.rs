//! Device sample → [`PlayerIntent`]. No player-controller class (HLD §9).
//!
//! Injected devices are the v1 test surface. OS backends land with
//! `klotho-platform`. Runtime wraps the result as `Proposal::Player`.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeSet;

use klotho_core::{PlayerId, Tick, YawMd};
use klotho_ir::{Agency, Analog, Channel, IntentTarget, PlayerIntent, Verb};

/// One digital control a bind table can name.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Button {
    /// Keyboard E — Hearth Use.
    KeyE,
    /// Keyboard F — Carry.
    KeyF,
    /// Keyboard G — Drop.
    KeyG,
    /// Keyboard T — Talk.
    KeyT,
    /// Keyboard P — Pay.
    KeyP,
    /// Keyboard R — Ash Fire.
    KeyR,
    /// Keyboard Space — Time / Timing.
    KeySpace,
    /// Keyboard Enter — Talk + DialogueChoice (accept).
    KeyEnter,
    /// Mouse left — Use.
    MouseLeft,
    /// Gamepad south face (A/X) — Use.
    PadSouth,
    /// Gamepad west face (X/Y) — Carry.
    PadWest,
    /// Gamepad east face (B/Circle) — Drop.
    PadEast,
    /// Gamepad start — Talk + DialogueChoice.
    PadStart,
}

/// One bind: button → verb and optional agency channel.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Binding {
    /// Digital control.
    pub button: Button,
    /// Verb emitted while the button is down.
    pub verb: Verb,
    /// Channel claimed on that packet (`Timing`, `DialogueChoice`, …).
    pub channel: Option<Channel>,
}

/// Keyboard / mouse / gamepad bind table. Canon-authored in v2; Hearth defaults here.
#[derive(Clone, Debug, Default)]
pub struct BindTable {
    binds: Vec<Binding>,
}

impl BindTable {
    /// Hearth + Ash v1 defaults. No player-controller class.
    #[must_use]
    pub fn hearth() -> Self {
        Self {
            binds: vec![
                Binding {
                    button: Button::KeySpace,
                    verb: Verb::Time,
                    channel: Some(Channel::Timing),
                },
                Binding {
                    button: Button::KeyEnter,
                    verb: Verb::Talk,
                    channel: Some(Channel::DialogueChoice),
                },
                Binding {
                    button: Button::PadStart,
                    verb: Verb::Talk,
                    channel: Some(Channel::DialogueChoice),
                },
                Binding {
                    button: Button::KeyT,
                    verb: Verb::Talk,
                    channel: None,
                },
                Binding {
                    button: Button::KeyE,
                    verb: Verb::Use,
                    channel: None,
                },
                Binding {
                    button: Button::MouseLeft,
                    verb: Verb::Use,
                    channel: None,
                },
                Binding {
                    button: Button::PadSouth,
                    verb: Verb::Use,
                    channel: None,
                },
                Binding {
                    button: Button::KeyF,
                    verb: Verb::Carry,
                    channel: None,
                },
                Binding {
                    button: Button::PadWest,
                    verb: Verb::Carry,
                    channel: None,
                },
                Binding {
                    button: Button::KeyG,
                    verb: Verb::Drop,
                    channel: None,
                },
                Binding {
                    button: Button::PadEast,
                    verb: Verb::Drop,
                    channel: None,
                },
                Binding {
                    button: Button::KeyP,
                    verb: Verb::Pay,
                    channel: None,
                },
                Binding {
                    button: Button::KeyR,
                    verb: Verb::Fire,
                    channel: None,
                },
            ],
        }
    }

    /// Bindings in table order.
    #[must_use]
    pub fn bindings(&self) -> &[Binding] {
        &self.binds
    }
}

/// One injected (or OS-sampled) device snapshot for a local player.
#[derive(Clone, Debug)]
pub struct DeviceSample {
    /// Tick the sample was taken. Copied onto [`PlayerIntent::at`].
    pub tick: Tick,
    /// Local player slot.
    pub player: PlayerId,
    /// Buttons held this tick.
    pub buttons: BTreeSet<Button>,
    /// Stick X. Copied onto analog; not committed vel.
    pub stick_x: i16,
    /// Stick Z (forward).
    pub stick_z: i16,
    /// Look yaw delta, millidegrees.
    pub look_yaw: YawMd,
    /// Look pitch delta, millidegrees.
    pub look_pitch: i32,
    /// WAIT-window phase, per-mille.
    pub phase: u16,
    /// Intent target (reticle / interact).
    pub target: IntentTarget,
}

impl DeviceSample {
    /// Empty sample at `tick` for `player`.
    #[must_use]
    pub fn new(player: PlayerId, tick: Tick) -> Self {
        Self {
            tick,
            player,
            buttons: BTreeSet::new(),
            stick_x: 0,
            stick_z: 0,
            look_yaw: YawMd::ZERO,
            look_pitch: 0,
            phase: 0,
            target: IntentTarget::None,
        }
    }
}

/// Maps a [`DeviceSample`] through a [`BindTable`] to one [`PlayerIntent`].
///
/// One packet per tick (K18 write-cell): analog (stick/look) rides along with
/// the highest-priority discrete verb, or `Move`/`Look` when none is held.
pub struct InputMapper {
    table: BindTable,
}

impl InputMapper {
    /// Mapper with the given table.
    #[must_use]
    pub fn new(table: BindTable) -> Self {
        Self { table }
    }

    /// Hearth defaults.
    #[must_use]
    pub fn hearth() -> Self {
        Self::new(BindTable::hearth())
    }

    /// Bind table in use.
    #[must_use]
    pub fn table(&self) -> &BindTable {
        &self.table
    }

    /// Map an injected (or OS) sample. Always returns a structurally valid packet.
    #[must_use]
    pub fn map(&self, sample: &DeviceSample) -> PlayerIntent {
        let mut verb = None;
        let mut claimed = Vec::new();
        let mut best = u8::MAX;
        for b in &self.table.binds {
            if !sample.buttons.contains(&b.button) {
                continue;
            }
            let rank = verb_rank(b.verb);
            if rank < best {
                best = rank;
                verb = Some(b.verb);
                claimed.clear();
                if let Some(ch) = b.channel {
                    claimed.push(ch);
                }
            }
        }
        let moving = sample.stick_x != 0 || sample.stick_z != 0;
        let verb = verb.unwrap_or(if moving { Verb::Move } else { Verb::Look });
        PlayerIntent {
            player: sample.player,
            at: sample.tick,
            verb,
            target: sample.target.clone(),
            analog: Analog {
                phase: sample.phase.min(1000),
                stick_x: sample.stick_x,
                stick_z: sample.stick_z,
                look_yaw: sample.look_yaw,
                look_pitch: sample.look_pitch,
            },
            agency: Agency {
                claimed,
                assist: Default::default(),
            },
        }
    }
}

fn verb_rank(v: Verb) -> u8 {
    match v {
        Verb::Time => 0,
        Verb::Talk => 1,
        Verb::Use | Verb::Open => 2,
        Verb::Carry => 3,
        Verb::Drop => 4,
        Verb::Pay => 5,
        Verb::Fire => 6,
        Verb::Investigate => 7,
        Verb::Move | Verb::Look => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{PlayerId, Tick, YawMd};
    use klotho_ir::{Channel, IntentTarget, PlayerIntent, Verb};

    #[test]
    fn key_e_is_use() {
        let mut s = DeviceSample::new(PlayerId(0), Tick(3));
        s.buttons.insert(Button::KeyE);
        s.target = IntentTarget::Name(klotho_ir::Name::from("oak_door"));
        let p = InputMapper::hearth().map(&s);
        assert_eq!(p.verb, Verb::Use);
        assert_eq!(p.at, Tick(3));
        assert!(p.agency.claimed.is_empty());
        p.validate().unwrap();
    }

    #[test]
    fn space_claims_timing() {
        let mut s = DeviceSample::new(PlayerId(0), Tick(0));
        s.buttons.insert(Button::KeySpace);
        s.phase = 400;
        let p = InputMapper::hearth().map(&s);
        assert_eq!(p.verb, Verb::Time);
        assert_eq!(p.agency.claimed, vec![Channel::Timing]);
        assert_eq!(p.analog.phase, 400);
    }

    #[test]
    fn enter_claims_dialogue() {
        let mut s = DeviceSample::new(PlayerId(0), Tick(0));
        s.buttons.insert(Button::KeyEnter);
        let p = InputMapper::hearth().map(&s);
        assert_eq!(p.verb, Verb::Talk);
        assert_eq!(p.agency.claimed, vec![Channel::DialogueChoice]);
    }

    #[test]
    fn stick_is_move_when_no_button() {
        let mut s = DeviceSample::new(PlayerId(0), Tick(0));
        s.stick_z = 32_000;
        let p = InputMapper::hearth().map(&s);
        assert_eq!(p.verb, Verb::Move);
        assert_eq!(p.analog.stick_z, 32_000);
    }

    #[test]
    fn look_delta_is_look() {
        let mut s = DeviceSample::new(PlayerId(0), Tick(0));
        s.look_yaw = YawMd(1500);
        let p = InputMapper::hearth().map(&s);
        assert_eq!(p.verb, Verb::Look);
        assert_eq!(p.analog.look_yaw, YawMd(1500));
    }

    #[test]
    fn timing_outranks_use() {
        let mut s = DeviceSample::new(PlayerId(0), Tick(0));
        s.buttons.insert(Button::KeyE);
        s.buttons.insert(Button::KeySpace);
        let p = InputMapper::hearth().map(&s);
        assert_eq!(p.verb, Verb::Time);
        assert!(p.agency.claims(Channel::Timing));
    }

    #[test]
    fn pad_south_is_use() {
        let mut s = DeviceSample::new(PlayerId(1), Tick(1));
        s.buttons.insert(Button::PadSouth);
        let p = InputMapper::hearth().map(&s);
        assert_eq!(p.player, PlayerId(1));
        assert_eq!(p.verb, Verb::Use);
    }

    #[test]
    fn mapper_cannot_mint_infer() {
        // Type-level: map returns PlayerIntent only. This test exists so a
        // future "convenience" Infer path cannot sneak in without failing CI.
        let p = InputMapper::hearth().map(&DeviceSample::new(PlayerId(0), Tick(0)));
        let _: PlayerIntent = p;
    }
}
