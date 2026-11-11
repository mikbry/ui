//! `progress_bar` — determinate horizontal progress indicator (#191, C6 of
//! #173).
//!
//! Shares its track-plus-fill shape with [`super::slider`] minus the thumb
//! and the hit rect — a progress bar is non-interactive, so there is
//! nothing for a caller to grab or hit-test.
//!
//! `fraction` is clamped to `[0.0, 1.0]`. The filled quad's width is exactly
//! `track_rect.size.width * fraction`, so the two edge cases the issue calls
//! out render correctly: at `0.0` no filled quad is emitted at all (an empty
//! track, not a zero-width degenerate quad), and at `1.0` the filled quad is
//! flush with the track's own rect and corner radius.

use crate::theme::ProgressBarStyle;
use crate::types::{CornerRadii, Quad, Rect, Scene, Size, Stroke};

/// Horizontal progress bar — a track with a proportionally filled portion.
/// `fraction` is clamped to `[0.0, 1.0]`; the caller supplies the outer
/// `track_rect`.
pub fn progress_bar(scene: &mut Scene, track_rect: Rect, fraction: f32, style: &ProgressBarStyle) {
    let fraction = fraction.clamp(0.0, 1.0);

    scene.quad(Quad {
        rect: track_rect,
        fill: style.track_fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.track_stroke,
            width: style.track_stroke_width,
        }),
    });

    if fraction > 0.0 {
        let filled_rect = Rect::new(
            track_rect.origin,
            Size::new(track_rect.size.width * fraction, track_rect.size.height),
        );
        scene.quad(Quad {
            rect: filled_rect,
            fill: style.filled_fill,
            corner_radii: CornerRadii::all(style.corner_radius),
            stroke: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Point, Primitive, Size};

    fn scene() -> Scene {
        Scene::new(Size::new(400.0, 80.0))
    }

    fn quads(scene: &Scene) -> Vec<&Quad> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) => Some(q),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn zero_percent_renders_track_only_no_filled_quad() {
        let theme = WgpuTheme::default();
        let style = theme.progress_bar();
        let mut s = scene();
        let track = Rect::new(Point::new(10.0, 10.0), Size::new(200.0, 8.0));
        progress_bar(&mut s, track, 0.0, &style);

        let shapes = quads(&s);
        assert_eq!(
            shapes.len(),
            1,
            "0% should emit the track quad only, no degenerate zero-width fill"
        );
        assert_eq!(shapes[0].fill, style.track_fill);
    }

    #[test]
    fn hundred_percent_fills_the_entire_track() {
        let theme = WgpuTheme::default();
        let style = theme.progress_bar();
        let mut s = scene();
        let track = Rect::new(Point::new(10.0, 10.0), Size::new(200.0, 8.0));
        progress_bar(&mut s, track, 1.0, &style);

        let shapes = quads(&s);
        assert_eq!(shapes.len(), 2, "100% should emit track + filled quad");
        let filled = shapes[1];
        assert_eq!(filled.rect.size.width, track.size.width);
        assert_eq!(filled.rect.origin, track.origin);
        assert_eq!(filled.corner_radii, CornerRadii::all(style.corner_radius));
    }

    #[test]
    fn partial_fraction_is_proportional_to_track_width() {
        let theme = WgpuTheme::default();
        let style = theme.progress_bar();
        let mut s = scene();
        let track = Rect::new(Point::new(0.0, 0.0), Size::new(200.0, 8.0));
        progress_bar(&mut s, track, 0.25, &style);

        let shapes = quads(&s);
        assert_eq!(shapes.len(), 2);
        assert!((shapes[1].rect.size.width - 50.0).abs() < 0.001);
    }

    #[test]
    fn fraction_is_clamped_outside_zero_to_one() {
        let theme = WgpuTheme::default();
        let style = theme.progress_bar();
        let track = Rect::new(Point::new(0.0, 0.0), Size::new(200.0, 8.0));

        let mut below = scene();
        progress_bar(&mut below, track, -0.5, &style);
        assert_eq!(
            quads(&below).len(),
            1,
            "a negative fraction clamps to 0% (track only)"
        );

        let mut above = scene();
        progress_bar(&mut above, track, 1.5, &style);
        let above_shapes = quads(&above);
        assert_eq!(above_shapes.len(), 2);
        assert_eq!(
            above_shapes[1].rect.size.width, track.size.width,
            "a fraction above 1.0 clamps to 100% (full width), not overflow"
        );
    }

    #[test]
    fn renders_to_non_empty_finite_triangles_at_0_50_and_100_percent() {
        // The acceptance criterion is "renders 0-100% correctly" — this
        // proves the full pipeline (not just the `Scene` primitives) by
        // tessellating through the same `tessellate_scene` path every
        // example renders with, at both edges and the midpoint.
        let theme = WgpuTheme::default();
        let style = theme.progress_bar();
        let track = Rect::new(Point::new(10.0, 10.0), Size::new(200.0, 8.0));

        for fraction in [0.0, 0.5, 1.0] {
            let mut s = scene();
            progress_bar(&mut s, track, fraction, &style);
            let triangles = crate::tessellate_scene(&s);
            assert!(
                !triangles.is_empty(),
                "fraction {fraction} must tessellate to at least the track's triangles"
            );
            for triangle in &triangles {
                for point in triangle.points {
                    assert!(
                        point.x.is_finite() && point.y.is_finite(),
                        "fraction {fraction} produced a non-finite triangle vertex"
                    );
                }
            }
        }
    }
}
