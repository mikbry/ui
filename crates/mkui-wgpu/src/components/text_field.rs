//! `text_field` — single-line text input chrome.
//!
//! Draws the input box plus right-aligned value text; focus / editing state is
//! supplied by the caller each frame (the renderer is immediate-mode and keeps
//! no component tree between frames). Named `text_field` rather than shadcn's
//! `Input` to disambiguate from the crate's pointer-input plumbing
//! ([`crate::pointer`]) and to match the renderer's actual semantic — a
//! single-line text input field, not a generic input router.

use crate::theme::InputStyle;
use crate::types::{CornerRadii, Quad, Rect, Scene, Stroke, TextAlign, TextStyle};

use super::label;

/// Text input chrome. Draws the input box and right-aligned value text;
/// focus / editing state is supplied by the caller each frame.
pub fn text_field(scene: &mut Scene, rect: Rect, value: impl Into<String>, style: InputStyle) {
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
}
