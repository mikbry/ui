//! `slider` — horizontal track + filled portion + circular thumb.
//!
//! `fraction` is clamped to `[0.0, 1.0]`. The caller supplies the outer
//! `track_rect`; the widget draws everything and returns a [`SliderRegions`]
//! so the caller can hit-test (the vertically expanded `hit_rect`) and assert
//! on the thumb position in tests.
//!
//! [`slider_with_label`] extends this with an optional value label and is
//! the entry point for issue #190's "shows its current value as a label
//! while dragging". The stepped/keyboard-adjustable half of #190 is
//! [`mkui_core::headless::Slider`] — logic-only and identical across every
//! backend; this module only ever renders the `fraction` the caller hands
//! it each frame.

use crate::theme::SliderStyle;
use crate::types::{
    CornerRadii, Point, Quad, Rect, Scene, Size, Stroke, Text, TextAlign, TextStyle,
};

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

/// Same as [`slider`], plus an optional value label centered above the
/// thumb (issue #190's "shows its current value as a label while
/// dragging"). `value_label` is caller-formatted (e.g. `"50%"`, `"1.20 m"`)
/// — this widget stays unit-agnostic, matching every other label-taking
/// widget in this module (`text_field`, `label`, …).
///
/// `slider` itself is unchanged for existing call sites (e.g.
/// [`crate::builder::UiBuilder::number_row`]); pass `None` here to recover
/// identical output to calling [`slider`] directly. The caller decides when
/// to pass `Some` — typically while the drag that moves this slider is in
/// progress — since this immediate-mode widget has no notion of time or
/// input state of its own.
pub fn slider_with_label(
    scene: &mut Scene,
    track_rect: Rect,
    fraction: f32,
    value_label: Option<&str>,
    style: &SliderStyle,
) -> SliderRegions {
    let regions = slider(scene, track_rect, fraction, style);

    if let Some(text) = value_label {
        let label_style = style.value_label_style;
        let label_height = label_style.line_height_px;
        // Approximate width, consistent with `text_field`'s per-char-count
        // approximation pending real glyph measurement (#182).
        let label_width = (text.chars().count() as f32 * label_style.font_size_px * 0.6).max(24.0);
        let thumb_center_x = regions.thumb_rect.origin.x + regions.thumb_rect.size.width * 0.5;
        let label_rect = Rect::new(
            Point::new(
                thumb_center_x - label_width * 0.5,
                regions.thumb_rect.origin.y - label_height - style.value_label_gap,
            ),
            Size::new(label_width, label_height),
        );
        scene.text(Text {
            rect: label_rect,
            content: text.to_string(),
            style: TextStyle {
                align: TextAlign::Center,
                ..label_style
            },
        });
    }

    regions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Primitive, Size};

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

    fn track() -> Rect {
        Rect::new(Point::new(10.0, 40.0), Size::new(200.0, 8.0))
    }

    #[test]
    fn slider_with_label_omits_text_when_none() {
        let mut scene = Scene::new(Size::new(400.0, 80.0));
        let theme = WgpuTheme::default();
        slider_with_label(&mut scene, track(), 0.5, None, &theme.slider());
        assert!(!scene
            .primitives
            .iter()
            .any(|p| matches!(p, Primitive::Text(_))));
    }

    #[test]
    fn slider_with_label_draws_value_centered_over_thumb() {
        let mut scene = Scene::new(Size::new(400.0, 80.0));
        let theme = WgpuTheme::default();
        let regions = slider_with_label(&mut scene, track(), 0.5, Some("50%"), &theme.slider());
        let label = scene
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Text(t) if t.content == "50%" => Some(t),
                _ => None,
            })
            .expect("value label text present");
        let label_center_x = label.rect.origin.x + label.rect.size.width * 0.5;
        let thumb_center_x = regions.thumb_rect.origin.x + regions.thumb_rect.size.width * 0.5;
        assert!((label_center_x - thumb_center_x).abs() < 0.5);
    }

    /// Proves the keyboard/gamepad-operability + stepped-mode acceptance
    /// criterion (#190) end to end: repeated `ArrowRight` presses drive the
    /// headless `Slider`'s fraction, and this render function reflects that
    /// new fraction without any logic of its own.
    #[test]
    fn headless_slider_arrow_key_steps_drive_the_rendered_thumb_position() {
        use mkui_core::headless::{KeyboardInteractable, Slider as HeadlessSlider};

        let mut headless = HeadlessSlider::builder()
            .min(0.0)
            .max(1.0)
            .step(0.1)
            .value(0.0)
            .build();
        for _ in 0..5 {
            headless.handle_key_down("ArrowRight");
        }
        assert!((headless.fraction() - 0.5).abs() < f32::EPSILON);

        let mut scene = Scene::new(Size::new(400.0, 80.0));
        let theme = WgpuTheme::default();
        let regions = slider(&mut scene, track(), headless.fraction(), &theme.slider());
        let thumb_center = regions.thumb_rect.origin.x + regions.thumb_rect.size.width * 0.5;
        let expected = track().origin.x + track().size.width * 0.5;
        assert!((thumb_center - expected).abs() < 0.5);
    }
}
