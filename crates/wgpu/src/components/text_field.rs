//! `text_field` — single-line text input chrome.
//!
//! Draws the input box plus right-aligned value text; focus / editing state is
//! supplied by the caller each frame (the renderer is immediate-mode and keeps
//! no component tree between frames). Named `text_field` rather than shadcn's
//! `Input` to disambiguate from the crate's pointer-input plumbing
//! ([`crate::pointer`]) and to match the renderer's actual semantic — a
//! single-line text input field, not a generic input router.
//!
//! [`text_field_editable`] (#185) extends this with caret, selection, and
//! IME-composition rendering, driven by a caller-owned [`TextEditState`].
//! [`text_field`] itself is unchanged and stays display-only — the one
//! existing call site ([`crate::builder::UiBuilder::number_row`]) shows a
//! read-only numeric value, not an editable field.
//!
//! Caret/selection x-positions are a per-char-count approximation (see
//! [`approx_x_offset`]), not real glyph measurement — `mkui-text` carries the
//! glyph advances needed for exact placement (`LayoutGlyph::x_advance_px`),
//! but no caller-facing measurement API exposes them yet pending #182
//! ("text layout ... measurement"). Visually this reads fine for the typical
//! immediate-mode HUD use case (short single-line values) with a
//! roughly-monospace-ish font; it's a known approximation to tighten once
//! #182 lands, not a correctness bug in the logical editing model
//! ([`TextEditState`]), which operates on exact byte offsets throughout.

use crate::text_edit::TextEditState;
use crate::theme::InputStyle;
use crate::types::{CornerRadii, Point, Quad, Rect, Scene, Size, Stroke, TextAlign, TextStyle};

use super::label;

fn input_box(scene: &mut Scene, rect: Rect, style: &InputStyle) {
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
pub fn text_field(scene: &mut Scene, rect: Rect, value: impl Into<String>, style: InputStyle) {
    input_box(scene, rect, &style);
    let text_rect = rect.inset(style.padding);
    let text_style = TextStyle {
        align: TextAlign::End,
        color: style.text_color,
        ..style.text_style
    };
    label(scene, text_rect, value, text_style);
}

/// Approximate advance width per char, used only to place the caret and
/// selection highlight. See the module doc for why this isn't exact glyph
/// measurement.
fn approx_char_advance_px(text_style: &TextStyle) -> f32 {
    text_style.font_size_px * 0.5
}

/// Approximate x offset, from the start of the text box, of the byte
/// position `byte_idx` within `text`. Counts chars (not bytes) up to
/// `byte_idx` so multi-byte UTF-8 text isn't over-counted.
fn approx_x_offset(text: &str, byte_idx: usize, text_style: &TextStyle) -> f32 {
    let chars_before = text[..byte_idx].chars().count() as f32;
    chars_before * approx_char_advance_px(text_style)
}

/// Editable single-line text field: input chrome, left-aligned text,
/// a selection highlight, an IME-composing underline, and a caret (#185).
///
/// `state` is caller-owned and persists across frames, like
/// [`crate::pointer::PointerState`] — this function only reads it to draw.
/// `caret_visible` lets the caller blink the caret (toggle a timer) without
/// this function needing to know about time; pass `true` every frame for a
/// steady caret.
///
/// Returns the inset text rect, in case the caller wants it for further
/// layout (e.g. hit-testing clicks to a byte offset — left to the caller
/// pending the input-routing contract, #176).
pub fn text_field_editable(
    scene: &mut Scene,
    rect: Rect,
    state: &TextEditState,
    style: InputStyle,
    caret_visible: bool,
) -> Rect {
    input_box(scene, rect, &style);
    let text_rect = rect.inset(style.padding);
    let text_style = TextStyle {
        align: TextAlign::Start,
        color: style.text_color,
        ..style.text_style
    };
    let text = state.text();

    if let Some((start, end)) = state.selection() {
        let x0 = text_rect.origin.x + approx_x_offset(text, start, &text_style);
        let x1 = text_rect.origin.x + approx_x_offset(text, end, &text_style);
        scene.quad(Quad {
            rect: Rect::new(
                Point::new(x0, text_rect.origin.y),
                Size::new((x1 - x0).max(0.0), text_rect.size.height),
            ),
            fill: style.selection_fill,
            corner_radii: CornerRadii::all(0.0),
            stroke: None,
        });
    }

    label(scene, text_rect, text, text_style);

    if let Some((start, end)) = state.composing() {
        let x0 = text_rect.origin.x + approx_x_offset(text, start, &text_style);
        let x1 = text_rect.origin.x + approx_x_offset(text, end, &text_style);
        let underline_width = (x1 - x0).max(approx_char_advance_px(&text_style) * 0.25);
        scene.quad(Quad {
            rect: Rect::new(
                Point::new(x0, text_rect.height_end() - 1.0),
                Size::new(underline_width, 1.0),
            ),
            fill: style.caret_color,
            corner_radii: CornerRadii::all(0.0),
            stroke: None,
        });
    }

    if caret_visible {
        let x = text_rect.origin.x + approx_x_offset(text, state.caret(), &text_style);
        scene.quad(Quad {
            rect: Rect::new(
                Point::new(x, text_rect.origin.y),
                Size::new(1.0, text_rect.size.height),
            ),
            fill: style.caret_color,
            corner_radii: CornerRadii::all(0.0),
            stroke: None,
        });
    }

    text_rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Point, Primitive, Size};

    #[test]
    fn text_field_draws_box_and_text() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        let theme = WgpuTheme::default();
        text_field(
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

    fn quad_count(scene: &Scene) -> usize {
        scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .count()
    }

    fn editable_rect() -> Rect {
        Rect::new(Point::new(10.0, 10.0), Size::new(200.0, 20.0))
    }

    #[test]
    fn text_field_editable_draws_box_text_and_caret() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        let theme = WgpuTheme::default();
        let state = TextEditState::new("hello");
        text_field_editable(&mut scene, editable_rect(), &state, theme.input(true), true);

        // Input box + caret bar; no selection, no composing.
        assert_eq!(quad_count(&scene), 2);
        assert!(scene.primitives.iter().any(|p| matches!(
            p,
            Primitive::Text(text) if text.content == "hello"
        )));
    }

    #[test]
    fn text_field_editable_omits_caret_when_not_visible() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        let theme = WgpuTheme::default();
        let state = TextEditState::new("hello");
        text_field_editable(
            &mut scene,
            editable_rect(),
            &state,
            theme.input(true),
            false,
        );

        // Input box only.
        assert_eq!(quad_count(&scene), 1);
    }

    #[test]
    fn text_field_editable_draws_selection_highlight() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        let theme = WgpuTheme::default();
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        state.move_right(true);
        state.move_right(true);
        let rect = editable_rect();
        text_field_editable(&mut scene, rect, &state, theme.input(true), true);

        // Input box + selection highlight + caret bar.
        assert_eq!(quad_count(&scene), 3);
        let selection_quad = scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) if q.fill == theme.input(true).selection_fill => Some(q),
                _ => None,
            })
            .next()
            .expect("selection highlight quad present");
        assert!(selection_quad.rect.origin.x >= rect.origin.x);
        assert!(selection_quad.rect.size.width > 0.0);
    }

    #[test]
    fn text_field_editable_caret_x_advances_with_caret_position() {
        let theme = WgpuTheme::default();
        let rect = editable_rect();

        let mut scene_start = Scene::new(Size::new(240.0, 80.0));
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        text_field_editable(&mut scene_start, rect, &state, theme.input(true), true);

        let mut scene_end = Scene::new(Size::new(240.0, 80.0));
        state.move_end(false);
        text_field_editable(&mut scene_end, rect, &state, theme.input(true), true);

        let caret_x = |scene: &Scene| -> f32 {
            scene
                .primitives
                .iter()
                .filter_map(|p| match p {
                    Primitive::Quad(q) if q.rect.size.width <= 1.5 => Some(q.rect.origin.x),
                    _ => None,
                })
                .next_back()
                .expect("caret quad present")
        };

        assert!(caret_x(&scene_end) > caret_x(&scene_start));
    }

    #[test]
    fn text_field_editable_draws_composing_underline() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        let theme = WgpuTheme::default();
        let mut state = TextEditState::new("ab");
        state.move_left(false);
        state.set_composing("n", None);
        text_field_editable(&mut scene, editable_rect(), &state, theme.input(true), true);

        // Input box + composing underline + caret bar.
        assert_eq!(quad_count(&scene), 3);
        assert!(scene.primitives.iter().any(|p| matches!(
            p,
            Primitive::Text(text) if text.content == "anb"
        )));
    }
}
