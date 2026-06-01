//! `button` / `button_with` — two-state push button.
//!
//! `button` is the themed entry point: shadcn-style `variant` + `size` resolve
//! through [`WgpuTheme`] so a theme swap is a one-line change at every call
//! site. `button_with` is the escape hatch that takes a pre-resolved
//! [`ButtonStyle`] for one-off looks the theme does not cover.

use crate::theme::{ButtonSize, ButtonState, ButtonStyle, ButtonVariant, WgpuTheme};
use crate::types::{CornerRadii, Point, Quad, Rect, Scene, Size, Stroke, TextAlign, TextStyle};

use super::label;

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

/// Two-state button with shadcn-style `variant` + `size`. Resolves through
/// the provided [`WgpuTheme`] so a theme swap is a one-line change at every
/// call site.
pub fn button(
    scene: &mut Scene,
    rect: Rect,
    label_text: &str,
    variant: ButtonVariant,
    size: ButtonSize,
    state: ButtonState,
    theme: &WgpuTheme,
) {
    let style = theme.button_style(variant, size);
    button_with(scene, rect, label_text, state, &style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Primitive, Size};

    fn first_quad_fill(scene: &Scene) -> Option<crate::types::Color> {
        scene.primitives.iter().find_map(|p| {
            if let Primitive::Quad(q) = p {
                Some(q.fill)
            } else {
                None
            }
        })
    }

    #[test]
    fn button_emits_different_fills_for_idle_and_active() {
        let theme = WgpuTheme::default();
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
}
