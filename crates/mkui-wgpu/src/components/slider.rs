//! `slider` — horizontal track + filled portion + circular thumb.
//!
//! `fraction` is clamped to `[0.0, 1.0]`. The caller supplies the outer
//! `track_rect`; the widget draws everything and returns a [`SliderRegions`]
//! so the caller can hit-test (the vertically expanded `hit_rect`) and assert
//! on the thumb position in tests.

use crate::theme::SliderStyle;
use crate::types::{CornerRadii, Point, Quad, Rect, Scene, Size, Stroke};

/// Return value of [`slider`]. `hit_rect` is the rect the caller should push
/// into their `PanelLayout<T>` (expanded vertically for easier grabbing);
/// `thumb_rect` is useful for tests and debugging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderRegions {
    pub track_rect: Rect,
    pub hit_rect: Rect,
    pub thumb_rect: Rect,
}

/// Horizontal slider — track with a filled portion + a circular thumb.
///
/// `fraction` is clamped to `[0.0, 1.0]`. The caller supplies the outer
/// `track_rect`; the widget draws everything and returns the regions to
/// hit-test.
pub fn slider(
    scene: &mut Scene,
    track_rect: Rect,
    fraction: f32,
    style: &SliderStyle,
) -> SliderRegions {
    let fraction = fraction.clamp(0.0, 1.0);

    scene.quad(Quad {
        rect: track_rect,
        fill: style.track_fill,
        corner_radii: CornerRadii::all(style.track_corner_radius),
        stroke: Some(Stroke {
            color: style.track_stroke,
            width: style.track_stroke_width,
        }),
    });
    let filled_rect = Rect::new(
        track_rect.origin,
        Size::new(track_rect.size.width * fraction, track_rect.size.height),
    );
    scene.quad(Quad {
        rect: filled_rect,
        fill: style.filled_fill,
        corner_radii: CornerRadii::all(style.track_corner_radius),
        stroke: None,
    });
    let thumb_center_x = track_rect.origin.x + track_rect.size.width * fraction;
    let thumb_rect = Rect::new(
        Point::new(
            thumb_center_x - style.thumb_diameter * 0.5,
            track_rect.origin.y + (track_rect.size.height - style.thumb_diameter) * 0.5,
        ),
        Size::new(style.thumb_diameter, style.thumb_diameter),
    );
    scene.quad(Quad {
        rect: thumb_rect,
        fill: style.thumb_fill,
        corner_radii: CornerRadii::all(style.thumb_diameter * 0.5),
        stroke: Some(Stroke {
            color: style.thumb_stroke,
            width: 1.0,
        }),
    });

    let hit_rect = Rect::new(
        Point::new(track_rect.origin.x, track_rect.origin.y - style.hit_padding),
        Size::new(
            track_rect.size.width,
            track_rect.size.height + style.hit_padding * 2.0,
        ),
    );

    SliderRegions {
        track_rect,
        hit_rect,
        thumb_rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::Size;

    #[test]
    fn slider_thumb_sits_proportional_to_fraction() {
        let mut scene = Scene::new(Size::new(400.0, 80.0));
        let theme = WgpuTheme::default();
        let style = theme.slider();
        let track = Rect::new(Point::new(10.0, 40.0), Size::new(200.0, 8.0));
        let regions = slider(&mut scene, track, 0.25, &style);
        let thumb_center = regions.thumb_rect.origin.x + regions.thumb_rect.size.width * 0.5;
        let expected = track.origin.x + track.size.width * 0.25;
        assert!((thumb_center - expected).abs() < 0.5);
    }
}
