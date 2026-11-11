//! `scroll_area` — ties a caller-owned [`ScrollState`](crate::scroll::ScrollState)'s
//! offset/extent to the existing [`scrollbar`](super::scrollbar) draw
//! function (#194, C9 of #173).
//!
//! Like [`super::slider::slider`], this computes the track/thumb geometry
//! itself from the state and the caller-supplied viewport rect, draws
//! through the existing [`scrollbar`](super::scrollbar) primitive (left
//! untouched — its signature and the one rendering contract it has, "caller
//! precomputes both rects", stay exactly as #194 found them), and hands back
//! a [`ScrollAreaRegions`] so the caller can hit-test wheel/drag input
//! against the same rects that were drawn.
//!
//! This function only draws the vertical scrollbar chrome; it does not clip
//! or draw the scrolled content itself — that's the caller's layout/content
//! concern (whatever components are being scrolled), same division of
//! responsibility `text_field_editable` has with the text it's given.

use crate::scroll::ScrollState;
use crate::theme::ScrollbarStyle;
use crate::types::{Point, Rect, Scene, Size};

use super::scrollbar;

/// Hit-testable regions returned by [`scroll_area`], modeled on
/// [`super::slider::SliderRegions`]. `track_rect` additionally serves as the
/// wheel-hit region some callers may want the whole `viewport_rect` for
/// instead (wheel scrolling conventionally fires anywhere over the content,
/// not just over the thin scrollbar strip).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollAreaRegions {
    pub viewport_rect: Rect,
    pub track_rect: Rect,
    pub thumb_rect: Rect,
}

/// Draw the scrollbar for a scroll area occupying `viewport_rect`, with the
/// track as a `track_width`-wide strip along the viewport's right edge.
/// `state` supplies the current offset/extent (read-only here — the caller
/// drives it from wheel/drag/keyboard/gamepad input, then calls this once
/// per frame to draw).
pub fn scroll_area(
    scene: &mut Scene,
    viewport_rect: Rect,
    track_width: f32,
    state: &ScrollState,
    style: &ScrollbarStyle,
) -> ScrollAreaRegions {
    let track_rect = Rect::new(
        Point::new(
            viewport_rect.width_end() - track_width,
            viewport_rect.origin.y,
        ),
        Size::new(track_width, viewport_rect.size.height),
    );
    let thumb_len = state.thumb_len(track_rect.size.height, style.min_thumb_len);
    let thumb_pos = state.thumb_pos(track_rect.size.height, thumb_len);
    let thumb_rect = Rect::new(
        Point::new(track_rect.origin.x, track_rect.origin.y + thumb_pos),
        Size::new(track_width, thumb_len),
    );

    scrollbar(scene, track_rect, thumb_rect, style);

    ScrollAreaRegions {
        viewport_rect,
        track_rect,
        thumb_rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Primitive, Size as TypesSize};

    fn viewport() -> Rect {
        Rect::new(Point::new(0.0, 0.0), Size::new(300.0, 400.0))
    }

    #[test]
    fn scroll_area_emits_track_and_thumb_quads() {
        let mut scene = Scene::new(TypesSize::new(300.0, 400.0));
        let theme = WgpuTheme::default();
        let state = ScrollState::new(1200.0, 400.0);
        scroll_area(&mut scene, viewport(), 8.0, &state, &theme.scrollbar());

        let quads = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .count();
        assert_eq!(quads, 2);
    }

    #[test]
    fn scroll_area_track_sits_at_the_viewport_right_edge() {
        let mut scene = Scene::new(TypesSize::new(300.0, 400.0));
        let theme = WgpuTheme::default();
        let state = ScrollState::new(1200.0, 400.0);
        let regions = scroll_area(&mut scene, viewport(), 8.0, &state, &theme.scrollbar());

        assert_eq!(regions.track_rect.origin.x, 300.0 - 8.0);
        assert_eq!(regions.track_rect.size.width, 8.0);
        assert_eq!(regions.track_rect.size.height, 400.0);
    }

    #[test]
    fn scroll_area_thumb_grows_to_cover_most_of_the_track_when_content_nearly_fits() {
        let mut scene = Scene::new(TypesSize::new(300.0, 400.0));
        let theme = WgpuTheme::default();
        // Content barely exceeds the viewport: thumb should be most of the
        // track, not clamped down to the minimum.
        let state = ScrollState::new(420.0, 400.0);
        let regions = scroll_area(&mut scene, viewport(), 8.0, &state, &theme.scrollbar());

        assert!(regions.thumb_rect.size.height > regions.track_rect.size.height * 0.9);
    }

    #[test]
    fn scroll_area_thumb_moves_down_as_offset_increases() {
        let mut scene = Scene::new(TypesSize::new(300.0, 400.0));
        let theme = WgpuTheme::default();

        let mut state = ScrollState::new(1200.0, 400.0);
        let regions_top = scroll_area(&mut scene, viewport(), 8.0, &state, &theme.scrollbar());
        assert_eq!(regions_top.thumb_rect.origin.y, 0.0);

        state.scroll_to_end();
        let mut scene2 = Scene::new(TypesSize::new(300.0, 400.0));
        let regions_bottom = scroll_area(&mut scene2, viewport(), 8.0, &state, &theme.scrollbar());
        assert_eq!(
            regions_bottom.thumb_rect.origin.y,
            regions_bottom.track_rect.height_end() - regions_bottom.thumb_rect.size.height
        );
    }

    #[test]
    fn scroll_area_on_unscrollable_content_fills_the_entire_track() {
        let mut scene = Scene::new(TypesSize::new(300.0, 400.0));
        let theme = WgpuTheme::default();
        let state = ScrollState::new(100.0, 400.0); // content fits entirely
        let regions = scroll_area(&mut scene, viewport(), 8.0, &state, &theme.scrollbar());

        assert_eq!(
            regions.thumb_rect.size.height,
            regions.track_rect.size.height
        );
        assert_eq!(regions.thumb_rect.origin.y, regions.track_rect.origin.y);
    }
}
