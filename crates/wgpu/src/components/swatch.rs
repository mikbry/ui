//! `swatch` — small rounded colour sample.
//!
//! Used in list rows and material / family UI to preview a concrete colour.
//! The caller supplies the `fill`; chrome (corner radius + stroke) resolves
//! through [`SwatchStyle`].

use crate::theme::SwatchStyle;
use crate::types::{Color, CornerRadii, Quad, Rect, Scene, Stroke};

/// Small rounded color sample used in list rows and material/family UI.
pub fn swatch(scene: &mut Scene, rect: Rect, fill: Color, style: SwatchStyle) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Point, Primitive, Size};

    #[test]
    fn swatch_draws_colored_quad() {
        let mut scene = Scene::new(Size::new(80.0, 80.0));
        let theme = WgpuTheme::default();
        let fill = Color::rgba(0.3, 0.4, 0.5, 1.0);
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
}
