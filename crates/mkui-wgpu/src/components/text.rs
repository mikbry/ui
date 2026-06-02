//! `text` / `heading` / `label` — themed typography helpers.
//!
//! `label` is the thin primitive: a direct wrapper around `Scene::text`
//! matching the shadcn `Label` widget shape, and the single place that turns a
//! rect + content + style into a `Text` primitive. `heading` and `text` are
//! themed conveniences over it that pull a concrete [`TextStyle`] from the
//! theme's [`TextVariant`] scale. `heading` is the title-row variant kept for
//! symmetry with miklabs/ui's `Text` component; callers who want a raw
//! [`TextStyle`] can use [`label`] directly.

use crate::theme::{TextVariant, WgpuTheme};
use crate::types::{Rect, Scene, Text, TextStyle};

/// Rounded label at `rect` with the given text style. Thin wrapper around
/// `Scene::text` to match the shadcn `Label` widget shape.
pub fn label(scene: &mut Scene, rect: Rect, text: impl Into<String>, style: TextStyle) {
    scene.text(Text {
        rect,
        content: text.into(),
        style,
    });
}

/// Convenience: emit the title row of a [`titled_panel`](super::titled_panel)
/// as a themed heading. Kept for symmetry with miklabs/ui's `Text` component;
/// callers who want the raw `TextStyle` can still use [`label`] directly.
pub fn heading(scene: &mut Scene, rect: Rect, text: impl Into<String>, theme: &WgpuTheme) {
    label(scene, rect, text, theme.text_style(TextVariant::Heading));
}

/// Convenience: emit body text with the theme's body style.
pub fn text(
    scene: &mut Scene,
    rect: Rect,
    text: impl Into<String>,
    variant: TextVariant,
    theme: &WgpuTheme,
) {
    label(scene, rect, text, theme.text_style(variant));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Point, Primitive, Size};

    #[test]
    fn label_emits_a_single_text_primitive() {
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        let theme = WgpuTheme::default();
        label(
            &mut scene,
            Rect::new(Point::new(0.0, 0.0), Size::new(120.0, 20.0)),
            "Hello",
            theme.body_style,
        );
        let texts: Vec<_> = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Text(_)))
            .collect();
        assert_eq!(texts.len(), 1);
        assert!(matches!(texts[0], Primitive::Text(t) if t.content == "Hello"));
    }

    #[test]
    fn heading_and_text_each_emit_one_text_primitive() {
        let theme = WgpuTheme::default();
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(200.0, 24.0));

        let mut heading_scene = Scene::new(Size::new(300.0, 80.0));
        heading(&mut heading_scene, rect, "Title", &theme);
        let mut body_scene = Scene::new(Size::new(300.0, 80.0));
        text(&mut body_scene, rect, "Body", TextVariant::Body, &theme);

        for scene in [&heading_scene, &body_scene] {
            let texts: Vec<_> = scene
                .primitives
                .iter()
                .filter(|p| matches!(p, Primitive::Text(_)))
                .collect();
            assert_eq!(texts.len(), 1);
        }
    }
}
