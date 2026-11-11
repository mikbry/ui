//! `dialog` / `titled_dialog` — modal chrome: a full-viewport dimming
//! backdrop behind a panel (#188).
//!
//! Pure `Scene` emitters, like every other component in this module — no
//! input-routing knowledge here. The *caller* owns the open/closed state,
//! the panel rect, and the [`crate::input_router::InputRouter`] wiring:
//! each frame a dialog is open, draw it with [`dialog`]/[`titled_dialog`]
//! and feed the same `panel_rect` to
//! [`crate::input_router::InputRouter::push_modal`] so backdrop-click
//! dismiss and the Tab focus trap line up with what's actually painted.
//! Stacking falls out of call order: draw the bottom dialog first and the
//! top dialog second, same as any other overlapping `Scene` content — the
//! second call's backdrop quad dims the first dialog's panel further
//! before its own panel paints on top.

use crate::theme::DialogStyle;
use crate::types::{CornerRadii, Point, Quad, Rect, Scene, TextStyle};

use super::{panel, titled_panel};

/// Full-viewport dimming scrim, drawn first so the panel composites on top.
fn backdrop(scene: &mut Scene, style: &DialogStyle) {
    let viewport_rect = Rect::new(Point::new(0.0, 0.0), scene.viewport);
    scene.quad(Quad {
        rect: viewport_rect,
        fill: style.backdrop,
        corner_radii: CornerRadii::all(0.0),
        stroke: None,
    });
}

/// Backdrop + panel chrome, no title. Returns the padded inner content rect
/// (same as [`panel`]), e.g. for a confirmation dialog that's just body text
/// and action buttons.
pub fn dialog(scene: &mut Scene, panel_rect: Rect, style: DialogStyle) -> Rect {
    backdrop(scene, &style);
    panel(scene, panel_rect, style.panel)
}

/// Backdrop + titled panel. Returns the remaining content rect below the
/// title (same as [`titled_panel`]).
pub fn titled_dialog(
    scene: &mut Scene,
    panel_rect: Rect,
    title: &str,
    style: DialogStyle,
    title_style: TextStyle,
) -> Rect {
    backdrop(scene, &style);
    titled_panel(scene, panel_rect, title, style.panel, title_style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Point, Primitive, Size};

    fn viewport() -> Size {
        Size::new(800.0, 600.0)
    }

    fn panel_rect() -> Rect {
        Rect::new(Point::new(250.0, 200.0), Size::new(300.0, 200.0))
    }

    #[test]
    fn dialog_paints_a_full_viewport_backdrop_behind_the_panel() {
        let mut scene = Scene::new(viewport());
        let theme = WgpuTheme::default();
        dialog(&mut scene, panel_rect(), theme.dialog());

        let backdrop_quad = scene
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Quad(q) if q.fill == theme.dialog().backdrop => Some(q),
                _ => None,
            })
            .expect("backdrop quad present");
        assert_eq!(backdrop_quad.rect.size, viewport());
        assert_eq!(backdrop_quad.rect.origin, Point::new(0.0, 0.0));

        // The backdrop must be painted *before* the panel, so the panel's
        // own quad composites on top of it rather than under it.
        let backdrop_index = scene
            .primitives
            .iter()
            .position(|p| matches!(p, Primitive::Quad(q) if q.fill == theme.dialog().backdrop))
            .unwrap();
        let panel_index = scene
            .primitives
            .iter()
            .position(|p| matches!(p, Primitive::Quad(q) if q.fill == theme.dialog().panel.fill))
            .unwrap();
        assert!(backdrop_index < panel_index);
    }

    #[test]
    fn dialog_returns_the_panels_padded_content_rect() {
        let mut scene = Scene::new(viewport());
        let theme = WgpuTheme::default();
        let content = dialog(&mut scene, panel_rect(), theme.dialog());
        assert_eq!(content, panel_rect().inset(theme.dialog().panel.padding));
    }

    #[test]
    fn titled_dialog_draws_backdrop_panel_and_title() {
        let mut scene = Scene::new(viewport());
        let theme = WgpuTheme::default();
        let body = titled_dialog(
            &mut scene,
            panel_rect(),
            "Settings",
            theme.dialog(),
            theme.title_style(),
        );

        assert!(scene.primitives.iter().any(|p| match p {
            Primitive::Text(t) => t.content == "Settings",
            _ => false,
        }));
        let content = panel_rect().inset(theme.dialog().panel.padding);
        assert!(
            body.origin.y > content.origin.y,
            "content sits below the title"
        );
    }

    #[test]
    fn two_stacked_dialogs_each_paint_their_own_backdrop() {
        let mut scene = Scene::new(viewport());
        let theme = WgpuTheme::default();
        dialog(
            &mut scene,
            Rect::new(Point::new(50.0, 50.0), Size::new(200.0, 150.0)),
            theme.dialog(),
        );
        dialog(&mut scene, panel_rect(), theme.dialog());

        let backdrop_count = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(q) if q.fill == theme.dialog().backdrop))
            .count();
        assert_eq!(
            backdrop_count, 2,
            "each stacked dialog paints its own full-viewport backdrop"
        );
    }
}
