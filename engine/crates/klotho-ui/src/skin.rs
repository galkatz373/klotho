//! Presentation-only HUD skinning for an already-gated [`UiManifest`].

use klotho_core::Epoch;
use klotho_manifest::{UiManifest, WidgetKind};

/// An sRGB color with an alpha channel.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel.
    pub a: u8,
}

impl Color {
    /// Construct a color from RGBA channels.
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Pixel rectangle in a HUD viewport.
#[derive(Copy, Clone, Default, Eq, PartialEq, Hash, Debug)]
pub struct Rect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Insets reserved for overscan, notches, or platform chrome.
#[derive(Copy, Clone, Default, Eq, PartialEq, Hash, Debug)]
pub struct SafeArea {
    /// Left inset in pixels.
    pub left: u32,
    /// Top inset in pixels.
    pub top: u32,
    /// Right inset in pixels.
    pub right: u32,
    /// Bottom inset in pixels.
    pub bottom: u32,
}

/// Target surface and user scale for responsive HUD layout.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct HudViewport {
    /// Surface width in pixels.
    pub width: u32,
    /// Surface height in pixels.
    pub height: u32,
    /// User interface scale in thousandths. Values are clamped to 750–2,000.
    pub scale_milli: u16,
    /// Platform safe area.
    pub safe_area: SafeArea,
}

impl HudViewport {
    /// A viewport with no platform insets and 100% UI scale.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            scale_milli: 1_000,
            safe_area: SafeArea {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
        }
    }
}

/// Semantic screen region selected from an existing widget kind.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum HudSlot {
    /// Persistent resources at the lower-left.
    Status,
    /// Attention messages at the upper-right.
    Notice,
    /// A centered interaction prompt near the bottom.
    Prompt,
    /// A centered choice or inventory list.
    Menu,
    /// A presenter-projected diegetic label.
    World,
}

/// Production HUD color tokens. They affect presentation only.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct HudPalette {
    /// Main text.
    pub text: Color,
    /// Translucent panel.
    pub panel: Color,
    /// Resource meter track.
    pub track: Color,
    /// Resource meter fill and focus accent.
    pub accent: Color,
    /// Prompt accent.
    pub prompt: Color,
}

impl HudPalette {
    /// Neutral dark production palette.
    pub const DEFAULT: Self = Self {
        text: Color::rgba(242, 240, 232, 255),
        panel: Color::rgba(12, 16, 24, 218),
        track: Color::rgba(48, 55, 68, 244),
        accent: Color::rgba(218, 93, 65, 255),
        prompt: Color::rgba(232, 190, 92, 255),
    };

    /// High-contrast palette for accessibility presets.
    pub const HIGH_CONTRAST: Self = Self {
        text: Color::rgba(255, 255, 255, 255),
        panel: Color::rgba(0, 0, 0, 242),
        track: Color::rgba(44, 44, 44, 255),
        accent: Color::rgba(0, 215, 255, 255),
        prompt: Color::rgba(255, 224, 0, 255),
    };
}

/// Presentation settings for [`skin_hud`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct HudSkin {
    /// Color tokens.
    pub palette: HudPalette,
    /// Whether the whole HUD is hidden by the active presentation beat.
    pub hidden: bool,
    /// Suppress non-essential HUD motion. Layout is unchanged.
    pub reduce_motion: bool,
}

impl Default for HudSkin {
    fn default() -> Self {
        Self {
            palette: HudPalette::DEFAULT,
            hidden: false,
            reduce_motion: false,
        }
    }
}

impl HudSkin {
    /// High-contrast accessibility preset.
    #[must_use]
    pub const fn high_contrast() -> Self {
        Self {
            palette: HudPalette::HIGH_CONTRAST,
            hidden: false,
            reduce_motion: false,
        }
    }

    /// Skin tokens from an authorable accessibility profile.
    #[must_use]
    pub const fn from_profile(profile: &klotho_ir::A11yProfile) -> Self {
        Self {
            palette: if matches!(profile.contrast, klotho_ir::ContrastMode::High) {
                HudPalette::HIGH_CONTRAST
            } else {
                HudPalette::DEFAULT
            },
            hidden: false,
            reduce_motion: profile.reduce_motion,
        }
    }
}

/// One render-ready, styled form of an existing attention widget.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct HudElement {
    /// Index in [`UiManifest::widgets`], retained for input/focus routing.
    pub source_index: usize,
    /// Semantic region.
    pub slot: HudSlot,
    /// Screen bounds. World labels retain zero bounds until projected.
    pub bounds: Rect,
    /// Primary text copied verbatim from the gated widget.
    pub body: String,
    /// Text color.
    pub foreground: Color,
    /// Panel or meter-track color.
    pub background: Color,
    /// Focus or meter-fill color.
    pub accent: Color,
    /// Text size in pixels.
    pub text_px: u16,
    /// Filled width for a bar, in pixels. `None` for other widget kinds.
    pub bar_fill_px: Option<u32>,
}

/// Disposable, styled HUD output for one observer and frame.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct HudFrame {
    /// Cook epoch copied from the input manifest.
    pub epoch: Epoch,
    /// Whether a presentation beat hid the HUD.
    pub hidden: bool,
    /// Whether motion should be suppressed. Presentation only.
    pub reduce_motion: bool,
    /// Styled elements in stable input order.
    pub elements: Vec<HudElement>,
}

/// Apply responsive production styling to an already Knows-gated manifest.
///
/// This function deliberately has no `WorldSnapshot` or `Canon` input. It can
/// place and color existing widgets, but it cannot discover or invent a fact.
#[must_use]
pub fn skin_hud(ui: &UiManifest, viewport: HudViewport, skin: HudSkin) -> HudFrame {
    if skin.hidden || viewport.width == 0 || viewport.height == 0 {
        return HudFrame {
            epoch: ui.epoch,
            hidden: skin.hidden,
            reduce_motion: skin.reduce_motion,
            elements: Vec::new(),
        };
    }

    let scale = u32::from(viewport.scale_milli.clamp(750, 2_000));
    let px = |base: u32| base.saturating_mul(scale).div_ceil(1_000);
    let safe = safe_rect(viewport);
    let margin = px(24).min(safe.width / 4).min(safe.height / 4);
    let gap = px(10);
    let row_h = px(42).max(1).min(safe.height.max(1));
    let panel_w = px(360).min(safe.width.saturating_sub(margin * 2));
    let prompt_w = px(560).min(safe.width.saturating_sub(margin * 2));
    let text_px = px(18).min(u32::from(u16::MAX)) as u16;

    let mut status_y = safe
        .y
        .saturating_add(safe.height)
        .saturating_sub(margin + row_h);
    let mut notice_y = safe.y.saturating_add(margin);
    let mut menu_y = safe.y.saturating_add(safe.height / 4);
    let mut elements = Vec::with_capacity(ui.widgets.len());

    for (source_index, widget) in ui.widgets.iter().enumerate() {
        let (slot, mut bounds, bar_fill_px, accent) = match widget.kind {
            WidgetKind::Bar { value, cap } => {
                let bounds = Rect {
                    x: safe.x.saturating_add(margin),
                    y: status_y,
                    width: panel_w,
                    height: row_h,
                };
                status_y = status_y.saturating_sub(row_h.saturating_add(gap));
                let fill = if cap <= 0 {
                    0
                } else {
                    let clamped = value.clamp(0, cap);
                    (u64::from(bounds.width) * clamped as u64 / cap as u64) as u32
                };
                (HudSlot::Status, bounds, Some(fill), skin.palette.accent)
            }
            WidgetKind::Prompt => {
                let bounds = Rect {
                    x: safe
                        .x
                        .saturating_add((safe.width.saturating_sub(prompt_w)) / 2),
                    y: safe
                        .y
                        .saturating_add(safe.height)
                        .saturating_sub(margin + row_h),
                    width: prompt_w,
                    height: row_h,
                };
                (HudSlot::Prompt, bounds, None, skin.palette.prompt)
            }
            WidgetKind::List => {
                let bounds = Rect {
                    x: safe
                        .x
                        .saturating_add((safe.width.saturating_sub(panel_w)) / 2),
                    y: menu_y,
                    width: panel_w,
                    height: row_h.saturating_mul(3).min(safe.height),
                };
                menu_y = menu_y.saturating_add(bounds.height.saturating_add(gap));
                (HudSlot::Menu, bounds, None, skin.palette.accent)
            }
            WidgetKind::Label { .. } => {
                (HudSlot::World, Rect::default(), None, skin.palette.prompt)
            }
            WidgetKind::Text => {
                let bounds = Rect {
                    x: safe
                        .x
                        .saturating_add(safe.width)
                        .saturating_sub(margin + panel_w),
                    y: notice_y,
                    width: panel_w,
                    height: row_h,
                };
                notice_y = notice_y.saturating_add(row_h.saturating_add(gap));
                (HudSlot::Notice, bounds, None, skin.palette.accent)
            }
        };
        if slot != HudSlot::World {
            bounds = fit_rect(bounds, safe);
        }
        elements.push(HudElement {
            source_index,
            slot,
            bounds,
            body: widget.body.clone(),
            foreground: skin.palette.text,
            background: skin.palette.panel,
            accent,
            text_px,
            bar_fill_px,
        });
    }

    HudFrame {
        epoch: ui.epoch,
        hidden: false,
        reduce_motion: skin.reduce_motion,
        elements,
    }
}

pub(crate) fn safe_rect(viewport: HudViewport) -> Rect {
    let left = viewport.safe_area.left.min(viewport.width);
    let top = viewport.safe_area.top.min(viewport.height);
    let right = viewport
        .safe_area
        .right
        .min(viewport.width.saturating_sub(left));
    let bottom = viewport
        .safe_area
        .bottom
        .min(viewport.height.saturating_sub(top));
    Rect {
        x: left,
        y: top,
        width: viewport.width.saturating_sub(left).saturating_sub(right),
        height: viewport.height.saturating_sub(top).saturating_sub(bottom),
    }
}

fn fit_rect(rect: Rect, within: Rect) -> Rect {
    let right = within.x.saturating_add(within.width);
    let bottom = within.y.saturating_add(within.height);
    let x = rect.x.clamp(within.x, right);
    let y = rect.y.clamp(within.y, bottom);
    Rect {
        x,
        y,
        width: rect.width.min(right.saturating_sub(x)),
        height: rect.height.min(bottom.saturating_sub(y)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_core::{Epoch, IVec3};
    use klotho_manifest::Widget;

    fn manifest() -> UiManifest {
        UiManifest::from_widgets(
            Epoch(7),
            [
                Widget {
                    kind: WidgetKind::Text,
                    body: "gate open".into(),
                },
                Widget {
                    kind: WidgetKind::Bar {
                        value: 75,
                        cap: 100,
                    },
                    body: "stamina".into(),
                },
                Widget {
                    kind: WidgetKind::Prompt,
                    body: "hold use".into(),
                },
                Widget {
                    kind: WidgetKind::List,
                    body: "bucket\nhammer".into(),
                },
                Widget {
                    kind: WidgetKind::Label {
                        world: IVec3 { x: 1, y: 2, z: 3 },
                    },
                    body: "forge".into(),
                },
            ],
        )
    }

    #[test]
    fn styles_existing_widgets_in_stable_order() {
        let frame = skin_hud(
            &manifest(),
            HudViewport::new(1_920, 1_080),
            HudSkin::default(),
        );
        assert_eq!(frame.epoch, Epoch(7));
        assert_eq!(frame.elements.len(), 5);
        assert_eq!(
            frame.elements.iter().map(|e| e.slot).collect::<Vec<_>>(),
            [
                HudSlot::Notice,
                HudSlot::Status,
                HudSlot::Prompt,
                HudSlot::Menu,
                HudSlot::World,
            ]
        );
        assert_eq!(frame.elements[1].bar_fill_px, Some(270));
        assert_eq!(frame.elements[4].bounds, Rect::default());
    }

    #[test]
    fn respects_safe_area_scale_and_tiny_surfaces() {
        let mut viewport = HudViewport::new(320, 180);
        viewport.scale_milli = 4_000;
        viewport.safe_area = SafeArea {
            left: 20,
            top: 10,
            right: 20,
            bottom: 10,
        };
        let frame = skin_hud(&manifest(), viewport, HudSkin::high_contrast());
        assert!(
            frame
                .elements
                .iter()
                .filter(|e| e.slot != HudSlot::World)
                .all(|e| {
                    e.bounds.x >= 20
                        && e.bounds.y >= 10
                        && e.bounds.x.saturating_add(e.bounds.width) <= 300
                        && e.bounds.y.saturating_add(e.bounds.height) <= 170
                })
        );
        assert_eq!(frame.elements[0].foreground, HudPalette::HIGH_CONTRAST.text);
        assert_eq!(frame.elements[0].text_px, 36);
    }

    #[test]
    fn hide_is_presentation_only_and_emits_no_elements() {
        let ui = manifest();
        let frame = skin_hud(
            &ui,
            HudViewport::new(1_920, 1_080),
            HudSkin {
                hidden: true,
                ..HudSkin::default()
            },
        );
        assert!(frame.hidden);
        assert!(frame.elements.is_empty());
        assert_eq!(ui.widgets.len(), 5);
    }

    #[test]
    fn bar_fill_is_clamped_and_zero_cap_is_empty() {
        let ui = UiManifest::from_widgets(
            Epoch::ZERO,
            [
                Widget {
                    kind: WidgetKind::Bar {
                        value: 200,
                        cap: 100,
                    },
                    body: "high".into(),
                },
                Widget {
                    kind: WidgetKind::Bar { value: 1, cap: 0 },
                    body: "invalid".into(),
                },
            ],
        );
        let frame = skin_hud(&ui, HudViewport::new(1_000, 600), HudSkin::default());
        assert_eq!(frame.elements[0].bar_fill_px, Some(360));
        assert_eq!(frame.elements[1].bar_fill_px, Some(0));
    }
}
