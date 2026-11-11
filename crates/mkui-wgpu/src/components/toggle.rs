//! `toggle` — pill track + sliding thumb, two-state on/off switch.
//!
//! Draws a rounded pill track and a circular thumb that sits at the track's
//! left edge when off and its right edge when on. The caller supplies the
//! outer `rect`; the widget draws everything and returns a [`ToggleRegions`]
//! so the caller can hit-test (the expanded `hit_rect`) and assert on the
//! thumb position in tests.
//!
//! Keyboard/gamepad operability (issue #190) is delivered by
//! [`mkui_core::headless::Toggle`] — its `handle_key_down` already flips
//! `checked` on `" "` / `"Enter"`, and is the hook a future gamepad-button
//! mapping (#203) would call into. This widget only renders whatever
//! `checked` the caller currently holds each frame; like every other widget
//! in this module, it owns no keyboard logic itself (the renderer is
//! immediate-mode and keeps no component tree between frames).

use crate::theme::ToggleStyle;
use crate::types::{CornerRadii, Point, Quad, Rect, Scene, Size, Stroke};

/// Return value of [`toggle`]. `hit_rect` is the rect the caller should push
/// into their `PanelLayout<T>` (expanded on every side for easier clicking);
/// `thumb_rect` is useful for tests and debugging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToggleRegions {
    pub track_rect: Rect,
    pub hit_rect: Rect,
    pub thumb_rect: Rect,
}

/// Two-state on/off switch — a pill track with a thumb that sits left (off)
/// or right (on). The caller supplies the outer `rect`; the widget draws
/// everything and returns the regions to hit-test.
pub fn toggle(scene: &mut Scene, rect: Rect, checked: bool, style: &ToggleStyle) -> ToggleRegions {
    let track_fill = if checked {
        style.on_fill
    } else {
        style.off_fill
    };
    scene.quad(Quad {
        rect,
        fill: track_fill,
        corner_radii: CornerRadii::all(rect.size.height * 0.5),
        stroke: Some(Stroke {
            color: style.track_stroke,
            width: style.track_stroke_width,
        }),
    });

    let thumb_diameter = (rect.size.height - style.thumb_inset * 2.0).max(0.0);
    let thumb_y = rect.origin.y + style.thumb_inset;
    let thumb_x = if checked {
        rect.origin.x + rect.size.width - style.thumb_inset - thumb_diameter
    } else {
        rect.origin.x + style.thumb_inset
    };
    let thumb_rect = Rect::new(
        Point::new(thumb_x, thumb_y),
        Size::new(thumb_diameter, thumb_diameter),
    );
    scene.quad(Quad {
        rect: thumb_rect,
        fill: style.thumb_fill,
        corner_radii: CornerRadii::all(thumb_diameter * 0.5),
        stroke: Some(Stroke {
            color: style.thumb_stroke,
            width: 1.0,
        }),
    });

    ToggleRegions {
        track_rect: rect,
        hit_rect: rect.expand(style.hit_padding),
        thumb_rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Primitive, Size};
    use mkui_core::headless::{HeadlessComponent, KeyboardInteractable, Toggle as HeadlessToggle};

    fn rect() -> Rect {
        Rect::new(Point::new(10.0, 10.0), Size::new(40.0, 20.0))
    }

    #[test]
    fn thumb_sits_left_when_off_and_right_when_on() {
        let theme = WgpuTheme::default();
        let style = theme.toggle();

        let mut off_scene = Scene::new(Size::new(200.0, 80.0));
        let off = toggle(&mut off_scene, rect(), false, &style);
        assert!((off.thumb_rect.origin.x - (rect().origin.x + style.thumb_inset)).abs() < 0.01);

        let mut on_scene = Scene::new(Size::new(200.0, 80.0));
        let on = toggle(&mut on_scene, rect(), true, &style);
        assert!(on.thumb_rect.origin.x > off.thumb_rect.origin.x);
    }

    #[test]
    fn track_fill_differs_between_off_and_on() {
        let theme = WgpuTheme::default();
        let style = theme.toggle();
        assert_ne!(style.off_fill, style.on_fill);

        let mut scene = Scene::new(Size::new(200.0, 80.0));
        toggle(&mut scene, rect(), true, &style);
        let on_fill = scene
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Quad(q) if q.rect == rect() => Some(q.fill),
                _ => None,
            })
            .expect("track quad present");
        assert_eq!(on_fill, style.on_fill);
    }

    #[test]
    fn hit_rect_is_expanded_on_every_side_by_hit_padding() {
        let theme = WgpuTheme::default();
        let style = theme.toggle();
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        let regions = toggle(&mut scene, rect(), false, &style);
        assert!((regions.hit_rect.origin.x - (rect().origin.x - style.hit_padding)).abs() < 0.01);
        assert!(regions.hit_rect.size.width > rect().size.width);
    }

    /// Proves the keyboard/gamepad-operability acceptance criterion (#190)
    /// end to end: a key press flips the headless `Toggle`'s state, and this
    /// render function reflects that new state without any logic of its own.
    #[test]
    fn headless_toggle_key_press_drives_the_rendered_thumb_side() {
        let mut headless = HeadlessToggle::new();
        let theme = WgpuTheme::default();
        let style = theme.toggle();

        let mut before_scene = Scene::new(Size::new(200.0, 80.0));
        let before = toggle(&mut before_scene, rect(), headless.is_checked(), &style);

        headless.handle_key_down(" ");
        assert!(headless.is_checked());

        let mut after_scene = Scene::new(Size::new(200.0, 80.0));
        let after = toggle(&mut after_scene, rect(), headless.is_checked(), &style);

        assert!(after.thumb_rect.origin.x > before.thumb_rect.origin.x);
    }
}
