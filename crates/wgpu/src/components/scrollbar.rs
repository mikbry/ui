//! `scrollbar` — vertical pill-shaped track + thumb.
//!
//! The caller precomputes both rects in the layout pass (so hit-testing
//! matches rendering) and the widget emits the two pill quads; chrome resolves
//! through [`ScrollbarStyle`].

use crate::theme::ScrollbarStyle;
use crate::types::{CornerRadii, Quad, Rect, Scene};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Point, Primitive, Size};

    #[test]
    fn scrollbar_emits_track_and_thumb_quads() {
        let mut scene = Scene::new(Size::new(80.0, 400.0));
        let theme = WgpuTheme::default();
        let track = Rect::new(Point::new(60.0, 0.0), Size::new(8.0, 400.0));
        let thumb = Rect::new(Point::new(60.0, 40.0), Size::new(8.0, 120.0));
        scrollbar(&mut scene, track, thumb, &theme.scrollbar());
        let quads: Vec<_> = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .collect();
        assert_eq!(quads.len(), 2);
    }
}
