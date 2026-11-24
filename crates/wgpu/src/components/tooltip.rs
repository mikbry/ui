//! Tooltip bubble atom (#192).
//!
//! A tooltip appears on hover **or** keyboard/gamepad focus of its
//! anchor — never hover-only. The hover/focus state machine lives in
//! [`crate::tooltip::TooltipState`] (a caller-owned, per-frame state like
//! [`crate::pointer::PointerState`] — see that module for why); this
//! module only positions and paints the bubble once the caller has
//! decided, via [`crate::tooltip::TooltipState::is_visible`], that it
//! should be shown.
//!
//! Sizing follows the same caller-measures contract as
//! [`crate::components::badge`]: the caller supplies `content_size` — no
//! glyph-measurement API is exposed at this layer yet (tracked by
//! #182/#233 for an exact-width case). [`tooltip_rect`] turns that size
//! plus the anchor's rect into a placed bubble rect: it flips to the
//! opposite side when the preferred placement would run off the anchor's
//! edge of the viewport, then clamps into the viewport bounds as a last
//! resort (e.g. an anchor near a corner with no room on either side).
//!
//! Paint order is z-order in this immediate-mode [`Scene`]: call [`tooltip`]
//! **last** in the frame so the bubble always draws over everything else.
//! [`tooltip`] pushes no [`crate::walker::HitTestEntry`] — the bubble is
//! display-only and must never itself be hoverable (a hoverable tooltip
//! could never close once the real cursor left the anchor).

use crate::theme::WgpuTheme;
use crate::types::{CornerRadii, Point, Quad, Rect, Scene, Size, Stroke, Text, TextStyle};

/// Which side of the anchor the bubble prefers to render on. [`tooltip_rect`]
/// flips to the opposite side when the preferred side would run off the
/// viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TooltipPlacement {
    Top,
    Bottom,
    Left,
    Right,
}

impl TooltipPlacement {
    fn flipped(self) -> Self {
        match self {
            TooltipPlacement::Top => TooltipPlacement::Bottom,
            TooltipPlacement::Bottom => TooltipPlacement::Top,
            TooltipPlacement::Left => TooltipPlacement::Right,
            TooltipPlacement::Right => TooltipPlacement::Left,
        }
    }
}

/// Default gap, in pixels, between the anchor's edge and the bubble.
pub const TOOLTIP_GAP_PX: f32 = 6.0;

/// Compute the tooltip bubble's rect for `content_size`, placed against
/// `anchor` on the given `placement` side and offset by `gap` pixels,
/// flipping to the opposite side first if the preferred side would run off
/// `viewport`, then clamping the result so the bubble never draws outside
/// `viewport`.
///
/// Pure geometry — no theme, no scene — so callers (and tests) can reuse it
/// without constructing a [`WgpuTheme`].
pub fn tooltip_rect(
    anchor: Rect,
    content_size: Size,
    placement: TooltipPlacement,
    gap: f32,
    viewport: Size,
) -> Rect {
    let placement = if fits(anchor, content_size, placement, gap, viewport) {
        placement
    } else {
        placement.flipped()
    };
    clamp_to_viewport(placed_rect(anchor, content_size, placement, gap), viewport)
}

fn fits(
    anchor: Rect,
    content_size: Size,
    placement: TooltipPlacement,
    gap: f32,
    viewport: Size,
) -> bool {
    match placement {
        TooltipPlacement::Top => anchor.origin.y - gap - content_size.height >= 0.0,
        TooltipPlacement::Bottom => {
            anchor.origin.y + anchor.size.height + gap + content_size.height <= viewport.height
        }
        TooltipPlacement::Left => anchor.origin.x - gap - content_size.width >= 0.0,
        TooltipPlacement::Right => {
            anchor.origin.x + anchor.size.width + gap + content_size.width <= viewport.width
        }
    }
}

fn placed_rect(anchor: Rect, content_size: Size, placement: TooltipPlacement, gap: f32) -> Rect {
    let origin = match placement {
        TooltipPlacement::Top => Point::new(
            anchor.origin.x + (anchor.size.width - content_size.width) * 0.5,
            anchor.origin.y - gap - content_size.height,
        ),
        TooltipPlacement::Bottom => Point::new(
            anchor.origin.x + (anchor.size.width - content_size.width) * 0.5,
            anchor.origin.y + anchor.size.height + gap,
        ),
        TooltipPlacement::Left => Point::new(
            anchor.origin.x - gap - content_size.width,
            anchor.origin.y + (anchor.size.height - content_size.height) * 0.5,
        ),
        TooltipPlacement::Right => Point::new(
            anchor.origin.x + anchor.size.width + gap,
            anchor.origin.y + (anchor.size.height - content_size.height) * 0.5,
        ),
    };
    Rect::new(origin, content_size)
}

fn clamp_to_viewport(rect: Rect, viewport: Size) -> Rect {
    let max_x = (viewport.width - rect.size.width).max(0.0);
    let max_y = (viewport.height - rect.size.height).max(0.0);
    Rect::new(
        Point::new(
            rect.origin.x.clamp(0.0, max_x),
            rect.origin.y.clamp(0.0, max_y),
        ),
        rect.size,
    )
}

/// Emit a tooltip bubble into `scene` at `rect` with `content` text.
///
/// The caller decides visibility ([`crate::tooltip::TooltipState::is_visible`])
/// and position ([`tooltip_rect`]) — this function only paints. Call it
/// last in the frame (see the module docs on paint order).
pub fn tooltip(scene: &mut Scene, rect: Rect, content: impl Into<String>, theme: &WgpuTheme) {
    let style = theme.tooltip_style();

    scene.quad(Quad {
        rect,
        fill: style.fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.stroke,
            width: style.stroke_width,
        }),
    });

    let label_style = TextStyle {
        color: style.label_color,
        ..style.label_style
    };
    let label_height = label_style.line_height_px;
    let label_rect = Rect::new(
        Point::new(
            rect.origin.x + style.padding.left,
            rect.origin.y + (rect.size.height - label_height) * 0.5,
        ),
        Size::new(
            (rect.size.width - style.padding.left - style.padding.right).max(0.0),
            label_height,
        ),
    );
    scene.text(Text {
        rect: label_rect,
        content: content.into(),
        style: label_style,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Primitive;

    fn scene() -> Scene {
        Scene::new(Size::new(400.0, 300.0))
    }

    #[test]
    fn emits_one_quad_and_one_text() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        let rect = Rect::new(Point::new(10.0, 10.0), Size::new(80.0, 24.0));
        tooltip(&mut s, rect, "Save", &theme);

        let quads = s
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .count();
        let texts = s
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Text(_)))
            .count();
        assert_eq!(quads, 1);
        assert_eq!(texts, 1);
    }

    #[test]
    fn text_content_is_preserved() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(80.0, 24.0));
        tooltip(&mut s, rect, "Undo", &theme);
        let text = s
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Text(t) => Some(t),
                _ => None,
            })
            .expect("a text primitive");
        assert_eq!(text.content, "Undo");
    }

    #[test]
    fn top_placement_sits_above_and_centered_on_anchor() {
        let anchor = Rect::new(Point::new(100.0, 100.0), Size::new(40.0, 20.0));
        let content = Size::new(60.0, 24.0);
        let viewport = Size::new(400.0, 300.0);
        let rect = tooltip_rect(anchor, content, TooltipPlacement::Top, 6.0, viewport);
        assert_eq!(rect.origin.y, 100.0 - 6.0 - 24.0);
        assert_eq!(rect.origin.x, 100.0 + (40.0 - 60.0) * 0.5);
    }

    #[test]
    fn flips_to_bottom_when_top_would_run_off_viewport() {
        let anchor = Rect::new(Point::new(100.0, 2.0), Size::new(40.0, 20.0));
        let content = Size::new(60.0, 24.0);
        let viewport = Size::new(400.0, 300.0);
        let rect = tooltip_rect(anchor, content, TooltipPlacement::Top, 6.0, viewport);
        assert_eq!(
            rect.origin.y,
            2.0 + 20.0 + 6.0,
            "must flip below the anchor"
        );
    }

    #[test]
    fn flips_to_right_when_left_would_run_off_viewport() {
        let anchor = Rect::new(Point::new(2.0, 100.0), Size::new(20.0, 20.0));
        let content = Size::new(60.0, 24.0);
        let viewport = Size::new(400.0, 300.0);
        let rect = tooltip_rect(anchor, content, TooltipPlacement::Left, 6.0, viewport);
        assert_eq!(
            rect.origin.x,
            2.0 + 20.0 + 6.0,
            "must flip to the right of the anchor"
        );
    }

    #[test]
    fn clamps_into_viewport_when_even_the_flipped_side_does_not_fit() {
        // The viewport is too short for the bubble on either side of the
        // anchor — the bubble must still stay on-screen (pinned to the
        // top) rather than running off the flipped side too.
        let anchor = Rect::new(Point::new(10.0, 0.0), Size::new(40.0, 10.0));
        let content = Size::new(60.0, 500.0);
        let viewport = Size::new(400.0, 300.0);
        let rect = tooltip_rect(anchor, content, TooltipPlacement::Top, 6.0, viewport);
        assert_eq!(rect.origin.y, 0.0);
    }

    #[test]
    fn clamps_horizontally_when_centered_bubble_would_overhang_the_edge() {
        // Anchor near the left edge; a wide, centered bubble would start
        // at a negative x — must clamp to 0 instead of running off-screen.
        let anchor = Rect::new(Point::new(5.0, 100.0), Size::new(20.0, 20.0));
        let content = Size::new(100.0, 24.0);
        let viewport = Size::new(400.0, 300.0);
        let rect = tooltip_rect(anchor, content, TooltipPlacement::Top, 6.0, viewport);
        assert!(rect.origin.x >= 0.0);
    }
}
