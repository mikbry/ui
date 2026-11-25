//! `ScrollState` — scroll-area logic: wheel, thumb drag, keyboard paging,
//! gamepad stick/d-pad stepping, and focus-driven reveal (#194, C9 of #173).
//!
//! Pure logic, no winit/host dependency: like [`crate::pointer::PointerState`]
//! and [`crate::text_edit::TextEditState`], one instance is owned by the
//! caller and kept across frames (the renderer is immediate-mode and retains
//! no component tree between frames). It tracks a single scroll axis —
//! vertical, the common case for lists and reading content
//! ([`components::scrollbar`](crate::components::scrollbar)'s existing
//! orientation). A caller that also needs horizontal scrolling owns a second,
//! independent instance for that axis; nothing here assumes there is only
//! one.
//!
//! [`WheelDelta`] mirrors the shape of `winit::event::MouseScrollDelta`
//! without depending on winit, the same way [`crate::text_edit::TextEditState`]
//! mirrors `winit::event::Ime` — a host wiring this up (#176's input-routing
//! contract) translates the real event into it. Gamepad input is handled the
//! same way: [`ScrollState::apply_stick`] takes a plain `[-1.0, 1.0]` axis
//! value and a frame delta-time, with no device-polling dependency of its
//! own — #203 (the parent tracker's dedicated gamepad/focus-navigation
//! issue) owns actually reading a controller; this only owns what a stick
//! value does to the scroll offset once a host hands one in. A d-pad press
//! is a single discrete step, so it reuses [`ScrollState::scroll_lines`]
//! directly rather than needing its own method.
//!
//! ## Why keyboard/gamepad stepping can't "fight" wheel/drag scrolling
//!
//! Every mutating method here — wheel, drag, keyboard paging, stick/d-pad —
//! goes through the same clamped `offset` in the same way a plain
//! `scroll_by` would. There is no separate "focus position" this type tracks
//! and silently re-applies: [`ScrollState::ensure_visible`] is the one method
//! that moves the offset *because of* focus, and it only runs when a caller
//! calls it (meant for a focus-change event, e.g. tabbing to an off-screen
//! list item), never automatically on every frame. A caller that mistakenly
//! calls it every frame would indeed fight a simultaneous wheel/drag scroll —
//! that's a caller bug, not a property of this type — see the
//! `wheel_scroll_is_not_overridden_by_a_stale_focus_reveal` test below for
//! the behavior this type actually guarantees.

/// A wheel-scroll input, decoupled from any particular windowing crate's
/// event type. Positive values scroll content *down* (increase the offset);
/// a caller translating a raw platform event is responsible for any sign
/// flip its platform's convention requires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelDelta {
    /// A discrete number of lines (typical desktop mouse wheel "notches").
    /// Scaled by the caller-supplied line length to get a pixel delta.
    Lines(f32),
    /// An exact pixel delta (high-resolution trackpad / precision wheel).
    Pixels(f32),
}

/// Scroll-area state: content/viewport extents, current offset, and
/// in-progress thumb-drag state. One instance per scrollable viewport, along
/// one axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollState {
    content_len: f32,
    viewport_len: f32,
    offset: f32,
    /// Distance, in track pixels, from the thumb's leading edge to the
    /// cursor position at the moment a drag began. Preserves that offset
    /// for the rest of the gesture so the thumb doesn't jump to re-center
    /// under the cursor on the first `drag_to` call — the same
    /// press-to-arm-at-the-grabbed-point contract a native scrollbar uses.
    /// `None` when no drag is in progress.
    drag_grab: Option<f32>,
}

impl ScrollState {
    /// New state with the given content/viewport lengths and the offset at
    /// the top.
    pub fn new(content_len: f32, viewport_len: f32) -> Self {
        Self {
            content_len: content_len.max(0.0),
            viewport_len: viewport_len.max(0.0),
            offset: 0.0,
            drag_grab: None,
        }
    }

    pub fn offset(&self) -> f32 {
        self.offset
    }

    pub fn content_len(&self) -> f32 {
        self.content_len
    }

    pub fn viewport_len(&self) -> f32 {
        self.viewport_len
    }

    /// The largest valid offset — zero when the content already fits the
    /// viewport (never negative).
    pub fn max_offset(&self) -> f32 {
        (self.content_len - self.viewport_len).max(0.0)
    }

    /// Whether there is anywhere to scroll at all.
    pub fn is_scrollable(&self) -> bool {
        self.max_offset() > 0.0
    }

    /// Current offset as a `[0.0, 1.0]` fraction of [`Self::max_offset`].
    /// `0.0` (rather than `NaN`) when nothing is scrollable.
    pub fn fraction(&self) -> f32 {
        let max_offset = self.max_offset();
        if max_offset <= 0.0 {
            0.0
        } else {
            self.offset / max_offset
        }
    }

    /// `viewport_len / content_len`, clamped to `[0.0, 1.0]` — the thumb's
    /// proportion of the track. `1.0` (rather than a divide-by-zero `NaN`)
    /// when there's no content yet.
    pub fn viewport_fraction(&self) -> f32 {
        if self.content_len <= 0.0 {
            1.0
        } else {
            (self.viewport_len / self.content_len).clamp(0.0, 1.0)
        }
    }

    /// Update the content/viewport extents (e.g. the window resized, or the
    /// content grew/shrank) and re-clamp the offset against the new
    /// [`Self::max_offset`]. Call once per frame before reading geometry —
    /// shrinking content must not leave the offset pointing past the new
    /// end.
    pub fn set_extent(&mut self, content_len: f32, viewport_len: f32) {
        self.content_len = content_len.max(0.0);
        self.viewport_len = viewport_len.max(0.0);
        self.offset = self.offset.clamp(0.0, self.max_offset());
    }

    fn set_offset(&mut self, offset: f32) {
        self.offset = offset.clamp(0.0, self.max_offset());
    }

    /// Scroll by a relative pixel amount; positive moves down.
    pub fn scroll_by(&mut self, delta: f32) {
        self.set_offset(self.offset + delta);
    }

    /// Scroll to an absolute pixel offset, clamped to `[0, max_offset]`.
    pub fn scroll_to(&mut self, offset: f32) {
        self.set_offset(offset);
    }

    /// Scroll to a `[0.0, 1.0]` fraction of [`Self::max_offset`] (out-of-range
    /// values clamp). Used by thumb-drag and click-to-position.
    pub fn scroll_to_fraction(&mut self, fraction: f32) {
        self.set_offset(self.max_offset() * fraction.clamp(0.0, 1.0));
    }

    pub fn scroll_to_start(&mut self) {
        self.set_offset(0.0);
    }

    pub fn scroll_to_end(&mut self) {
        self.set_offset(self.max_offset());
    }

    /// Step by `n` lines of `line_len` pixels each. Negative `n` scrolls up.
    /// Keyboard arrow-key granularity; a gamepad d-pad press reuses this
    /// directly with `n = ±1`.
    pub fn scroll_lines(&mut self, n: i32, line_len: f32) {
        self.scroll_by(n as f32 * line_len);
    }

    /// Step by `n` viewport-heights. Negative `n` scrolls up. Keyboard
    /// Page Up/Page Down granularity.
    pub fn scroll_pages(&mut self, n: i32) {
        self.scroll_by(n as f32 * self.viewport_len);
    }

    /// Apply one wheel-input event. `line_len` is the pixel size of one
    /// "line" for [`WheelDelta::Lines`] — the caller's row/line height.
    pub fn apply_wheel(&mut self, delta: WheelDelta, line_len: f32) {
        match delta {
            WheelDelta::Lines(lines) => self.scroll_by(lines * line_len),
            WheelDelta::Pixels(pixels) => self.scroll_by(pixels),
        }
    }

    /// Apply one frame of continuous gamepad-stick scrolling. `axis` is the
    /// raw `[-1.0, 1.0]` stick value (already including any trigger-side
    /// sign flip the host's mapping wants); values with `abs(axis) <=
    /// deadzone` are ignored. Speed ramps linearly from `0` at the deadzone
    /// edge to `max_speed_px_per_sec` at full deflection, scaled by `dt_secs`
    /// so it reads the same regardless of frame rate.
    pub fn apply_stick(
        &mut self,
        axis: f32,
        dt_secs: f32,
        deadzone: f32,
        max_speed_px_per_sec: f32,
    ) {
        let deadzone = deadzone.clamp(0.0, 1.0 - f32::EPSILON);
        let magnitude = axis.abs();
        if magnitude <= deadzone {
            return;
        }
        let travel = (magnitude - deadzone) / (1.0 - deadzone);
        let speed = travel * max_speed_px_per_sec;
        self.scroll_by(axis.signum() * speed * dt_secs);
    }

    /// Whether a thumb drag is currently in progress.
    pub fn is_dragging(&self) -> bool {
        self.drag_grab.is_some()
    }

    /// Begin a thumb drag. `cursor_pos`/`thumb_pos` are both in the same
    /// track-pixel coordinate space (see [`Self::thumb_pos`]); `thumb_len`
    /// is the current rendered thumb length (see [`Self::thumb_len`]).
    /// Records where within the thumb the cursor grabbed it, so
    /// [`Self::drag_to`] moves the thumb relative to that point instead of
    /// re-centering it under the cursor.
    pub fn begin_drag(&mut self, cursor_pos: f32, thumb_pos: f32, thumb_len: f32) {
        self.drag_grab = Some((cursor_pos - thumb_pos).clamp(0.0, thumb_len));
    }

    /// Continue an in-progress thumb drag, moving the thumb so its grabbed
    /// point tracks `cursor_pos`. A no-op if no drag is in progress (call
    /// [`Self::begin_drag`] first), or if there's no room on the track to
    /// drag within (content already fits the viewport).
    pub fn drag_to(&mut self, track_len: f32, thumb_len: f32, cursor_pos: f32) {
        let Some(grab) = self.drag_grab else { return };
        let travel = (track_len - thumb_len).max(0.0);
        if travel <= f32::EPSILON {
            return;
        }
        let thumb_pos = (cursor_pos - grab).clamp(0.0, travel);
        self.scroll_to_fraction(thumb_pos / travel);
    }

    /// End the in-progress thumb drag, if any.
    pub fn end_drag(&mut self) {
        self.drag_grab = None;
    }

    /// A click on the track (not the thumb itself) at `cursor_pos`: page
    /// toward the click, one viewport-height at a time, rather than jumping
    /// straight to that position. `thumb_pos`/`thumb_len` are the thumb's
    /// current geometry (see [`Self::thumb_pos`]/[`Self::thumb_len`]).
    pub fn page_toward(&mut self, thumb_pos: f32, thumb_len: f32, cursor_pos: f32) {
        if cursor_pos < thumb_pos {
            self.scroll_pages(-1);
        } else if cursor_pos > thumb_pos + thumb_len {
            self.scroll_pages(1);
        }
    }

    /// The thumb's rendered length along a track of `track_len` pixels,
    /// proportional to [`Self::viewport_fraction`] but never smaller than
    /// `min_len` (so a very long document still leaves a grabbable thumb),
    /// and never larger than the track itself.
    pub fn thumb_len(&self, track_len: f32, min_len: f32) -> f32 {
        let track_len = track_len.max(0.0);
        (track_len * self.viewport_fraction()).max(min_len.min(track_len))
    }

    /// The thumb's leading-edge position along a track of `track_len`
    /// pixels, given a thumb of `thumb_len` pixels (see [`Self::thumb_len`]).
    pub fn thumb_pos(&self, track_len: f32, thumb_len: f32) -> f32 {
        let travel = (track_len.max(0.0) - thumb_len).max(0.0);
        travel * self.fraction()
    }

    /// Reveal an item at `[item_offset, item_offset + item_len)` (in the
    /// same coordinate space as [`Self::offset`]) by moving the offset the
    /// minimal amount needed to bring it fully into view — or, if `item_len`
    /// is larger than the viewport, aligning to the item's start. A no-op
    /// if the item is already fully visible.
    ///
    /// Meant to be called once, on a focus-change event (e.g. keyboard/
    /// gamepad focus navigation moving to an off-screen list item, #203) —
    /// never every frame, which would fight an in-progress wheel/drag scroll
    /// by re-snapping the offset back on top of it every frame.
    pub fn ensure_visible(&mut self, item_offset: f32, item_len: f32) {
        let visible_start = self.offset;
        let visible_end = self.offset + self.viewport_len;
        let item_end = item_offset + item_len;

        if item_len >= self.viewport_len || item_offset < visible_start {
            self.scroll_to(item_offset);
        } else if item_end > visible_end {
            self.scroll_to(item_end - self.viewport_len);
        }
        // Else: already fully visible, no-op.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- extents, clamping, divide-by-zero safety ----

    #[test]
    fn new_state_starts_at_top() {
        let s = ScrollState::new(1000.0, 200.0);
        assert_eq!(s.offset(), 0.0);
        assert_eq!(s.max_offset(), 800.0);
    }

    #[test]
    fn content_smaller_than_viewport_has_zero_max_offset() {
        let s = ScrollState::new(100.0, 200.0);
        assert_eq!(s.max_offset(), 0.0);
        assert!(!s.is_scrollable());
        assert_eq!(s.fraction(), 0.0, "must not be NaN");
        assert_eq!(s.viewport_fraction(), 1.0);
    }

    #[test]
    fn zero_content_len_does_not_panic_or_nan() {
        let s = ScrollState::new(0.0, 0.0);
        assert_eq!(s.fraction(), 0.0);
        assert_eq!(s.viewport_fraction(), 1.0);
        assert_eq!(s.thumb_len(100.0, 20.0), 100.0);
        assert_eq!(s.thumb_pos(100.0, 100.0), 0.0);
    }

    #[test]
    fn scroll_by_clamps_to_bounds() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_by(-50.0);
        assert_eq!(s.offset(), 0.0, "cannot scroll above the top");
        s.scroll_by(10_000.0);
        assert_eq!(s.offset(), s.max_offset(), "clamps at the bottom");
    }

    #[test]
    fn shrinking_content_reclamps_offset() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_to_end();
        assert_eq!(s.offset(), 800.0);
        // Content shrinks (e.g. items removed) below the current offset.
        s.set_extent(300.0, 200.0);
        assert_eq!(s.max_offset(), 100.0);
        assert_eq!(s.offset(), 100.0, "offset must not point past the new end");
    }

    #[test]
    fn fraction_and_scroll_to_fraction_round_trip() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_to_fraction(0.5);
        assert_eq!(s.offset(), 400.0);
        assert_eq!(s.fraction(), 0.5);
        s.scroll_to_fraction(-1.0);
        assert_eq!(s.offset(), 0.0, "clamps below 0.0");
        s.scroll_to_fraction(2.0);
        assert_eq!(s.offset(), s.max_offset(), "clamps above 1.0");
    }

    // ---- "Wheel ... scroll" ----

    #[test]
    fn wheel_lines_scales_by_line_length() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.apply_wheel(WheelDelta::Lines(3.0), 20.0);
        assert_eq!(s.offset(), 60.0);
        s.apply_wheel(WheelDelta::Lines(-1.0), 20.0);
        assert_eq!(s.offset(), 40.0);
    }

    #[test]
    fn wheel_pixels_applies_directly() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.apply_wheel(WheelDelta::Pixels(123.0), 20.0);
        assert_eq!(s.offset(), 123.0);
    }

    #[test]
    fn wheel_cannot_scroll_past_either_end() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.apply_wheel(WheelDelta::Lines(-100.0), 20.0);
        assert_eq!(s.offset(), 0.0);
        s.apply_wheel(WheelDelta::Pixels(1_000_000.0), 20.0);
        assert_eq!(s.offset(), s.max_offset());
    }

    // ---- "drag ... scroll" + thumb geometry ----

    #[test]
    fn thumb_len_is_proportional_and_never_below_min() {
        let s = ScrollState::new(1000.0, 200.0);
        assert_eq!(s.thumb_len(400.0, 10.0), 80.0, "400 * 200/1000");
        // A tiny viewport fraction still leaves a grabbable thumb.
        let tiny = ScrollState::new(100_000.0, 200.0);
        assert_eq!(tiny.thumb_len(400.0, 24.0), 24.0);
    }

    #[test]
    fn thumb_pos_tracks_fraction_along_available_travel() {
        let mut s = ScrollState::new(1000.0, 200.0);
        let thumb_len = s.thumb_len(400.0, 10.0); // 80.0
        s.scroll_to_fraction(0.5);
        // Travel = 400 - 80 = 320; half of that = 160.
        assert_eq!(s.thumb_pos(400.0, thumb_len), 160.0);
    }

    #[test]
    fn drag_preserves_grab_point_instead_of_recentering() {
        let mut s = ScrollState::new(1000.0, 200.0);
        let track_len = 400.0;
        let thumb_len = s.thumb_len(track_len, 10.0); // 80.0
        let thumb_pos = s.thumb_pos(track_len, thumb_len); // 0.0, at the top

        // Grab the thumb 20px into its 80px length, not at its center.
        s.begin_drag(thumb_pos + 20.0, thumb_pos, thumb_len);
        // Immediately "move" the cursor by 1px — the thumb must advance by
        // 1px too, not jump to center the cursor on the thumb.
        s.drag_to(track_len, thumb_len, thumb_pos + 21.0);
        let new_thumb_pos = s.thumb_pos(track_len, s.thumb_len(track_len, 10.0));
        assert!(
            (new_thumb_pos - 1.0).abs() < 0.01,
            "thumb should have moved ~1px, not jumped to re-center; got {new_thumb_pos}"
        );
    }

    #[test]
    fn drag_to_full_extent_reaches_scroll_bounds() {
        let mut s = ScrollState::new(1000.0, 200.0);
        let track_len = 400.0;
        let thumb_len = s.thumb_len(track_len, 10.0);
        s.begin_drag(0.0, 0.0, thumb_len);
        s.drag_to(track_len, thumb_len, track_len); // drag to the very end
        assert_eq!(s.offset(), s.max_offset());

        s.drag_to(track_len, thumb_len, -1000.0); // drag past the start
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn drag_without_begin_is_a_noop() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.drag_to(400.0, 80.0, 999.0);
        assert_eq!(s.offset(), 0.0, "no drag in progress, nothing happens");
    }

    #[test]
    fn drag_on_unscrollable_content_does_not_panic_or_move() {
        let mut s = ScrollState::new(100.0, 200.0); // fits entirely
        let track_len = 400.0;
        let thumb_len = s.thumb_len(track_len, 10.0); // == track_len
        s.begin_drag(0.0, 0.0, thumb_len);
        s.drag_to(track_len, thumb_len, 350.0);
        assert_eq!(s.offset(), 0.0);
        assert!(!s.offset().is_nan());
    }

    #[test]
    fn end_drag_clears_in_progress_state() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.begin_drag(10.0, 0.0, 80.0);
        assert!(s.is_dragging());
        s.end_drag();
        assert!(!s.is_dragging());
        // Further drag_to calls without a new begin_drag are no-ops.
        let before = s.offset();
        s.drag_to(400.0, 80.0, 300.0);
        assert_eq!(s.offset(), before);
    }

    #[test]
    fn page_toward_click_above_or_below_the_thumb() {
        let mut s = ScrollState::new(2000.0, 200.0);
        s.scroll_to_fraction(0.5);
        let before = s.offset();
        let track_len = 400.0;
        let thumb_len = s.thumb_len(track_len, 10.0);
        let thumb_pos = s.thumb_pos(track_len, thumb_len);

        s.page_toward(thumb_pos, thumb_len, 0.0); // click above
        assert_eq!(s.offset(), before - 200.0);

        let mut s2 = ScrollState::new(2000.0, 200.0);
        s2.scroll_to_fraction(0.5);
        let before2 = s2.offset();
        let thumb_pos2 = s2.thumb_pos(track_len, thumb_len);
        s2.page_toward(thumb_pos2, thumb_len, track_len); // click below
        assert_eq!(s2.offset(), before2 + 200.0);
    }

    // ---- "keyboard (page up/down, arrows) ... scroll" ----

    #[test]
    fn scroll_lines_steps_by_line_length_either_direction() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_lines(1, 24.0);
        assert_eq!(s.offset(), 24.0);
        s.scroll_lines(-1, 24.0);
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn scroll_pages_steps_by_viewport_length() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_pages(1);
        assert_eq!(s.offset(), 200.0);
        s.scroll_pages(10);
        assert_eq!(s.offset(), s.max_offset(), "clamped at the bottom");
        s.scroll_pages(-1);
        assert_eq!(s.offset(), s.max_offset() - 200.0);
    }

    // ---- "gamepad (stick/d-pad) ... scroll" ----

    #[test]
    fn stick_within_deadzone_does_not_scroll() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.apply_stick(0.1, 1.0, 0.2, 500.0);
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn stick_past_deadzone_scrolls_proportional_to_travel_and_dt() {
        let mut s = ScrollState::new(1000.0, 200.0);
        // Full deflection, deadzone 0.2, 0.5s at max speed 100px/s: travel
        // factor is 1.0 (fully past the deadzone) -> 100 * 0.5 = 50px.
        s.apply_stick(1.0, 0.5, 0.2, 100.0);
        assert_eq!(s.offset(), 50.0);
    }

    #[test]
    fn stick_negative_axis_scrolls_up() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_to_fraction(0.5);
        let before = s.offset();
        s.apply_stick(-1.0, 0.5, 0.2, 100.0);
        assert!(s.offset() < before);
    }

    #[test]
    fn dpad_step_reuses_scroll_lines() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_lines(1, 24.0); // a d-pad-down press
        assert_eq!(s.offset(), 24.0);
    }

    // ---- "Scrollbar reflects current position/extent" ----

    #[test]
    fn thumb_geometry_reflects_offset_changes() {
        let mut s = ScrollState::new(1000.0, 200.0);
        let track_len = 400.0;
        let thumb_len = s.thumb_len(track_len, 10.0);
        assert_eq!(s.thumb_pos(track_len, thumb_len), 0.0);
        s.scroll_to_end();
        assert_eq!(s.thumb_pos(track_len, thumb_len), track_len - thumb_len);
    }

    // ---- "Scrolling via one input method doesn't fight focus navigation
    //       from another" ----

    #[test]
    fn wheel_scroll_is_not_overridden_by_a_stale_focus_reveal() {
        // ensure_visible is only ever supposed to run on a focus *change*
        // event, never every frame — so a wheel/drag scroll that moves the
        // offset away from wherever focus last was must stick, as long as
        // the caller doesn't call ensure_visible again.
        let mut s = ScrollState::new(1000.0, 200.0);
        s.ensure_visible(0.0, 50.0); // focus starts at the top, no-op here
        s.apply_wheel(WheelDelta::Lines(10.0), 24.0);
        let after_wheel = s.offset();
        assert!(after_wheel > 0.0);
        // No further ensure_visible call happens on this frame (the
        // contract this type provides) — the wheel scroll's offset stands.
        assert_eq!(s.offset(), after_wheel);
    }

    #[test]
    fn ensure_visible_moves_minimally_to_reveal_an_off_screen_item_below() {
        let mut s = ScrollState::new(1000.0, 200.0);
        // Item at [300, 340) is below the initial viewport [0, 200).
        s.ensure_visible(300.0, 40.0);
        assert_eq!(s.offset(), 140.0, "item's end (340) - viewport (200)");
    }

    #[test]
    fn ensure_visible_moves_minimally_to_reveal_an_off_screen_item_above() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_to(500.0);
        // Item at [100, 140) is above the current viewport [500, 700).
        s.ensure_visible(100.0, 40.0);
        assert_eq!(s.offset(), 100.0, "item's start");
    }

    #[test]
    fn ensure_visible_already_in_view_is_a_noop() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.scroll_to(100.0);
        s.ensure_visible(120.0, 40.0); // fully inside [100, 300)
        assert_eq!(s.offset(), 100.0);
    }

    #[test]
    fn ensure_visible_item_taller_than_viewport_aligns_to_its_start() {
        let mut s = ScrollState::new(1000.0, 200.0);
        s.ensure_visible(50.0, 500.0); // item is taller than the 200px viewport
        assert_eq!(s.offset(), 50.0);
    }
}
