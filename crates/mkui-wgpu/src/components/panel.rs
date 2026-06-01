//! `panel` / `titled_panel` — the low-level chrome primitives.
//!
//! `panel` draws shadow + rounded fill + stroke and returns the padded inner
//! content rect; it is the primitive `card` and the higher-level builders are
//! composed from. `titled_panel` layers a leading title row on top and returns
//! the remaining content rect below it for continued layout.

use crate::theme::PanelStyle;
use crate::types::{CornerRadii, Point, Quad, Rect, Scene, Shadow, Size, TextStyle};

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
}
