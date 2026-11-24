//! `segmented_select` — one continuous track split into N equal segments
//! with a single sliding highlight, the shadcn/iOS "Segmented Control"
//! shape (e.g. a language picker, or any small enum choice).
//!
//! Distinct from [`chip_group`](super::chip_group): `chip_group` draws N
//! separately-chromed pill buttons with a gap between them (tuned for
//! `picker_row`'s label-above layout, and it doesn't draw into exactly the
//! rect it's given — see its module doc's `INNER_PADDING`/`CHIP_HEIGHT`/
//! `CHIP_BOTTOM_INSET`). `segmented_select` instead draws one continuous
//! pill track filling `rect` and slides a single highlight quad behind
//! whichever segment is selected. Issue #190 asks for this distinct visual,
//! so this is a sibling widget, not a `chip_group` wrapper.
//!
//! The caller supplies the outer `rect`; the widget draws everything and
//! pushes one [`HitRegion`] per segment into `layout`, keyed by
//! `to_target(index)`.
//!
//! Keyboard/gamepad operability (issue #190) is delivered by
//! [`mkui_core::headless::SegmentedSelect`] — its `handle_key_down` already
//! moves `selected` on the arrow keys / Home / End, and is the hook a
//! future gamepad-button mapping (#203) would call into. This widget only
//! renders whatever `selected` the caller currently holds each frame.

use crate::theme::SegmentedStyle;
use crate::types::{
    CornerRadii, HitRegion, PanelLayout, Point, Quad, Rect, Scene, Size, Stroke, TextAlign,
    TextStyle,
};

use super::label;

/// Draw a segmented select into `rect` and push one `HitRegion` per option
/// into `layout`. Returns the emitted per-segment rects (index-aligned with
/// `labels`) so the caller can wire their own layout / test assertions.
pub fn segmented_select<T>(
    scene: &mut Scene,
    layout: &mut PanelLayout<T>,
    rect: Rect,
    labels: &[&str],
    selected: usize,
    style: &SegmentedStyle,
    to_target: impl Fn(usize) -> T,
) -> Vec<Rect> {
    if labels.is_empty() {
        return Vec::new();
    }

    scene.quad(Quad {
        rect,
        fill: style.track_fill,
        corner_radii: CornerRadii::all(style.track_corner_radius),
        stroke: Some(Stroke {
            color: style.track_stroke,
            width: style.track_stroke_width,
        }),
    });

    let count = labels.len();
    let segment_width = rect.size.width / count as f32;

    if selected < count {
        let inset = style.highlight_inset;
        let highlight_rect = Rect::new(
            Point::new(
                rect.origin.x + segment_width * selected as f32 + inset,
                rect.origin.y + inset,
            ),
            Size::new(
                (segment_width - inset * 2.0).max(0.0),
                (rect.size.height - inset * 2.0).max(0.0),
            ),
        );
        scene.quad(Quad {
            rect: highlight_rect,
            fill: style.highlight_fill,
            corner_radii: CornerRadii::all(style.segment_corner_radius),
            stroke: None,
        });
    }

    let label_height = style.label_style.line_height_px;
    let mut rects = Vec::with_capacity(count);
    for (index, label_text) in labels.iter().enumerate() {
        let segment_rect = Rect::new(
            Point::new(rect.origin.x + segment_width * index as f32, rect.origin.y),
            Size::new(segment_width, rect.size.height),
        );
        let label_color = if index == selected {
            style.label_active
        } else {
            style.label_idle
        };
        let label_rect = Rect::new(
            Point::new(
                segment_rect.origin.x,
                segment_rect.origin.y + (segment_rect.size.height - label_height) * 0.5,
            ),
            Size::new(segment_rect.size.width, label_height),
        );
        label(
            scene,
            label_rect,
            *label_text,
            TextStyle {
                align: TextAlign::Center,
                color: label_color,
                ..style.label_style
            },
        );
        layout.hit_regions.push(HitRegion {
            rect: segment_rect,
            target: to_target(index),
        });
        rects.push(segment_rect);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Primitive, Size};

    fn row() -> Rect {
        Rect::new(Point::new(10.0, 10.0), Size::new(240.0, 32.0))
    }

    #[test]
    fn segmented_select_pushes_one_hit_region_per_option() {
        let mut scene = Scene::new(Size::new(400.0, 200.0));
        let theme = WgpuTheme::default();
        let mut layout: PanelLayout<usize> = PanelLayout::default();
        let rects = segmented_select(
            &mut scene,
            &mut layout,
            row(),
            &["EN", "FR", "DE"],
            1,
            &theme.segmented_select(),
            |i| i,
        );
        assert_eq!(rects.len(), 3);
        assert_eq!(layout.hit_regions.len(), 3);
        assert_eq!(layout.hit_regions[1].target, 1);
    }

    #[test]
    fn empty_labels_draws_and_pushes_nothing() {
        let mut scene = Scene::new(Size::new(400.0, 200.0));
        let theme = WgpuTheme::default();
        let mut layout: PanelLayout<usize> = PanelLayout::default();
        let rects = segmented_select(
            &mut scene,
            &mut layout,
            row(),
            &[],
            0,
            &theme.segmented_select(),
            |i| i,
        );
        assert!(rects.is_empty());
        assert!(layout.hit_regions.is_empty());
    }

    fn highlight_origin_x(scene: &Scene, style: &SegmentedStyle) -> f32 {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) if q.fill == style.highlight_fill => Some(q.rect.origin.x),
                _ => None,
            })
            .next()
            .expect("highlight quad present")
    }

    #[test]
    fn highlight_slides_to_the_selected_segment() {
        let theme = WgpuTheme::default();
        let style = theme.segmented_select();

        let mut first_scene = Scene::new(Size::new(400.0, 200.0));
        let mut first_layout: PanelLayout<usize> = PanelLayout::default();
        segmented_select(
            &mut first_scene,
            &mut first_layout,
            row(),
            &["A", "B", "C"],
            0,
            &style,
            |i| i,
        );

        let mut last_scene = Scene::new(Size::new(400.0, 200.0));
        let mut last_layout: PanelLayout<usize> = PanelLayout::default();
        segmented_select(
            &mut last_scene,
            &mut last_layout,
            row(),
            &["A", "B", "C"],
            2,
            &style,
            |i| i,
        );

        assert!(highlight_origin_x(&last_scene, &style) > highlight_origin_x(&first_scene, &style));
    }

    /// Proves the keyboard/gamepad-operability acceptance criterion (#190)
    /// end to end: arrow-key presses drive the headless `SegmentedSelect`'s
    /// `selected` index, and this render function reflects that new
    /// selection without any logic of its own.
    #[test]
    fn headless_segmented_select_arrow_keys_drive_the_highlighted_segment() {
        use mkui_core::headless::{
            KeyboardInteractable, SegmentedSelect as HeadlessSegmentedSelect,
        };

        let mut headless = HeadlessSegmentedSelect::builder().option_count(3).build();
        headless.handle_key_down("ArrowRight");
        headless.handle_key_down("ArrowRight");
        assert_eq!(headless.selected(), 2);

        let theme = WgpuTheme::default();
        let style = theme.segmented_select();
        let mut scene = Scene::new(Size::new(400.0, 200.0));
        let mut layout: PanelLayout<usize> = PanelLayout::default();
        segmented_select(
            &mut scene,
            &mut layout,
            row(),
            &["A", "B", "C"],
            headless.selected(),
            &style,
            |i| i,
        );

        let segment_width = row().size.width / 3.0;
        let expected_x = row().origin.x + segment_width * 2.0 + style.highlight_inset;
        assert!((highlight_origin_x(&scene, &style) - expected_x).abs() < 0.5);
    }
}
