//! `toast_stack` / `toast_history` — the toast/banner stack render layer
//! (#193). State lives in [`crate::toast_stack::ToastStack`]; this module
//! only paints the current snapshot and reports where its interactive
//! regions landed.
//!
//! Unread count has no dedicated render helper here — [`crate::toast_stack::ToastStack::unread_count`]
//! plus [`super::badge`] already cover it (e.g.
//! `badge(scene, rect, stack.unread_count().to_string(), BadgeVariant::Destructive, BadgeSize::Sm, theme)`
//! when the count is non-zero); a wrapper would just be guessing at a host's
//! anchor point for free.

use crate::theme::WgpuTheme;
use crate::toast_stack::{ToastId, ToastStack, ToastTarget};
use crate::types::{
    CornerRadii, HitRegion, PanelLayout, Point, Quad, Rect, Scene, Shadow, Size, Stroke, Text,
    TextAlign, TextStyle,
};

use super::{label, titled_panel};

/// Full-form toast row height (message line + the dismiss/minimise
/// affordances).
const FULL_HEIGHT: f32 = 56.0;
/// Compact row height once a toast is minimised. Still tall enough for one
/// line of identifying message text (see [`toast_stack`]'s doc comment).
const MINIMISED_HEIGHT: f32 = 28.0;
/// Vertical gap between stacked rows.
const GAP: f32 = 8.0;
/// Square hit-region side for the per-item dismiss/minimise affordances.
const BUTTON_SIZE: f32 = 18.0;
/// Gap between the two per-item buttons.
const BUTTON_GAP: f32 = 6.0;

/// Render the active toasts in `stack` **newest-first** inside `region`,
/// one row per toast: a minimised toast uses a compact height, a full one a
/// taller one, separated by a fixed gap — so rows are always pairwise
/// disjoint, never overlapping, regardless of the minimised/full mix.
/// Newest-first (not [`ToastStack::active`]'s push order) matches how a
/// toast stack reads: the thing that just happened is on top.
///
/// Stops emitting — rather than spilling past `region` or overlapping —
/// once the next row would not fit. Since rendering goes newest-first,
/// running out of room drops the *oldest* active toasts, never the one that
/// just got pushed; a row that isn't drawn gets no hit region either.
/// Returns `(ToastId, Rect)` per rendered toast, newest-first — the id,
/// not just the index, because index order isn't a stable way to find a
/// toast again once overflow (or a future reorder) makes it not match
/// [`ToastStack::active`]'s order 1:1.
///
/// Pushes three [`HitRegion`]s per rendered toast into `layout`: the full
/// row body first (targeting [`ToastTarget::MarkRead`]), then the minimise
/// button, then the dismiss button — later entries win in
/// [`PanelLayout::hit`], so a click on either button takes priority over
/// the body underneath it. Mirrors [`super::chip_group::chip_group`]'s
/// `to_target` shape so a caller's existing `PanelLayout<T>` + pointer
/// router wires straight in. Feed a resolved hit back through
/// [`ToastStack::apply`].
///
/// Message text longer than the row's available width wraps/ellipsizes
/// through the same text-layout pipeline every other `Scene::Text` emitter
/// in this crate uses (the rect's width is passed straight through to the
/// text system) — no toast-specific truncation logic needed here.
pub fn toast_stack<T>(
    scene: &mut Scene,
    layout: &mut PanelLayout<T>,
    region: Rect,
    stack: &ToastStack,
    theme: &WgpuTheme,
    to_target: impl Fn(ToastTarget) -> T,
) -> Vec<(ToastId, Rect)> {
    let mut rects = Vec::with_capacity(stack.active().len());
    let mut y = region.origin.y;

    for toast in stack.active().iter().rev() {
        let height = if toast.minimised {
            MINIMISED_HEIGHT
        } else {
            FULL_HEIGHT
        };
        if y + height > region.height_end() {
            break;
        }

        let rect = Rect::new(
            Point::new(region.origin.x, y),
            Size::new(region.size.width, height),
        );
        let style = theme.toast_style(toast.variant);

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
            stroke: Some(Stroke {
                color: style.stroke,
                width: style.stroke_width,
            }),
        });

        let buttons_width = BUTTON_SIZE * 2.0 + BUTTON_GAP;
        let label_height = style.label_style.line_height_px;
        let message_width =
            (rect.size.width - style.padding.left - style.padding.right - buttons_width).max(0.0);
        scene.text(Text {
            rect: Rect::new(
                Point::new(
                    rect.origin.x + style.padding.left,
                    rect.origin.y + (height - label_height) * 0.5,
                ),
                Size::new(message_width, label_height),
            ),
            content: toast.message.clone(),
            style: style.label_style,
        });

        let button_style = TextStyle {
            align: TextAlign::Center,
            ..style.label_style
        };
        let minimise_rect = Rect::new(
            Point::new(
                rect.width_end() - style.padding.right - BUTTON_SIZE * 2.0 - BUTTON_GAP,
                rect.origin.y + (height - BUTTON_SIZE) * 0.5,
            ),
            Size::new(BUTTON_SIZE, BUTTON_SIZE),
        );
        let dismiss_rect = Rect::new(
            Point::new(
                rect.width_end() - style.padding.right - BUTTON_SIZE,
                rect.origin.y + (height - BUTTON_SIZE) * 0.5,
            ),
            Size::new(BUTTON_SIZE, BUTTON_SIZE),
        );
        label(
            scene,
            minimise_rect,
            if toast.minimised { "+" } else { "-" },
            button_style,
        );
        label(scene, dismiss_rect, "x", button_style);

        // Body first: a click anywhere on the row marks it read unless a
        // button drawn on top of it (pushed after, so it wins) claims the
        // point instead.
        layout.hit_regions.push(HitRegion {
            rect,
            target: to_target(ToastTarget::MarkRead(toast.id)),
        });
        layout.hit_regions.push(HitRegion {
            rect: minimise_rect,
            target: to_target(ToastTarget::ToggleMinimise(toast.id)),
        });
        layout.hit_regions.push(HitRegion {
            rect: dismiss_rect,
            target: to_target(ToastTarget::Dismiss(toast.id)),
        });

        rects.push((toast.id, rect));
        y += height + GAP;
    }

    rects
}

/// Render the on-demand dismissed-items list named in #193's acceptance
/// criteria ("history list ... on demand"). Call this only when
/// [`ToastStack::history_visible`] is true — the function itself doesn't
/// check it, so a caller's own toggle (a button, an Escape key, …) decides
/// cheaply whether the list exists in the scene at all this frame. Reuses
/// [`super::titled_panel`] + [`super::label`], the same shape as
/// [`super::info_list::info_list`]; [`ToastStack::history`] is already
/// most-recent-first, and entries past `rect`'s height are clipped (not
/// drawn) rather than overflowing it.
pub fn toast_history(scene: &mut Scene, rect: Rect, stack: &ToastStack, theme: &WgpuTheme) -> Rect {
    let content = titled_panel(
        scene,
        rect,
        "History",
        theme.panel_compact(),
        theme.title_style(),
    );
    let body_style = theme.body_style();
    let mut y = content.origin.y;
    for toast in stack.history() {
        if y + body_style.line_height_px > content.height_end() {
            break;
        }
        label(
            scene,
            Rect::new(
                Point::new(content.origin.x, y),
                Size::new(content.size.width, body_style.line_height_px),
            ),
            toast.message.as_str(),
            body_style,
        );
        y += body_style.line_height_px + 2.0;
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ToastVariant;
    use crate::types::{PanelLayout, Point, Primitive, Size};

    fn scene_and_region() -> (Scene, Rect) {
        (
            Scene::new(Size::new(360.0, 600.0)),
            Rect::new(Point::new(10.0, 10.0), Size::new(320.0, 400.0)),
        )
    }

    #[test]
    fn multiple_toasts_stack_without_overlapping() {
        let (mut scene, region) = scene_and_region();
        let theme = WgpuTheme::default();
        let mut stack = ToastStack::new();
        stack.push(ToastVariant::Info, "one");
        stack.push(ToastVariant::Warning, "two");
        stack.push(ToastVariant::Destructive, "three");

        let mut layout: PanelLayout<ToastTarget> = PanelLayout::default();
        let rects = toast_stack(&mut scene, &mut layout, region, &stack, &theme, |t| t);

        assert_eq!(rects.len(), 3);
        for (_, rect) in &rects {
            assert!(rect.origin.y >= region.origin.y);
            assert!(rect.height_end() <= region.height_end());
        }
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let (_, a) = rects[i];
                let (_, b) = rects[j];
                let disjoint = a.height_end() <= b.origin.y || b.height_end() <= a.origin.y;
                assert!(disjoint, "rows {i} and {j} overlap: {a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn minimised_and_full_rows_use_different_heights_and_stay_disjoint() {
        let (mut scene, region) = scene_and_region();
        let theme = WgpuTheme::default();
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "full");
        stack.push(ToastVariant::Info, "also full");
        stack.minimise(a);

        let mut layout: PanelLayout<ToastTarget> = PanelLayout::default();
        let rects = toast_stack(&mut scene, &mut layout, region, &stack, &theme, |t| t);

        let minimised_rect = rects.iter().find(|(id, _)| *id == a).unwrap().1;
        let full_rect = rects.iter().find(|(id, _)| *id != a).unwrap().1;
        assert_eq!(minimised_rect.size.height, MINIMISED_HEIGHT);
        assert_eq!(full_rect.size.height, FULL_HEIGHT);
        let disjoint = minimised_rect.height_end() <= full_rect.origin.y
            || full_rect.height_end() <= minimised_rect.origin.y;
        assert!(disjoint);
    }

    #[test]
    fn overflow_drops_the_oldest_toast_not_the_newest() {
        let mut scene = Scene::new(Size::new(200.0, 200.0));
        // Only room for one full-height row.
        let region = Rect::new(Point::new(0.0, 0.0), Size::new(180.0, FULL_HEIGHT + 4.0));
        let theme = WgpuTheme::default();
        let mut stack = ToastStack::new();
        stack.push(ToastVariant::Info, "older");
        let newer = stack.push(ToastVariant::Info, "newer");

        let mut layout: PanelLayout<ToastTarget> = PanelLayout::default();
        let rects = toast_stack(&mut scene, &mut layout, region, &stack, &theme, |t| t);

        assert_eq!(rects.len(), 1, "only one row has room");
        assert_eq!(
            rects[0].0, newer,
            "the most recently pushed toast must still render when the stack overflows"
        );
        assert!(rects[0].1.height_end() <= region.height_end());
        // The toast that didn't fit gets no hit region either (3 per row:
        // body + minimise + dismiss).
        assert_eq!(layout.hit_regions.len(), 3);
        assert!(
            layout
                .hit_regions
                .iter()
                .any(|r| r.target == ToastTarget::Dismiss(newer)),
            "the rendered (newest) toast must have a dismiss hit region"
        );
    }

    #[test]
    fn dismiss_and_minimise_hit_regions_round_trip_through_a_click() {
        let (mut scene, region) = scene_and_region();
        let theme = WgpuTheme::default();
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "a");

        let mut layout: PanelLayout<ToastTarget> = PanelLayout::default();
        toast_stack(&mut scene, &mut layout, region, &stack, &theme, |t| t);

        let dismiss_hit = layout
            .hit_regions
            .iter()
            .find(|r| r.target == ToastTarget::Dismiss(a))
            .expect("a dismiss hit region for the toast");
        let click_point = Point::new(
            dismiss_hit.rect.origin.x + 1.0,
            dismiss_hit.rect.origin.y + 1.0,
        );
        let hit = layout.hit(click_point).expect("click lands on dismiss");
        assert!(stack.apply(hit.target));
        assert!(stack.active().is_empty());
        assert_eq!(stack.history().len(), 1);
    }

    #[test]
    fn a_click_on_the_row_body_marks_it_read_without_dismissing_or_minimising() {
        let (mut scene, region) = scene_and_region();
        let theme = WgpuTheme::default();
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "a");
        assert_eq!(stack.unread_count(), 1);

        let mut layout: PanelLayout<ToastTarget> = PanelLayout::default();
        let rects = toast_stack(&mut scene, &mut layout, region, &stack, &theme, |t| t);
        let (_, rect) = rects[0];

        // A point inside the row but away from either button.
        let click_point = Point::new(rect.origin.x + 4.0, rect.origin.y + 4.0);
        let hit = layout.hit(click_point).expect("click lands on the row");
        assert_eq!(hit.target, ToastTarget::MarkRead(a));
        assert!(stack.apply(hit.target));

        assert_eq!(stack.unread_count(), 0);
        assert!(!stack.active()[0].minimised);
        assert_eq!(stack.history().len(), 0);
    }

    #[test]
    fn minimised_toast_still_shows_identifying_message_text() {
        let (mut scene, region) = scene_and_region();
        let theme = WgpuTheme::default();
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "shh");
        stack.minimise(a);

        let mut layout: PanelLayout<ToastTarget> = PanelLayout::default();
        toast_stack(&mut scene, &mut layout, region, &stack, &theme, |t| t);

        assert!(
            scene.primitives.iter().any(|p| matches!(
                p,
                Primitive::Text(t) if t.content == "shh"
            )),
            "a minimised row must still identify which toast it is"
        );
    }

    #[test]
    fn history_list_renders_dismissed_messages_most_recent_first_and_skips_active_ones() {
        let mut scene = Scene::new(Size::new(360.0, 600.0));
        let theme = WgpuTheme::default();
        let mut stack = ToastStack::new();
        let older = stack.push(ToastVariant::Info, "older");
        let newer = stack.push(ToastVariant::Info, "newer");
        stack.push(ToastVariant::Info, "still active");
        stack.dismiss(older);
        stack.dismiss(newer);
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(260.0, 200.0));

        toast_history(&mut scene, rect, &stack, &theme);

        let texts: Vec<&str> = scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Text(t) => Some(t.content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["History", "newer", "older"]);
        assert!(!texts.contains(&"still active"));
    }
}
