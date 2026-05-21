//! Cross-platform components that emit into a `Scene`.
//!
//! The surface mirrors [`miklabs/ui`] (`mkui-core::components`) and
//! [shadcn/ui]: every overlay is composed from `Card`, `Button`, `Input`,
//! `Slider`, `ChipGroup`, `Scrollbar`, `Swatch`, `Label`, `Text`. Each
//! component takes a
//! `variant` and a `size` and resolves concrete colors through [`HudTheme`]
//! — the same cva-style shape shadcn uses for its `ButtonVariant` /
//! `ButtonSize`. State (`ButtonState::Idle` / `Active`) is orthogonal to
//! variant and supplied per-frame, because StoneSketch is immediate-mode
//! and does not retain a component tree between frames.
//!
//! Two layers:
//!
//! - **component builders** — [`card`], [`button`], [`input`], [`slider`],
//!   [`chip_group`], [`scrollbar`], [`swatch`], [`heading`], [`text`]. Each takes a
//!   variant / size / state and pulls concrete colors from [`HudTheme`].
//!   This is the layer StoneSketch overlays call.
//! - **primitives** — [`panel`], [`titled_panel`], [`label`], [`button_with`].
//!   These take pre-resolved `PanelStyle` / `ButtonStyle` values and are the
//!   escape hatch when a caller needs a one-off style that isn't in the
//!   theme.
//!
//! [shadcn/ui]: https://ui.shadcn.com/
//! [`miklabs/ui`]: https://github.com/mikbry/ui

use crate::theme::{
    ButtonSize, ButtonState, ButtonStyle, ButtonVariant, CardStyle, HudTheme, InputStyle,
    PanelStyle, ScrollbarStyle, SliderStyle, SwatchStyle, TextVariant,
};
use crate::types::{
    CornerRadii, HitRegion, PanelLayout, Point, Quad, Rect, Scene, Shadow, Size, Stroke, Text,
    TextAlign, TextStyle,
};

// ---------- Primitive builders ----------

/// Panel chrome (shadow + rounded fill + stroke). Returns the padded inner
/// content rect. This is the low-level primitive `card` is built from.
pub fn panel(scene: &mut Scene, rect: Rect, style: PanelStyle) -> Rect {
    scene.shadow(Shadow {
        rect,
        blur_radius: style.shadow.blur_radius,
        spread: style.shadow.spread,
        color: style.shadow.color,
        corner_radii: CornerRadii::all(style.corner_radius),
    });
    scene.quad(Quad {
        rect,
        fill: style.fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(style.stroke),
    });
    rect.inset(style.padding)
}

/// Rounded label at `rect` with the given text style. Thin wrapper around
/// `Scene::text` to match the shadcn `Label` widget shape.
pub fn label(scene: &mut Scene, rect: Rect, text: impl Into<String>, style: TextStyle) {
    scene.text(Text {
        rect,
        content: text.into(),
        style,
    });
}

/// Panel with a leading title row. Returns the remaining content rect
/// below the title so callers can continue laying out body content.
pub fn titled_panel(
    scene: &mut Scene,
    rect: Rect,
    title: &str,
    style: PanelStyle,
    title_style: TextStyle,
) -> Rect {
    let content = panel(scene, rect, style);
    label(
        scene,
        Rect::new(
            content.origin,
            Size::new(content.size.width, title_style.line_height_px),
        ),
        title,
        title_style,
    );
    let advance = title_style.line_height_px + 4.0;
    Rect::new(
        Point::new(content.origin.x, content.origin.y + advance),
        Size::new(content.size.width, (content.size.height - advance).max(0.0)),
    )
}

/// Low-level two-state button. Takes a pre-resolved `ButtonStyle` — reserved
/// for callers that need a one-off style. Most call sites should use the
/// themed [`button`] instead.
pub fn button_with(
    scene: &mut Scene,
    rect: Rect,
    label_text: &str,
    state: ButtonState,
    style: &ButtonStyle,
) {
    let (fill, stroke_color, label_color) = match state {
        ButtonState::Idle => (style.idle_fill, style.idle_stroke, style.idle_label),
        ButtonState::Active => (style.active_fill, style.active_stroke, style.active_label),
    };
    scene.quad(Quad {
        rect,
        fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: stroke_color,
            width: style.stroke_width,
        }),
    });
    let text_style = TextStyle {
        align: TextAlign::Center,
        color: label_color,
        ..style.label_style
    };
    let label_height = text_style.line_height_px;
    let label_rect = Rect::new(
        Point::new(
            rect.origin.x,
            rect.origin.y + (rect.size.height - label_height) * 0.5,
        ),
        Size::new(rect.size.width, label_height),
    );
    label(scene, label_rect, label_text, text_style);
}

// ---------- Variant-driven widgets (shadcn / miklabs-ui shape) ----------

/// Card chrome — rounded rect with fill, stroke, no shadow. The shadcn
/// "Card" primitive (minus the typography slot). Use this for inline list
/// rows and inspector row backgrounds.
pub fn card(scene: &mut Scene, rect: Rect, style: CardStyle) {
    scene.quad(Quad {
        rect,
        fill: style.fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.stroke,
            width: style.stroke_width,
        }),
    });
}

/// Text input chrome. Draws the input box and right-aligned value text;
/// focus / editing state is supplied by the caller each frame.
pub fn input(scene: &mut Scene, rect: Rect, value: impl Into<String>, style: InputStyle) {
    scene.quad(Quad {
        rect,
        fill: style.fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.stroke,
            width: style.stroke_width,
        }),
    });
    let text_rect = rect.inset(style.padding);
    let text_style = TextStyle {
        align: TextAlign::End,
        color: style.text_color,
        ..style.text_style
    };
    label(scene, text_rect, value, text_style);
}

/// Small rounded color sample used in list rows and material/family UI.
pub fn swatch(scene: &mut Scene, rect: Rect, fill: crate::types::Color, style: SwatchStyle) {
    scene.quad(Quad {
        rect,
        fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.stroke,
            width: style.stroke_width,
        }),
    });
}

/// Two-state button with shadcn-style `variant` + `size`. Resolves through
/// the provided [`HudTheme`] so a theme swap is a one-line change at every
/// call site.
pub fn button(
    scene: &mut Scene,
    rect: Rect,
    label_text: &str,
    variant: ButtonVariant,
    size: ButtonSize,
    state: ButtonState,
    theme: &HudTheme,
) {
    let style = theme.button_style(variant, size);
    button_with(scene, rect, label_text, state, &style);
}

/// Return value of [`slider`]. `hit_rect` is the rect the caller should push
/// into their `PanelLayout<T>` (expanded vertically for easier grabbing);
/// `thumb_rect` is useful for tests and debugging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderRegions {
    pub track_rect: Rect,
    pub hit_rect: Rect,
    pub thumb_rect: Rect,
}

/// Horizontal slider — track with a filled portion + a circular thumb.
///
/// `fraction` is clamped to `[0.0, 1.0]`. The caller supplies the outer
/// `track_rect`; the widget draws everything and returns the regions to
/// hit-test.
pub fn slider(
    scene: &mut Scene,
    track_rect: Rect,
    fraction: f32,
    style: &SliderStyle,
) -> SliderRegions {
    let fraction = fraction.clamp(0.0, 1.0);

    scene.quad(Quad {
        rect: track_rect,
        fill: style.track_fill,
        corner_radii: CornerRadii::all(style.track_corner_radius),
        stroke: Some(Stroke {
            color: style.track_stroke,
            width: style.track_stroke_width,
        }),
    });
    let filled_rect = Rect::new(
        track_rect.origin,
        Size::new(track_rect.size.width * fraction, track_rect.size.height),
    );
    scene.quad(Quad {
        rect: filled_rect,
        fill: style.filled_fill,
        corner_radii: CornerRadii::all(style.track_corner_radius),
        stroke: None,
    });
    let thumb_center_x = track_rect.origin.x + track_rect.size.width * fraction;
    let thumb_rect = Rect::new(
        Point::new(
            thumb_center_x - style.thumb_diameter * 0.5,
            track_rect.origin.y + (track_rect.size.height - style.thumb_diameter) * 0.5,
        ),
        Size::new(style.thumb_diameter, style.thumb_diameter),
    );
    scene.quad(Quad {
        rect: thumb_rect,
        fill: style.thumb_fill,
        corner_radii: CornerRadii::all(style.thumb_diameter * 0.5),
        stroke: Some(Stroke {
            color: style.thumb_stroke,
            width: 1.0,
        }),
    });

    let hit_rect = Rect::new(
        Point::new(track_rect.origin.x, track_rect.origin.y - style.hit_padding),
        Size::new(
            track_rect.size.width,
            track_rect.size.height + style.hit_padding * 2.0,
        ),
    );

    SliderRegions {
        track_rect,
        hit_rect,
        thumb_rect,
    }
}

/// Vertical scrollbar — pill-shaped track + thumb. The caller precomputes
/// both rects (in the layout pass, so hit-testing matches rendering) and
/// the widget emits the pill quads.
pub fn scrollbar(scene: &mut Scene, track_rect: Rect, thumb_rect: Rect, style: &ScrollbarStyle) {
    scene.quad(Quad {
        rect: track_rect,
        fill: style.track_fill,
        corner_radii: CornerRadii::all(track_rect.size.width * 0.5),
        stroke: None,
    });
    scene.quad(Quad {
        rect: thumb_rect,
        fill: style.thumb_fill,
        corner_radii: CornerRadii::all(thumb_rect.size.width * 0.5),
        stroke: None,
    });
}

/// Horizontal group of toggle chips (shadcn "Toggle Group" shape). Options
/// share one row, one is marked active, and the widget pushes a `HitRegion`
/// per chip into `layout` keyed by `to_target(index)`.
///
/// Returns the emitted chip rects (matching index → option order) so the
/// caller can wire their own layout / test assertions.
// Rationale: signature mirrors `button` (variant / size / theme as separate
// args) plus the two extras `chip_group` needs — `layout` to push HitRegions
// into and `to_target` to key them. Bundling these into a param-struct would
// diverge from the rest of the components module without clarity gain.
#[allow(clippy::too_many_arguments)]
pub fn chip_group<T>(
    scene: &mut Scene,
    layout: &mut PanelLayout<T>,
    row_rect: Rect,
    labels: &[&str],
    selected: usize,
    variant: ButtonVariant,
    size: ButtonSize,
    theme: &HudTheme,
    to_target: impl Fn(usize) -> T,
) -> Vec<Rect> {
    const INNER_PADDING: f32 = 10.0;
    const GAP: f32 = 6.0;
    const CHIP_HEIGHT: f32 = 22.0;
    const CHIP_BOTTOM_INSET: f32 = 6.0;

    if labels.is_empty() {
        return Vec::new();
    }

    let style = theme.button_style(variant, size);
    let count = labels.len() as f32;
    let available = row_rect.size.width - INNER_PADDING * 2.0 - GAP * (count - 1.0);
    let chip_width = (available / count).max(40.0);
    let chip_y = row_rect.origin.y + row_rect.size.height - CHIP_HEIGHT - CHIP_BOTTOM_INSET;

    let mut rects = Vec::with_capacity(labels.len());
    for (index, label_text) in labels.iter().enumerate() {
        let chip_rect = Rect::new(
            Point::new(
                row_rect.origin.x + INNER_PADDING + index as f32 * (chip_width + GAP),
                chip_y,
            ),
            Size::new(chip_width, CHIP_HEIGHT),
        );
        let state = if index == selected {
            ButtonState::Active
        } else {
            ButtonState::Idle
        };
        button_with(scene, chip_rect, label_text, state, &style);
        layout.hit_regions.push(HitRegion {
            rect: chip_rect,
            target: to_target(index),
        });
        rects.push(chip_rect);
    }
    rects
}

/// Convenience: emit the title row of a [`titled_panel`] as a themed
/// heading. Kept for symmetry with miklabs/ui's `Text` component; callers
/// who want the raw `TextStyle` can still use [`label`] directly.
pub fn heading(scene: &mut Scene, rect: Rect, text: impl Into<String>, theme: &HudTheme) {
    label(scene, rect, text, theme.text_style(TextVariant::Heading));
}

/// Convenience: emit body text with the theme's body style.
pub fn text(
    scene: &mut Scene,
    rect: Rect,
    text: impl Into<String>,
    variant: TextVariant,
    theme: &HudTheme,
) {
    label(scene, rect, text, theme.text_style(variant));
}

/// Vertical list of lines under a title inside a panel — the familiar
/// "mode / status / history" card used by the HUD. Kept for the existing
/// overlay code; new panels should compose [`titled_panel`] + [`label`]
/// directly.
pub fn hud_list(
    scene: &mut Scene,
    rect: Rect,
    title: &str,
    lines: &[impl AsRef<str>],
    theme: &HudTheme,
) {
    let content = panel(scene, rect, theme.panel);
    label(
        scene,
        Rect::new(
            content.origin,
            Size::new(content.size.width, theme.title_style.line_height_px),
        ),
        title,
        theme.title_style,
    );

    let mut y = content.origin.y + theme.title_style.line_height_px + 6.0;
    for line in lines {
        label(
            scene,
            Rect::new(
                Point::new(content.origin.x, y),
                Size::new(content.size.width, theme.body_style.line_height_px),
            ),
            line.as_ref(),
            theme.body_style,
        );
        y += theme.body_style.line_height_px + 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Primitive, Scene, Size};

    #[test]
    fn hud_list_builds_panel_and_text() {
        let mut scene = Scene::new(Size::new(800.0, 600.0));
        let theme = HudTheme::default();
        hud_list(
            &mut scene,
            Rect::new(Point::new(20.0, 20.0), Size::new(260.0, 140.0)),
            "Status",
            &["Plan view", "Undo Ctrl+Z"],
            &theme,
        );

        assert!(scene.primitives.len() >= 4);
        assert!(matches!(scene.primitives[0], Primitive::Shadow(_)));
        assert!(scene
            .primitives
            .iter()
            .any(|primitive| matches!(primitive, Primitive::Text(_))));
    }

    #[test]
    fn button_emits_different_fills_for_idle_and_active() {
        let theme = HudTheme::default();
        let mut idle_scene = Scene::new(Size::new(200.0, 80.0));
        let mut active_scene = Scene::new(Size::new(200.0, 80.0));
        button(
            &mut idle_scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            "Sel",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            &theme,
        );
        button(
            &mut active_scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            "Sel",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Active,
            &theme,
        );
        let idle_fill = first_quad_fill(&idle_scene).expect("idle quad");
        let active_fill = first_quad_fill(&active_scene).expect("active quad");
        assert_ne!(idle_fill, active_fill);
    }

    #[test]
    fn titled_panel_returns_content_below_title() {
        let mut scene = Scene::new(Size::new(400.0, 400.0));
        let theme = HudTheme::default();
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(260.0, 200.0));
        let body = titled_panel(&mut scene, rect, "Mode", theme.panel, theme.title_style);
        let content = rect.inset(theme.panel.padding);
        assert!(body.origin.y > content.origin.y);
        assert!(body.origin.y >= content.origin.y + theme.title_style.line_height_px);
        assert!(body.size.height < content.size.height);
        assert!(scene.primitives.iter().any(|p| match p {
            Primitive::Text(t) => t.content == "Mode",
            _ => false,
        }));
    }

    #[test]
    fn slider_thumb_sits_proportional_to_fraction() {
        let mut scene = Scene::new(Size::new(400.0, 80.0));
        let theme = HudTheme::default();
        let style = theme.slider();
        let track = Rect::new(Point::new(10.0, 40.0), Size::new(200.0, 8.0));
        let regions = slider(&mut scene, track, 0.25, &style);
        let thumb_center = regions.thumb_rect.origin.x + regions.thumb_rect.size.width * 0.5;
        let expected = track.origin.x + track.size.width * 0.25;
        assert!((thumb_center - expected).abs() < 0.5);
    }

    #[test]
    fn chip_group_pushes_one_hit_region_per_option() {
        let mut scene = Scene::new(Size::new(400.0, 200.0));
        let theme = HudTheme::default();
        let mut layout: PanelLayout<usize> = PanelLayout::default();
        let row = Rect::new(Point::new(0.0, 0.0), Size::new(300.0, 50.0));
        let rects = chip_group(
            &mut scene,
            &mut layout,
            row,
            &["A", "B", "C"],
            1,
            ButtonVariant::Outline,
            ButtonSize::Sm,
            &theme,
            |i| i,
        );
        assert_eq!(rects.len(), 3);
        assert_eq!(layout.hit_regions.len(), 3);
        assert_eq!(layout.hit_regions[1].target, 1);
    }

    #[test]
    fn input_draws_box_and_text() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        let theme = HudTheme::default();
        input(
            &mut scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(90.0, 20.0)),
            "1.20 m",
            theme.input(false),
        );

        assert!(scene
            .primitives
            .iter()
            .any(|primitive| matches!(primitive, Primitive::Quad(_))));
        assert!(scene.primitives.iter().any(|primitive| matches!(
            primitive,
            Primitive::Text(text) if text.content == "1.20 m"
        )));
    }

    #[test]
    fn swatch_draws_colored_quad() {
        let mut scene = Scene::new(Size::new(80.0, 80.0));
        let theme = HudTheme::default();
        let fill = crate::types::Color::rgba(0.3, 0.4, 0.5, 1.0);
        swatch(
            &mut scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(14.0, 14.0)),
            fill,
            theme.swatch(),
        );

        assert!(scene.primitives.iter().any(|primitive| matches!(
            primitive,
            Primitive::Quad(quad) if quad.fill == fill
        )));
    }

    fn first_quad_fill(scene: &Scene) -> Option<crate::types::Color> {
        scene.primitives.iter().find_map(|p| {
            if let Primitive::Quad(q) = p {
                Some(q.fill)
            } else {
                None
            }
        })
    }
}
