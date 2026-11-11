//! `panel` / `titled_panel` — the low-level chrome primitives.
//!
//! `panel` draws shadow + rounded fill + stroke and returns the padded inner
//! content rect; it is the primitive `card` and the higher-level builders are
//! composed from. `titled_panel` layers a leading title row on top and returns
//! the remaining content rect below it for continued layout.

use crate::theme::{GlassMode, GlassStyle, PanelStyle};
use crate::types::{
    Backdrop, Color, CornerRadii, Point, Quad, Rect, Scene, Shadow, Size, TextStyle,
};

use super::label;

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

/// Glass panel/sheet chrome (#187): `panel`'s shadow + rounded fill + stroke,
/// but with a real backdrop blur in [`GlassMode::Blur`] (a translucent
/// `Primitive::Backdrop` sample instead of a flat fill) or an opaque
/// solid-color fallback in [`GlassMode::Solid`] for "reduce transparency" /
/// low-quality mode. The caller decides the mode — this function has no
/// notion of accessibility or quality settings, only of what to paint.
///
/// [`GlassMode::Solid`] emits exactly the primitives `panel()` would (a
/// `Shadow` plus one filled-and-stroked `Quad`) — no `Primitive::Backdrop`
/// at all — so the fallback costs exactly what a plain panel costs, by
/// construction (#187 acceptance: "no blur cost" in the fallback).
pub fn glass_panel(scene: &mut Scene, rect: Rect, style: GlassStyle, mode: GlassMode) -> Rect {
    let corner_radii = CornerRadii::all(style.corner_radius);
    scene.shadow(Shadow {
        rect,
        blur_radius: style.shadow.blur_radius,
        spread: style.shadow.spread,
        color: style.shadow.color,
        corner_radii,
    });
    match mode {
        GlassMode::Blur => {
            scene.backdrop(Backdrop {
                rect,
                corner_radii,
                blur_radius: style.blur_radius,
                tint: style.tint,
            });
            scene.quad(Quad {
                rect,
                fill: Color::TRANSPARENT,
                corner_radii,
                stroke: Some(style.stroke),
            });
        }
        GlassMode::Solid => {
            scene.quad(Quad {
                rect,
                fill: style.solid_fill,
                corner_radii,
                stroke: Some(style.stroke),
            });
        }
    }
    rect.inset(style.padding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Primitive, Size};

    #[test]
    fn titled_panel_returns_content_below_title() {
        let mut scene = Scene::new(Size::new(400.0, 400.0));
        let theme = WgpuTheme::default();
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(260.0, 200.0));
        let body = titled_panel(
            &mut scene,
            rect,
            "Mode",
            theme.panel_compact(),
            theme.title_style(),
        );
        let content = rect.inset(theme.panel_compact().padding);
        assert!(body.origin.y > content.origin.y);
        assert!(body.origin.y >= content.origin.y + theme.title_style().line_height_px);
        assert!(body.size.height < content.size.height);
        assert!(scene.primitives.iter().any(|p| match p {
            Primitive::Text(t) => t.content == "Mode",
            _ => false,
        }));
    }

    #[test]
    fn glass_panel_blur_mode_emits_a_backdrop_primitive() {
        let mut scene = Scene::new(Size::new(400.0, 400.0));
        let theme = WgpuTheme::default();
        let rect = Rect::new(Point::new(10.0, 10.0), Size::new(200.0, 120.0));
        let style = theme.glass_panel_style();
        let content = glass_panel(&mut scene, rect, style, GlassMode::Blur);

        assert_eq!(content, rect.inset(style.padding));
        assert!(scene
            .primitives
            .iter()
            .any(|p| matches!(p, Primitive::Shadow(_))));
        assert_eq!(
            scene
                .primitives
                .iter()
                .filter(|p| matches!(p, Primitive::Backdrop(_)))
                .count(),
            1,
            "blur mode must emit exactly one Backdrop primitive"
        );
        // The stroke still paints on top, but with no fill of its own — the
        // Backdrop primitive carries the tint instead.
        assert!(scene.primitives.iter().any(|p| matches!(
            p,
            Primitive::Quad(q) if q.fill == Color::TRANSPARENT && q.stroke.is_some()
        )));
    }

    #[test]
    fn glass_panel_solid_mode_emits_no_backdrop_primitive() {
        // #187 acceptance: the "reduce transparency" / low-quality fallback
        // must cost exactly what a plain `panel()` costs — provable here,
        // with no GPU, by the scene containing zero `Backdrop` primitives.
        let mut scene = Scene::new(Size::new(400.0, 400.0));
        let theme = WgpuTheme::default();
        let rect = Rect::new(Point::new(10.0, 10.0), Size::new(200.0, 120.0));
        let style = theme.glass_panel_style();
        glass_panel(&mut scene, rect, style, GlassMode::Solid);

        assert!(scene
            .primitives
            .iter()
            .all(|p| !matches!(p, Primitive::Backdrop(_))));
        assert!(scene.primitives.iter().any(|p| matches!(
            p,
            Primitive::Quad(q) if q.fill == style.solid_fill && q.stroke.is_some()
        )));
    }

    #[test]
    fn glass_panel_solid_mode_matches_plain_panel_shape() {
        // Same primitive shape (Shadow, then one filled+stroked Quad) as
        // `panel()` — the fallback is not a cosmetically distinct "glass but
        // opaque" look, it's the existing panel chrome, unchanged.
        let mut scene_glass = Scene::new(Size::new(400.0, 400.0));
        let mut scene_panel = Scene::new(Size::new(400.0, 400.0));
        let theme = WgpuTheme::default();
        let rect = Rect::new(Point::new(10.0, 10.0), Size::new(200.0, 120.0));
        let glass_style = theme.glass_panel_style();
        let panel_style = PanelStyle {
            fill: glass_style.solid_fill,
            stroke: glass_style.stroke,
            shadow: glass_style.shadow,
            corner_radius: glass_style.corner_radius,
            padding: glass_style.padding,
        };

        glass_panel(&mut scene_glass, rect, glass_style, GlassMode::Solid);
        panel(&mut scene_panel, rect, panel_style);

        assert_eq!(scene_glass.primitives.len(), scene_panel.primitives.len());
        assert_eq!(scene_glass.primitives, scene_panel.primitives);
    }
}
