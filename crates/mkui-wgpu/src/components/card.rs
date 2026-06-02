//! `card` — rounded-rect surface chrome.
//!
//! The shadcn "Card" primitive (minus the typography slot): a rounded rect
//! with fill + stroke and no shadow. Use it for inline list rows and inspector
//! row backgrounds; the style resolves through [`CardStyle`].

use crate::theme::CardStyle;
use crate::types::{CornerRadii, Quad, Rect, Scene, Stroke};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Point, Primitive, Size};

    #[test]
    fn card_emits_a_single_filled_quad() {
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        let theme = WgpuTheme::default();
        card(
            &mut scene,
            Rect::new(Point::new(0.0, 0.0), Size::new(120.0, 40.0)),
            theme.card(),
        );
        let quads: Vec<_> = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .collect();
        assert_eq!(quads.len(), 1);
    }
}
