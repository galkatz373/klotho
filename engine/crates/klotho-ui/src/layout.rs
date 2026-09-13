//! Bounded integer constraint layout. Not a mutable widget world.

use klotho_ir::A11yProfile;
use klotho_manifest::FocusRole;

use crate::error::UiError;
use crate::skin::safe_rect;
use crate::{HudViewport, Rect, SafeArea};

/// How a node stacks its children.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum UiKind {
    /// Vertical stack.
    Column,
    /// Horizontal stack. RTL locales reverse children.
    Row,
    /// Activate control.
    Button,
    /// Binary control.
    Toggle,
    /// Bounded numeric control.
    Slider,
    /// Static text.
    Label,
    /// Subtitle / CC band.
    Caption,
    /// Resource meter. Body is the color-independent label.
    Bar,
    /// Empty flex space.
    Spacer,
}

/// Declarative layout node. Children are owned; there is no live widget graph.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct UiNode {
    /// Stable id (`resume`, `text_scale`).
    pub id: String,
    /// Layout kind.
    pub kind: UiKind,
    /// Resolved body. Empty for spacers.
    pub text: String,
    /// Accessible name. Required when `focusable`.
    pub name: String,
    /// Current value announced to a screen reader.
    pub value: String,
    /// Operation hint.
    pub hint: String,
    /// Focus role.
    pub role: FocusRole,
    /// Participates in the focus cycle.
    pub focusable: bool,
    /// Disabled nodes are skipped.
    pub enabled: bool,
    /// Minimum width before scale, pixels.
    pub min_w: u32,
    /// Minimum height before scale, pixels.
    pub min_h: u32,
    /// Child gap before scale, pixels.
    pub gap: u32,
    /// Padding before scale, pixels.
    pub pad: u32,
    /// Flex grow. Zero is rigid.
    pub flex: u16,
    /// Children in authored order.
    pub children: Vec<UiNode>,
}

impl UiNode {
    /// Column with `id`.
    #[must_use]
    pub fn column(id: impl Into<String>) -> Self {
        Self::leaf(id, UiKind::Column, FocusRole::Group, false)
    }

    /// Row with `id`.
    #[must_use]
    pub fn row(id: impl Into<String>) -> Self {
        Self::leaf(id, UiKind::Row, FocusRole::Group, false)
    }

    /// Focusable button.
    #[must_use]
    pub fn button(id: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut n = Self::leaf(id, UiKind::Button, FocusRole::Button, true);
        n.text = text.clone();
        n.name = text;
        n.hint = "activate".into();
        n.min_h = 36;
        n.min_w = 160;
        n.pad = 8;
        n
    }

    /// Focusable toggle.
    #[must_use]
    pub fn toggle(id: impl Into<String>, text: impl Into<String>, on: bool) -> Self {
        let text = text.into();
        let mut n = Self::leaf(id, UiKind::Toggle, FocusRole::Toggle, true);
        n.text = text.clone();
        n.name = text;
        n.value = if on { "on".into() } else { "off".into() };
        n.hint = "toggle".into();
        n.min_h = 36;
        n.min_w = 220;
        n.pad = 8;
        n
    }

    /// Focusable slider.
    #[must_use]
    pub fn slider(
        id: impl Into<String>,
        text: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        let text = text.into();
        let mut n = Self::leaf(id, UiKind::Slider, FocusRole::Slider, true);
        n.text = text.clone();
        n.name = text;
        n.value = value.into();
        n.hint = "adjust".into();
        n.min_h = 36;
        n.min_w = 280;
        n.pad = 8;
        n
    }

    /// Non-focusable label.
    #[must_use]
    pub fn label(id: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut n = Self::leaf(id, UiKind::Label, FocusRole::Item, false);
        n.text = text.clone();
        n.name = text;
        n.min_h = 28;
        n.min_w = 80;
        n
    }

    fn leaf(id: impl Into<String>, kind: UiKind, role: FocusRole, focusable: bool) -> Self {
        Self {
            id: id.into(),
            kind,
            text: String::new(),
            name: String::new(),
            value: String::new(),
            hint: String::new(),
            role,
            focusable,
            enabled: true,
            min_w: 0,
            min_h: 0,
            gap: 8,
            pad: 0,
            flex: 0,
            children: Vec::new(),
        }
    }

    /// Append a child.
    #[must_use]
    pub fn with(mut self, child: UiNode) -> Self {
        self.children.push(child);
        self
    }
}

/// One placed leaf or container.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LaidOut {
    /// Node id.
    pub id: String,
    /// Screen bounds.
    pub bounds: Rect,
    /// Resolved text.
    pub text: String,
    /// Accessible name.
    pub name: String,
    /// Current value.
    pub value: String,
    /// Operation hint.
    pub hint: String,
    /// Role.
    pub role: FocusRole,
    /// Focusable.
    pub focusable: bool,
    /// Enabled.
    pub enabled: bool,
    /// Kind.
    pub kind: UiKind,
    /// Text did not fit `bounds`.
    pub overflow: bool,
}

/// Placed tree plus overflow/overlap counts.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LayoutFrame {
    /// Viewport used.
    pub viewport: HudViewport,
    /// Safe-area rectangle.
    pub safe: Rect,
    /// Text size in pixels after scale.
    pub text_px: u16,
    /// Whether motion should be suppressed.
    pub reduce_motion: bool,
    /// Placed nodes, parents before children, authored order.
    pub nodes: Vec<LaidOut>,
}

impl LayoutFrame {
    /// Nodes that overflowed.
    #[must_use]
    pub fn overflows(&self) -> Vec<&LaidOut> {
        self.nodes.iter().filter(|n| n.overflow).collect()
    }

    /// Pairwise overlapping focusable/leaf rects.
    #[must_use]
    pub fn overlaps(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let leaves: Vec<_> = self
            .nodes
            .iter()
            .filter(|n| {
                n.kind != UiKind::Column && n.kind != UiKind::Row && n.kind != UiKind::Spacer
            })
            .collect();
        for (i, a) in leaves.iter().enumerate() {
            for b in leaves.iter().skip(i + 1) {
                if intersects(a.bounds, b.bounds) {
                    out.push((a.id.clone(), b.id.clone()));
                }
            }
        }
        out
    }

    /// Fail if any overflow or overlap is present.
    pub fn check(&self, locale: &str) -> Result<(), UiError> {
        if let Some(n) = self.nodes.iter().find(|n| n.overflow) {
            return Err(UiError::Overflow {
                node: n.id.clone(),
                locale: locale.to_owned(),
            });
        }
        if let Some((a, b)) = self.overlaps().into_iter().next() {
            return Err(UiError::Overlap { a, b });
        }
        for n in &self.nodes {
            if n.focusable && n.name.is_empty() {
                return Err(UiError::Compliance {
                    requirement: "screen_reader".into(),
                    witness: format!("node {} has no name", n.id),
                });
            }
        }
        Ok(())
    }
}

/// Place `root` inside `viewport` under `profile` and `locale`.
#[must_use]
pub fn layout(
    root: &UiNode,
    viewport: HudViewport,
    profile: &A11yProfile,
    locale: &str,
) -> LayoutFrame {
    let scale = u32::from(
        profile
            .text_scale_milli
            .clamp(A11yProfile::MIN_SCALE, A11yProfile::MAX_SCALE),
    );
    let mut viewport = viewport;
    viewport.scale_milli = profile
        .text_scale_milli
        .clamp(A11yProfile::MIN_SCALE, A11yProfile::MAX_SCALE);
    let safe = safe_rect(viewport);
    let text_px = (18u32.saturating_mul(scale).div_ceil(1_000)).min(u32::from(u16::MAX)) as u16;
    let mut nodes = Vec::new();
    let _ = place(root, safe, scale, text_px, locale, &mut nodes);
    LayoutFrame {
        viewport,
        safe,
        text_px,
        reduce_motion: profile.reduce_motion,
        nodes,
    }
}

fn px(base: u32, scale: u32) -> u32 {
    base.saturating_mul(scale).div_ceil(1_000).max(1)
}

fn place(
    node: &UiNode,
    within: Rect,
    scale: u32,
    text_px: u16,
    locale: &str,
    out: &mut Vec<LaidOut>,
) -> Rect {
    let pad = px(node.pad, scale);
    let gap = px(node.gap, scale);
    let min_w = px(node.min_w.max(1), scale);
    let min_h = px(node.min_h.max(1), scale);
    let inner = Rect {
        x: within.x.saturating_add(pad),
        y: within.y.saturating_add(pad),
        width: within.width.saturating_sub(pad.saturating_mul(2)),
        height: within.height.saturating_sub(pad.saturating_mul(2)),
    };

    let bounds = match node.kind {
        UiKind::Column => {
            let mut y = inner.y;
            let mut used_h = 0u32;
            for (i, child) in node.children.iter().enumerate() {
                if i > 0 {
                    y = y.saturating_add(gap);
                    used_h = used_h.saturating_add(gap);
                }
                let remaining = inner.height.saturating_sub(used_h);
                let slot = Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: remaining,
                };
                let placed = place(child, slot, scale, text_px, locale, out);
                y = placed.y.saturating_add(placed.height);
                used_h = used_h.saturating_add(placed.height);
            }
            Rect {
                x: within.x,
                y: within.y,
                width: within.width,
                height: used_h
                    .saturating_add(pad.saturating_mul(2))
                    .max(min_h)
                    .min(within.height),
            }
        }
        UiKind::Row => {
            let rtl = is_rtl(locale);
            let mut children: Vec<&UiNode> = node.children.iter().collect();
            if rtl {
                children.reverse();
            }
            let mut x = inner.x;
            let mut used_w = 0u32;
            let mut max_h = min_h;
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    x = x.saturating_add(gap);
                    used_w = used_w.saturating_add(gap);
                }
                let remaining = inner.width.saturating_sub(used_w);
                let slot = Rect {
                    x,
                    y: inner.y,
                    width: remaining,
                    height: inner.height,
                };
                let placed = place(child, slot, scale, text_px, locale, out);
                x = placed.x.saturating_add(placed.width);
                used_w = used_w.saturating_add(placed.width);
                max_h = max_h.max(placed.height);
            }
            Rect {
                x: within.x,
                y: within.y,
                width: used_w
                    .saturating_add(pad.saturating_mul(2))
                    .max(min_w)
                    .min(within.width),
                height: max_h
                    .saturating_add(pad.saturating_mul(2))
                    .max(min_h)
                    .min(within.height),
            }
        }
        _ => {
            let text_w = text_width(locale, &node.text, u32::from(text_px));
            let w = min_w
                .max(text_w.saturating_add(pad.saturating_mul(2)))
                .min(within.width);
            let lines = wrap_lines(
                locale,
                &node.text,
                u32::from(text_px),
                within.width.saturating_sub(pad.saturating_mul(2)),
            );
            let raw_h = min_h.max(
                u32::from(text_px)
                    .saturating_mul(lines.max(1))
                    .saturating_add(pad.saturating_mul(2)),
            );
            Rect {
                x: within.x,
                y: within.y,
                width: w.max(1),
                height: raw_h.min(within.height).max(1),
            }
        }
    };

    let overflow = match node.kind {
        UiKind::Column | UiKind::Row | UiKind::Spacer => {
            bounds.x.saturating_add(bounds.width) > within.x.saturating_add(within.width)
                || bounds.y.saturating_add(bounds.height) > within.y.saturating_add(within.height)
        }
        _ => {
            let avail = bounds.width.saturating_sub(pad.saturating_mul(2));
            let glyph = glyph_advance(locale, u32::from(text_px));
            let lines = wrap_lines(
                locale,
                &node.text,
                u32::from(text_px),
                within.width.saturating_sub(pad.saturating_mul(2)),
            );
            let raw_h = min_h.max(
                u32::from(text_px)
                    .saturating_mul(lines.max(1))
                    .saturating_add(pad.saturating_mul(2)),
            );
            (!node.text.is_empty() && avail < glyph) || raw_h > within.height
        }
    };

    out.push(LaidOut {
        id: node.id.clone(),
        bounds,
        text: node.text.clone(),
        name: node.name.clone(),
        value: node.value.clone(),
        hint: node.hint.clone(),
        role: node.role,
        focusable: node.focusable,
        enabled: node.enabled,
        kind: node.kind,
        overflow,
    });
    bounds
}

fn is_rtl(locale: &str) -> bool {
    locale == "ar" || locale.starts_with("ar-")
}

fn is_cjk(locale: &str) -> bool {
    matches!(locale, "ja" | "ko" | "zh-Hans" | "zh-Hant") || locale.starts_with("zh")
}

fn glyph_advance(locale: &str, text_px: u32) -> u32 {
    if is_cjk(locale) {
        text_px
    } else if is_rtl(locale) {
        text_px.saturating_mul(7).div_ceil(10)
    } else {
        text_px.saturating_mul(6).div_ceil(10)
    }
}

fn text_width(locale: &str, text: &str, text_px: u32) -> u32 {
    let chars = text.chars().count() as u32;
    glyph_advance(locale, text_px).saturating_mul(chars)
}

fn wrap_lines(locale: &str, text: &str, text_px: u32, width: u32) -> u32 {
    if text.is_empty() || width == 0 {
        return 1;
    }
    let adv = glyph_advance(locale, text_px).max(1);
    let per = (width / adv).max(1);
    let chars = text.chars().count() as u32;
    chars.div_ceil(per).max(1)
}

fn intersects(a: Rect, b: Rect) -> bool {
    let ar = a.x.saturating_add(a.width);
    let ab = a.y.saturating_add(a.height);
    let br = b.x.saturating_add(b.width);
    let bb = b.y.saturating_add(b.height);
    a.x < br && b.x < ar && a.y < bb && b.y < ab
}

/// 16:9 / 16:10 / 4:3 / 21:9 capture surfaces.
pub const CAPTURE_ASPECTS: &[(u32, u32)] = &[
    (1_920, 1_080),
    (1_920, 1_200),
    (1_440, 1_080),
    (2_560, 1_080),
];

/// First-title shipping locales plus the overflow pseudo-locale.
pub const CAPTURE_LOCALES: &[&str] = &[
    "en", "es", "fr", "de", "ja", "ko", "zh-Hans", "pt-BR", "ar", "ru", "en-XA",
];

/// Viewport for an aspect with a modest desktop safe area.
#[must_use]
pub fn aspect_viewport(width: u32, height: u32) -> HudViewport {
    HudViewport {
        width,
        height,
        scale_milli: 1_000,
        safe_area: SafeArea {
            left: 24,
            top: 24,
            right: 24,
            bottom: 24,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_ir::A11yProfile;

    fn tree() -> UiNode {
        UiNode::column("root")
            .with(UiNode::label("title", "Pause"))
            .with(UiNode::button("resume", "Resume"))
            .with(UiNode::button("settings", "Settings"))
    }

    #[test]
    fn column_places_buttons_inside_safe_area() {
        let profile = A11yProfile::first_title();
        let frame = layout(&tree(), aspect_viewport(1_920, 1_080), &profile, "en");
        frame.check("en").unwrap();
        assert!(frame.nodes.iter().any(|n| n.id == "resume" && n.focusable));
        assert!(frame.overflows().is_empty());
        assert!(frame.overlaps().is_empty());
    }

    #[test]
    fn large_text_still_fits_four_by_three() {
        let profile = A11yProfile::max_access();
        let frame = layout(&tree(), aspect_viewport(1_440, 1_080), &profile, "de");
        frame.check("de").unwrap();
        assert!(frame.text_px >= 36);
    }

    #[test]
    fn arabic_row_is_rtl() {
        let row = UiNode::row("bar")
            .with(UiNode::button("a", "A"))
            .with(UiNode::button("b", "B"));
        let profile = A11yProfile::first_title();
        let ltr = layout(&row, aspect_viewport(800, 400), &profile, "en");
        let rtl = layout(&row, aspect_viewport(800, 400), &profile, "ar");
        let ax = ltr.nodes.iter().find(|n| n.id == "a").unwrap().bounds.x;
        let bx = ltr.nodes.iter().find(|n| n.id == "b").unwrap().bounds.x;
        let ax_r = rtl.nodes.iter().find(|n| n.id == "a").unwrap().bounds.x;
        let bx_r = rtl.nodes.iter().find(|n| n.id == "b").unwrap().bounds.x;
        assert!(ax < bx);
        assert!(ax_r > bx_r);
    }

    #[test]
    fn tiny_surface_overflows() {
        let profile = A11yProfile::max_access();
        let mut viewport = aspect_viewport(80, 40);
        viewport.safe_area = SafeArea::default();
        let frame = layout(&tree(), viewport, &profile, "en-XA");
        assert!(!frame.overflows().is_empty());
    }
}
