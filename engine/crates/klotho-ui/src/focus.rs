//! Menu focus semantics. Presentation-only; pause menus do not enqueue PlayerIntent.

use klotho_input::Button;
use klotho_manifest::{FocusRole, ScreenReaderNode};

use crate::error::UiError;
use crate::layout::{LaidOut, LayoutFrame, UiKind, UiNode};

/// One focus navigation.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum FocusNav {
    /// Next in authored order.
    Next,
    /// Previous in authored order.
    Prev,
    /// Activate / confirm.
    Activate,
    /// Leave the current sheet.
    Back,
    /// Same as [`Self::Prev`] for vertical menus; slider down.
    Up,
    /// Same as [`Self::Next`] for vertical menus; slider up.
    Down,
    /// Slider down / group left.
    Left,
    /// Slider up / group right.
    Right,
}

/// Map a device button onto menu navigation. Unrelated gameplay buttons are ignored.
#[must_use]
pub fn nav_from_button(button: Button) -> Option<FocusNav> {
    match button {
        Button::KeyDown | Button::PadDown => Some(FocusNav::Next),
        Button::KeyUp | Button::PadUp => Some(FocusNav::Prev),
        Button::KeyLeft | Button::PadLeft => Some(FocusNav::Left),
        Button::KeyRight | Button::PadRight => Some(FocusNav::Right),
        Button::KeyEnter | Button::PadSouth | Button::KeyE => Some(FocusNav::Activate),
        Button::KeyEsc | Button::PadEast | Button::PadNorth | Button::KeyG => Some(FocusNav::Back),
        _ => None,
    }
}

/// Focus cycle over a laid-out tree.
#[derive(Clone, Debug)]
pub struct FocusCycle {
    items: Vec<String>,
    cursor: usize,
    activated: Option<String>,
    back: bool,
}

impl FocusCycle {
    /// Build from a layout, skipping disabled nodes. First enabled focusable wins.
    #[must_use]
    pub fn from_layout(frame: &LayoutFrame) -> Self {
        let items: Vec<String> = frame
            .nodes
            .iter()
            .filter(|n| n.focusable && n.enabled)
            .map(|n| n.id.clone())
            .collect();
        Self {
            items,
            cursor: 0,
            activated: None,
            back: false,
        }
    }

    /// Build from an authored tree so order matches declaration, not place order.
    #[must_use]
    pub fn from_tree(root: &UiNode) -> Self {
        let mut items = Vec::new();
        collect_focus(root, &mut items);
        Self {
            items,
            cursor: 0,
            activated: None,
            back: false,
        }
    }

    /// Focusable ids in cycle order.
    #[must_use]
    pub fn items(&self) -> &[String] {
        &self.items
    }

    /// Current focus id.
    #[must_use]
    pub fn focused(&self) -> Option<&str> {
        self.items.get(self.cursor).map(String::as_str)
    }

    /// Last activated id, if any.
    #[must_use]
    pub fn activated(&self) -> Option<&str> {
        self.activated.as_deref()
    }

    /// True after a Back nav.
    #[must_use]
    pub fn back(&self) -> bool {
        self.back
    }

    /// Apply one navigation. Wrap at the ends. Skip empty cycles.
    pub fn nav(&mut self, nav: FocusNav) {
        if self.items.is_empty() {
            if matches!(nav, FocusNav::Back) {
                self.back = true;
            }
            return;
        }
        self.back = false;
        match nav {
            FocusNav::Next | FocusNav::Down | FocusNav::Right => {
                self.cursor = (self.cursor + 1) % self.items.len();
            }
            FocusNav::Prev | FocusNav::Up | FocusNav::Left => {
                self.cursor = if self.cursor == 0 {
                    self.items.len() - 1
                } else {
                    self.cursor - 1
                };
            }
            FocusNav::Activate => {
                self.activated = self.items.get(self.cursor).cloned();
            }
            FocusNav::Back => {
                self.back = true;
                self.activated = None;
            }
        }
    }

    /// Visit every item once by walking Next, then return to the start.
    pub fn complete_cycle(&mut self) -> Result<(), UiError> {
        if self.items.is_empty() {
            return Err(UiError::IncompleteFocus {
                missing: "cycle".into(),
            });
        }
        let start = self.cursor;
        let mut seen = 0usize;
        loop {
            seen += 1;
            self.nav(FocusNav::Next);
            if self.cursor == start {
                break;
            }
            if seen > self.items.len() + 1 {
                return Err(UiError::IncompleteFocus {
                    missing: self.items[start].clone(),
                });
            }
        }
        if seen != self.items.len() {
            return Err(UiError::IncompleteFocus {
                missing: "wrap".into(),
            });
        }
        Ok(())
    }
}

fn collect_focus(node: &UiNode, out: &mut Vec<String>) {
    if node.focusable && node.enabled {
        out.push(node.id.clone());
    }
    for child in &node.children {
        collect_focus(child, out);
    }
}

/// Screen-reader tree in focus order. Names are copied, never invented.
#[must_use]
pub fn screen_reader_tree(frame: &LayoutFrame, focused: Option<&str>) -> Vec<ScreenReaderNode> {
    frame
        .nodes
        .iter()
        .filter(|n| n.focusable || n.kind == UiKind::Caption || n.role == FocusRole::Menu)
        .map(|n| reader_node(n, focused))
        .collect()
}

fn reader_node(n: &LaidOut, focused: Option<&str>) -> ScreenReaderNode {
    ScreenReaderNode {
        id: n.id.clone(),
        role: n.role,
        name: n.name.clone(),
        value: n.value.clone(),
        hint: n.hint.clone(),
        focused: focused == Some(n.id.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{aspect_viewport, layout};
    use crate::menu::pause_menu;
    use klotho_ir::A11yProfile;

    #[test]
    fn next_wraps_and_skips_disabled() {
        let mut root = pause_menu("en", &A11yProfile::first_title());
        if let Some(child) = root.children.iter_mut().find(|c| c.id == "quit") {
            child.enabled = false;
        }
        let mut cycle = FocusCycle::from_tree(&root);
        assert_eq!(cycle.focused(), Some("resume"));
        cycle.nav(FocusNav::Next);
        assert_eq!(cycle.focused(), Some("settings"));
        cycle.complete_cycle().unwrap();
        assert!(!cycle.items().iter().any(|id| id == "quit"));
    }

    #[test]
    fn activate_and_back() {
        let root = pause_menu("en", &A11yProfile::first_title());
        let mut cycle = FocusCycle::from_tree(&root);
        cycle.nav(FocusNav::Activate);
        assert_eq!(cycle.activated(), Some("resume"));
        cycle.nav(FocusNav::Back);
        assert!(cycle.back());
        assert!(cycle.activated().is_none());
    }

    #[test]
    fn buttons_map_to_nav() {
        assert_eq!(nav_from_button(Button::KeyDown), Some(FocusNav::Next));
        assert_eq!(nav_from_button(Button::PadSouth), Some(FocusNav::Activate));
        assert_eq!(nav_from_button(Button::KeyEsc), Some(FocusNav::Back));
        assert!(nav_from_button(Button::KeyR).is_none());
    }

    #[test]
    fn reader_names_every_focusable() {
        let profile = A11yProfile::max_access();
        let root = pause_menu("en", &profile);
        let frame = layout(&root, aspect_viewport(1_920, 1_080), &profile, "en");
        let cycle = FocusCycle::from_tree(&root);
        let tree = screen_reader_tree(&frame, cycle.focused());
        for id in cycle.items() {
            let node = tree.iter().find(|n| n.id == *id).expect(id);
            assert!(!node.name.is_empty(), "{id}");
        }
        assert!(tree.iter().any(|n| n.focused && n.id == "resume"));
    }
}
