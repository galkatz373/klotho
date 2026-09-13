//! Full remapping and glyph names (KAI-16). Gameplay verbs stay on the table.

use crate::{BindTable, Button};
use klotho_ir::Verb;

/// Device family a bind or glyph belongs to.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum InputFamily {
    /// Keyboard plus mouse.
    KeyboardMouse,
    /// Gamepad face / D-pad / start.
    Gamepad,
}

impl InputFamily {
    /// True when `button` belongs to this family.
    #[must_use]
    pub const fn contains(self, button: Button) -> bool {
        matches!(
            (self, button),
            (
                Self::KeyboardMouse,
                Button::KeyE
                    | Button::KeyF
                    | Button::KeyG
                    | Button::KeyT
                    | Button::KeyP
                    | Button::KeyR
                    | Button::KeySpace
                    | Button::KeyEnter
                    | Button::KeyUp
                    | Button::KeyDown
                    | Button::KeyLeft
                    | Button::KeyRight
                    | Button::KeyEsc
                    | Button::MouseLeft
            ) | (
                Self::Gamepad,
                Button::PadSouth
                    | Button::PadWest
                    | Button::PadEast
                    | Button::PadStart
                    | Button::PadUp
                    | Button::PadDown
                    | Button::PadLeft
                    | Button::PadRight
                    | Button::PadNorth
            )
        )
    }

    /// Verbs the first-title bind table must cover for this family.
    #[must_use]
    pub const fn required_verbs(self) -> &'static [Verb] {
        match self {
            Self::KeyboardMouse => &[
                Verb::Use,
                Verb::Carry,
                Verb::Drop,
                Verb::Talk,
                Verb::Time,
                Verb::Pay,
                Verb::Fire,
            ],
            Self::Gamepad => &[Verb::Use, Verb::Carry, Verb::Drop, Verb::Talk],
        }
    }

    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeyboardMouse => "keyboard_mouse",
            Self::Gamepad => "gamepad",
        }
    }
}

/// Why a remap was refused.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum RemapError {
    /// `verb` is not in the table.
    Missing {
        /// Missing verb.
        verb: Verb,
    },
    /// `button` is bound to a different verb that cannot swap (not in table).
    Occupied {
        /// Occupied control.
        button: Button,
        /// Verb already on it.
        verb: Verb,
    },
}

impl core::fmt::Display for RemapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Missing { verb } => write!(f, "Missing({verb:?})"),
            Self::Occupied { button, verb } => write!(f, "Occupied({button:?}:{verb:?})"),
        }
    }
}

impl core::error::Error for RemapError {}

/// Controller / keyboard glyph for prompts. Color is never the only cue.
#[must_use]
pub const fn glyph(button: Button) -> &'static str {
    match button {
        Button::KeyE => "E",
        Button::KeyF => "F",
        Button::KeyG => "G",
        Button::KeyT => "T",
        Button::KeyP => "P",
        Button::KeyR => "R",
        Button::KeySpace => "Space",
        Button::KeyEnter => "Enter",
        Button::MouseLeft => "Mouse1",
        Button::PadSouth => "A",
        Button::PadWest => "X",
        Button::PadEast => "B",
        Button::PadStart => "Start",
        Button::KeyUp => "Up",
        Button::KeyDown => "Down",
        Button::KeyLeft => "Left",
        Button::KeyRight => "Right",
        Button::KeyEsc => "Esc",
        Button::PadUp => "DPadUp",
        Button::PadDown => "DPadDown",
        Button::PadLeft => "DPadLeft",
        Button::PadRight => "DPadRight",
        Button::PadNorth => "Y",
    }
}

pub(crate) fn rebind(table: &mut BindTable, verb: Verb, button: Button) -> Result<(), RemapError> {
    let Some(src) = table
        .binds_mut()
        .iter()
        .position(|b| b.verb == verb && family_of(b.button) == family_of(button))
    else {
        return Err(RemapError::Missing { verb });
    };
    if table.binds_mut()[src].button == button {
        return Ok(());
    }
    if let Some(dst) = table.binds_mut().iter().position(|b| b.button == button) {
        let a = table.binds_mut()[src].button;
        table.binds_mut()[dst].button = a;
        table.binds_mut()[src].button = button;
        return Ok(());
    }
    table.binds_mut()[src].button = button;
    Ok(())
}

pub(crate) fn swap(table: &mut BindTable, a: Button, b: Button) -> Result<(), RemapError> {
    let ia = table
        .binds_mut()
        .iter()
        .position(|bind| bind.button == a)
        .ok_or(RemapError::Occupied {
            button: a,
            verb: Verb::Look,
        })?;
    let ib = table
        .binds_mut()
        .iter()
        .position(|bind| bind.button == b)
        .ok_or(RemapError::Occupied {
            button: b,
            verb: Verb::Look,
        })?;
    let ba = table.binds_mut()[ia].button;
    table.binds_mut()[ia].button = table.binds_mut()[ib].button;
    table.binds_mut()[ib].button = ba;
    Ok(())
}

fn family_of(button: Button) -> InputFamily {
    if InputFamily::Gamepad.contains(button) {
        InputFamily::Gamepad
    } else {
        InputFamily::KeyboardMouse
    }
}

/// True when every required verb for `family` is bound.
#[must_use]
pub fn coverage_complete(table: &BindTable, family: InputFamily) -> bool {
    family
        .required_verbs()
        .iter()
        .all(|v| table.covers(*v, family))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BindTable;

    #[test]
    fn hearth_covers_required_verbs() {
        let t = BindTable::hearth();
        assert!(coverage_complete(&t, InputFamily::KeyboardMouse));
        assert!(coverage_complete(&t, InputFamily::Gamepad));
        assert_eq!(glyph(Button::PadSouth), "A");
        assert_eq!(glyph(Button::KeyE), "E");
    }

    #[test]
    fn rebind_swaps_occupied_use_and_carry() {
        let mut t = BindTable::hearth();
        t.rebind(Verb::Use, Button::KeyF).unwrap();
        assert_eq!(
            t.binding_for(Verb::Use, InputFamily::KeyboardMouse)
                .unwrap()
                .button,
            Button::KeyF
        );
        assert_eq!(
            t.binding_for(Verb::Carry, InputFamily::KeyboardMouse)
                .unwrap()
                .button,
            Button::KeyE
        );
        assert!(coverage_complete(&t, InputFamily::KeyboardMouse));
    }

    #[test]
    fn rebind_to_free_menu_key_keeps_coverage() {
        let mut t = BindTable::hearth();
        t.rebind(Verb::Use, Button::KeyEsc).unwrap();
        assert_eq!(
            t.binding_for(Verb::Use, InputFamily::KeyboardMouse)
                .unwrap()
                .button,
            Button::KeyEsc
        );
        assert!(coverage_complete(&t, InputFamily::KeyboardMouse));
    }

    #[test]
    fn missing_verb_fails() {
        let mut t = BindTable::hearth();
        let err = t.rebind(Verb::Reload, Button::KeyE).unwrap_err();
        assert!(matches!(err, RemapError::Missing { verb: Verb::Reload }));
    }

    #[test]
    fn gamepad_rebind_does_not_touch_keyboard() {
        let mut t = BindTable::hearth();
        t.rebind(Verb::Use, Button::PadWest).unwrap();
        assert_eq!(
            t.binding_for(Verb::Use, InputFamily::Gamepad)
                .unwrap()
                .button,
            Button::PadWest
        );
        assert_eq!(
            t.binding_for(Verb::Use, InputFamily::KeyboardMouse)
                .unwrap()
                .button,
            Button::KeyE
        );
    }
}
