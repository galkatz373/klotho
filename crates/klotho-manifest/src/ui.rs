//! Attention / HUD buffer. Denied facts have no widget path (enforced at
//! extract in PR 15; this crate only owns the buffer schema).

use klotho_core::{Epoch, IVec3};

/// v1 widget kinds. No LLM layout.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub enum WidgetKind {
    /// Static or templated line.
    Text,
    /// Quantity bar (`stamina`, `heat`).
    Bar {
        /// Current.
        value: i32,
        /// Maximum.
        cap: i32,
    },
    /// Vertical list (inventory, dialogue choices).
    List,
    /// Modal prompt (WAIT window, trade).
    Prompt,
    /// Diegetic world label.
    Label {
        /// Anchor, millimetres.
        world: IVec3,
    },
}

/// One attention widget.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Widget {
    /// Kind.
    pub kind: WidgetKind,
    /// Primary string (title, prompt, or joined list body).
    pub body: String,
}

/// Dumb UI buffer extracted from `WorldSnapshot::view()` (PR 15).
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct UiManifest {
    /// Cook / hull epoch.
    pub epoch: Epoch,
    /// Widgets in draw order.
    pub widgets: Vec<Widget>,
}

impl UiManifest {
    /// Empty HUD.
    #[must_use]
    pub const fn empty(epoch: Epoch) -> Self {
        Self {
            epoch,
            widgets: Vec::new(),
        }
    }

    /// Build from widgets via the crate-private SoA.
    #[must_use]
    pub fn from_widgets(epoch: Epoch, widgets: impl IntoIterator<Item = Widget>) -> Self {
        let mut t = crate::tables::UiTables::new();
        for w in widgets {
            t.push(w);
        }
        t.extract(epoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::UiTables;
    use klotho_core::Epoch;

    #[test]
    fn extract_preserves_draw_order() {
        let mut t = UiTables::new();
        t.push(Widget {
            kind: WidgetKind::Text,
            body: "owed 50 copper".into(),
        });
        t.push(Widget {
            kind: WidgetKind::Bar {
                value: 80,
                cap: 100,
            },
            body: "stamina".into(),
        });
        let ui = t.extract(Epoch::ZERO);
        assert_eq!(ui.widgets.len(), 2);
        assert!(matches!(ui.widgets[0].kind, WidgetKind::Text));
        assert!(matches!(
            ui.widgets[1].kind,
            WidgetKind::Bar {
                value: 80,
                cap: 100
            }
        ));
    }
}
