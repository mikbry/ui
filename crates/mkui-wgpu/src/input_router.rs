//! Input routing contract for embedding hosts (#176, parent tracker #173).
//!
//! An embedding host — a `wgpu`-based game or interactive engine drawing
//! mkui inside its own frame — owns the `winit` event loop. It forwards
//! every `WindowEvent` it receives into [`InputRouter::route`] and learns,
//! per event, whether mkui consumed it ([`mkui_core::input::Routed`]). The
//! host must not feed a consumed event to its own gameplay input (movement
//! keys, camera look, hotkeys, ...); an ignored event is the host's to do
//! with as it pleases.
//!
//! This is deliberately a *standalone* contract: it does not assume a host
//! example exists yet (#175, the host-owned GPU/frame embedding mode, is a
//! sibling in-flight issue) and it does not assume real dialog or text-field
//! components exist yet (#185, #188). [`InputRouter`] exposes the seams
//! those components plug into — a modal stack for Escape precedence, a
//! text-focus slot for key-swallowing — as plain methods a future component
//! implementation calls. The router works today against whatever
//! `HitTestEntry`s the walker already produces (buttons); it generalizes to
//! any future interactive node without a shape change.
//!
//! ## Acceptance criteria → mechanism
//!
//! - **Clicking a HUD control never reaches the host's gameplay input** —
//!   [`InputRouter::route`] hit-tests `MouseInput` against the current
//!   frame's `HitTestEntry`s (exact, rounded-corner-aware —
//!   [`crate::types::rect_contains_rounded`]) and reports `Consumed` for any
//!   press or release that lands on one.
//! - **Dragging a slider never reaches the host's camera/look controls** —
//!   once a press arms a node, every `CursorMoved` and the eventual
//!   `MouseInput` release stay `Consumed` for the rest of the gesture, even
//!   after the cursor has left the control's rect (the standard
//!   press-to-arm drag contract, generalized from [`crate::pointer`]).
//! - **Escape closes the topmost open dialog before anything else observes
//!   the key** — [`InputRouter::push_modal`] / [`InputRouter::pop_modal`]
//!   track open dialogs; `route` checks the modal stack first and reports
//!   `Consumed` (popping the top) before falling through to pointer-cancel
//!   or text-blur.
//! - **A focused text field swallows WASD/arrow keys and other
//!   control-bound keys while composing** — while
//!   [`InputRouter::focus_text`] is active, every non-Escape key event is
//!   `Consumed` unconditionally.

use std::collections::HashMap;

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use mkui_core::input::Routed;
use mkui_runtime::NodeId;

use crate::pointer::{left_button_state, ClickHit, PointerState};
use crate::walker::HitTestEntry;

/// Outcome of routing one `winit::WindowEvent` through [`InputRouter::route`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RouteOutcome {
    /// Whether the host must suppress its own handling of this event.
    pub routed: Routed,
    /// Set when this event completed a press-release gesture that fired an
    /// action. Callers typically rebuild the scene and request a redraw
    /// when this is `Some`, mirroring [`crate::app::WgpuApp`]'s standalone
    /// `MouseInput` handling.
    pub click: Option<ClickHit>,
}

impl RouteOutcome {
    fn ignored() -> Self {
        Self {
            routed: Routed::Ignored,
            click: None,
        }
    }

    fn from_consumed(consumed: bool) -> Self {
        Self {
            routed: Routed::from_consumed(consumed),
            click: None,
        }
    }
}

/// Input routing state for one embedded mkui surface. Owns the pointer
/// press-to-arm state machine, the open-dialog stack, and the text-focus
/// slot — everything [`InputRouter::route`] needs to answer "did mkui want
/// this event?" without the host having to know mkui's internals.
#[derive(Debug, Default)]
pub struct InputRouter {
    pointer: PointerState,
    /// Open dialogs/sheets, topmost last. Components that open a modal
    /// surface (#188) call [`InputRouter::push_modal`]; closing it (by any
    /// means — a button, a backdrop click, Escape) calls
    /// [`InputRouter::pop_modal`]. Escape always targets
    /// [`InputRouter::top_modal`] first.
    modal_stack: Vec<NodeId>,
    /// Node currently claiming all keyboard input, e.g. a composing text
    /// field (#185). `Some` makes every non-Escape key event `Consumed`.
    text_focus: Option<NodeId>,
    /// Per-physical-key routing verdict, decided once on the initial
    /// (non-repeat) press and replayed verbatim for every OS key-repeat and
    /// for the matching release. Keyboard ownership (text focus, the modal
    /// stack) can change *during* a held key — e.g. a text field gains
    /// focus between a key's press and release — but the host already
    /// acted on the press's verdict, so the release must mirror it exactly
    /// rather than re-querying the now-different state (review P1#1,
    /// P1#2). Cleared on release; a key never seen pressed defaults to
    /// `Ignored` on an unmatched release.
    key_ownership: HashMap<PhysicalKey, Routed>,
    /// Whether mkui currently owns the in-flight pointer gesture — true
    /// from the moment a press arms a control until the *physical* button
    /// goes up (`MouseInput::Released`) or window focus is lost. Tracked
    /// separately from [`PointerState::armed`], which Escape/`CursorLeft`
    /// clear to stop the click from firing: clearing `armed` must not also
    /// hand the rest of a still-physically-held gesture to the host
    /// (review P2#1). `CursorMoved`/`CursorLeft`/the eventual release all
    /// consult this flag instead of `armed()`.
    gesture_owned: bool,
}

impl InputRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a modal surface. Escape now targets this node before anything
    /// else observes the key.
    pub fn push_modal(&mut self, node: NodeId) {
        self.modal_stack.push(node);
    }

    /// Close a modal surface explicitly (a button inside it, a backdrop
    /// click — anything other than the Escape path, which pops internally).
    /// Pops whichever modal is topmost; callers close modals in
    /// last-opened-first-closed order.
    pub fn pop_modal(&mut self) -> Option<NodeId> {
        self.modal_stack.pop()
    }

    pub fn top_modal(&self) -> Option<NodeId> {
        self.modal_stack.last().copied()
    }

    /// Claim all keyboard input for `node` (e.g. a text field gaining
    /// focus and entering composition).
    pub fn focus_text(&mut self, node: NodeId) {
        self.text_focus = Some(node);
    }

    /// Release the keyboard-input claim. Call on blur (Tab away, click
    /// elsewhere, programmatic blur).
    pub fn blur_text(&mut self) {
        self.text_focus = None;
    }

    pub fn is_text_focused(&self) -> bool {
        self.text_focus.is_some()
    }

    pub fn focused_text_node(&self) -> Option<NodeId> {
        self.text_focus
    }

    /// Currently pointer-armed node, if a press-to-arm gesture is in
    /// progress. Exposed for diagnostics/tests.
    pub fn armed(&self) -> Option<NodeId> {
        self.pointer.armed()
    }

    /// Route one `winit::WindowEvent` against the current frame's
    /// `hit_entries`. `scale_factor` converts a `MouseInput`'s latched
    /// physical cursor position to the logical coordinates `hit_entries`
    /// are expressed in (same contract as [`crate::pointer::PointerState`]).
    ///
    /// If the host applies a UI-scale setting on top of its own window's
    /// OS scale factor (#179 — see `WgpuApp::ui_scale` for the winit-owned
    /// equivalent), pass the **combined** `os_scale_factor * ui_scale`
    /// here, not the bare OS value: `hit_entries`' rects are laid out
    /// against a viewport already divided by that same combined factor, so
    /// passing anything else drifts hit-testing out of sync with what's on
    /// screen the moment `ui_scale != 1.0`.
    pub fn route(
        &mut self,
        event: &WindowEvent,
        hit_entries: &[HitTestEntry],
        scale_factor: f64,
    ) -> RouteOutcome {
        match event {
            WindowEvent::CursorMoved { position, .. } => self.route_cursor_moved(*position),
            WindowEvent::CursorLeft { .. } => self.route_cursor_left(),
            WindowEvent::Focused(false) => self.route_focus_lost(),
            WindowEvent::MouseInput { state, button, .. } => {
                self.route_mouse_input(*state, *button, hit_entries, scale_factor)
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.route_key(event.physical_key, event.state, event.repeat)
            }
            _ => RouteOutcome::ignored(),
        }
    }

    fn route_cursor_moved(&mut self, position: PhysicalPosition<f64>) -> RouteOutcome {
        self.pointer.update_cursor(position);
        // The gesture-ownership flag (not `pointer.armed()`) decides this:
        // Escape/`CursorLeft` clear `armed` to stop the click from firing
        // but deliberately leave `gesture_owned` set, since the physical
        // button may still be held (review P2#1).
        RouteOutcome::from_consumed(self.gesture_owned)
    }

    fn route_cursor_left(&mut self) -> RouteOutcome {
        let consumed = self.gesture_owned;
        // Clears `armed` so a later release can't fire the click, but
        // `gesture_owned` is left untouched — the mouse button may still
        // be physically down, and the eventual release is still mkui's to
        // consume (review P2#1).
        self.pointer.cancel();
        RouteOutcome::from_consumed(consumed)
    }

    /// The window lost focus while a press may still be armed (e.g. the
    /// user alt-tabs away without releasing the mouse button). There is no
    /// guarantee the eventual physical release will even reach this
    /// window, so the pending activation is cancelled outright rather than
    /// left armed for a possibly-stale release to resurrect (review P2#2).
    fn route_focus_lost(&mut self) -> RouteOutcome {
        self.pointer.cancel();
        self.gesture_owned = false;
        RouteOutcome::ignored()
    }

    fn route_mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
        hit_entries: &[HitTestEntry],
        scale_factor: f64,
    ) -> RouteOutcome {
        let Some(state) = left_button_state(button, state) else {
            // Non-primary buttons are not part of this contract yet; let
            // the host see them.
            return RouteOutcome::ignored();
        };
        let Some(cursor) = self.pointer.cursor_logical(scale_factor) else {
            return RouteOutcome::ignored();
        };
        match state {
            ElementState::Pressed => {
                let armed = self.pointer.on_press(hit_entries, cursor);
                // Always re-derive from this press — a fresh press defines
                // whether the new gesture is mkui's, regardless of
                // whatever `gesture_owned` was left at by an earlier,
                // already-concluded gesture.
                self.gesture_owned = armed.is_some();
                RouteOutcome::from_consumed(self.gesture_owned)
            }
            ElementState::Released => {
                // The physical button is coming up: this terminates
                // gesture ownership regardless of outcome. Read it before
                // clearing — a gesture mkui claimed at press time (even if
                // later cancelled by Escape/CursorLeft) still owns this
                // release (review P2#1).
                let gesture_owned = self.gesture_owned;
                self.gesture_owned = false;
                let click = self.pointer.on_release(hit_entries, cursor);
                RouteOutcome {
                    routed: Routed::from_consumed(gesture_owned || click.is_some()),
                    click,
                }
            }
        }
    }

    fn route_key(
        &mut self,
        physical_key: PhysicalKey,
        state: ElementState,
        repeat: bool,
    ) -> RouteOutcome {
        match state {
            ElementState::Pressed => {
                if repeat {
                    // An OS key-repeat for a key already tracked replays
                    // its original verdict verbatim: it must not pop a
                    // second modal off the stack, nor re-query text focus
                    // against state that may have changed since the
                    // initial press (review P1#2). A repeat with no
                    // tracked entry (the initial press was missed, or this
                    // is the first event we've seen for the key) falls
                    // through to the fresh-press path below.
                    if let Some(&decision) = self.key_ownership.get(&physical_key) {
                        return RouteOutcome::from_consumed(decision.is_consumed());
                    }
                }
                let decision = self.decide_key_press(physical_key);
                self.key_ownership.insert(physical_key, decision);
                RouteOutcome::from_consumed(decision.is_consumed())
            }
            ElementState::Released => {
                // The release always mirrors whatever the matching press
                // decided, never the router's *current* state — keyboard
                // ownership (text focus, the modal stack) can change
                // between a key's press and release, and the host already
                // acted on the press's verdict (review P1#1). A release
                // with no tracked press (should not happen in practice) is
                // conservatively `Ignored`, leaving it to the host.
                let decision = self.key_ownership.remove(&physical_key);
                RouteOutcome::from_consumed(decision.is_some_and(Routed::is_consumed))
            }
        }
    }

    /// Decide a *fresh* (non-repeat, or first-seen) key press's verdict.
    /// Called exactly once per physical press; the result is cached in
    /// `key_ownership` for repeats and the matching release to replay.
    fn decide_key_press(&mut self, physical_key: PhysicalKey) -> Routed {
        if matches!(physical_key, PhysicalKey::Code(KeyCode::Escape)) {
            return Routed::from_consumed(self.decide_escape_press());
        }

        // A composing text field claims every other key — WASD, arrow
        // keys, and any other control-bound key must never reach the
        // host's gameplay input while the user is typing.
        Routed::from_consumed(self.text_focus.is_some())
    }

    /// Dialog precedence: the topmost open modal consumes Escape before
    /// anything else — including a simultaneously in-flight pointer drag
    /// or text focus — observes it. With no modal open, Escape still
    /// cancels mkui's own in-progress state (an armed drag, a focused text
    /// field) so a stray release can't resurrect it, but it falls through
    /// to the host once that's done — e.g. the host's own pause menu.
    fn decide_escape_press(&mut self) -> bool {
        if self.pop_modal().is_some() {
            true
        } else {
            let had_armed = self.pointer.armed().is_some();
            // Clears `armed` (stops the click from firing) but not
            // `gesture_owned` — a still-held mouse button's remaining
            // motion/release stays mkui's (review P2#1).
            self.pointer.cancel();
            let had_text_focus = self.text_focus.take().is_some();
            had_armed || had_text_focus
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CornerRadii, Point, Rect, Size};
    use mkui_runtime::NodeId;
    use winit::event::DeviceId;

    fn hud_button(idx: u32) -> HitTestEntry {
        HitTestEntry {
            rect: Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 40.0)),
            node: NodeId::from_raw(idx, 0),
            action: None,
            corner_radii: CornerRadii::all(0.0),
        }
    }

    fn cursor_moved(x: f64, y: f64) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(x, y),
        }
    }

    fn mouse_input(state: ElementState) -> WindowEvent {
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button: MouseButton::Left,
        }
    }

    // ---- "Clicking a HUD control never reaches the host's gameplay input" ----

    #[test]
    fn click_on_hud_button_is_consumed() {
        let mut router = InputRouter::new();
        let entries = vec![hud_button(1)];
        router.route(&cursor_moved(50.0, 20.0), &entries, 1.0);

        let press = router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(
            press.routed.is_consumed(),
            "press on a HUD control must be consumed"
        );

        let release = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(release.routed.is_consumed());
        assert!(
            release.click.is_some(),
            "release on the armed node fires the click"
        );
    }

    #[test]
    fn click_on_empty_space_is_ignored() {
        let mut router = InputRouter::new();
        let entries = vec![hud_button(1)];
        router.route(&cursor_moved(500.0, 500.0), &entries, 1.0);

        let press = router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(
            press.routed.is_ignored(),
            "a press over empty space must fall through to the host"
        );
    }

    // ---- "Dragging a slider never reaches the host's camera/look controls" ----

    #[test]
    fn drag_stays_consumed_after_the_cursor_leaves_the_control() {
        let mut router = InputRouter::new();
        let entries = vec![hud_button(1)];
        router.route(&cursor_moved(50.0, 20.0), &entries, 1.0);
        let press = router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(press.routed.is_consumed());

        // Cursor drags far outside the control's rect — must stay consumed
        // so the host's camera/look control never sees the mid-drag motion.
        let moved = router.route(&cursor_moved(900.0, 900.0), &entries, 1.0);
        assert!(
            moved.routed.is_consumed(),
            "pointer motion mid-drag must stay consumed even off-control"
        );

        // Release off-control: no click fires, but the release itself is
        // still mkui's to consume — the host never saw the drag start.
        let release = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(release.routed.is_consumed());
        assert!(
            release.click.is_none(),
            "release off-node must not fire an action"
        );
        assert!(
            router.armed().is_none(),
            "release clears the armed slot either way"
        );
    }

    #[test]
    fn idle_cursor_motion_with_nothing_armed_is_ignored() {
        let mut router = InputRouter::new();
        let moved = router.route(&cursor_moved(10.0, 10.0), &[], 1.0);
        assert!(
            moved.routed.is_ignored(),
            "idle motion is the host's camera-look input"
        );
    }

    // ---- "Escape closes the topmost open dialog before anything else observes the key" ----

    #[test]
    fn escape_closes_the_topmost_modal_and_is_consumed() {
        let mut router = InputRouter::new();
        router.push_modal(NodeId::from_raw(1, 0));
        router.push_modal(NodeId::from_raw(2, 0));

        let pressed = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Pressed,
            false,
        );
        assert!(pressed.routed.is_consumed());
        assert_eq!(
            router.top_modal(),
            Some(NodeId::from_raw(1, 0)),
            "Escape pops exactly the topmost modal"
        );

        let released = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Released,
            false,
        );
        assert!(
            released.routed.is_consumed(),
            "the matching release mirrors the press's verdict"
        );
    }

    #[test]
    fn escape_with_no_modal_falls_through_to_the_host() {
        let mut router = InputRouter::new();
        let outcome = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Pressed,
            false,
        );
        assert!(
            outcome.routed.is_ignored(),
            "with nothing open, the host's own Escape handling (e.g. a pause menu) must see it"
        );
    }

    #[test]
    fn escape_cancels_an_armed_drag_before_falling_through() {
        let mut router = InputRouter::new();
        let entries = vec![hud_button(1)];
        router.route(&cursor_moved(50.0, 20.0), &entries, 1.0);
        router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(router.armed().is_some());

        let outcome = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Pressed,
            false,
        );
        assert!(
            outcome.routed.is_consumed(),
            "Escape cancelling mkui's own in-progress drag is still mkui's to consume"
        );
        assert!(router.armed().is_none());
    }

    // ---- review P1#2: Escape key-repeat must not pop a second modal ----

    #[test]
    fn escape_repeat_does_not_pop_additional_modals_or_disagree_on_release() {
        let mut router = InputRouter::new();
        router.push_modal(NodeId::from_raw(1, 0));
        router.push_modal(NodeId::from_raw(2, 0));

        let first = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Pressed,
            false,
        );
        assert!(first.routed.is_consumed());
        assert_eq!(router.top_modal(), Some(NodeId::from_raw(1, 0)));

        // The OS fires repeated Pressed events while the key stays held.
        for _ in 0..3 {
            let repeat = router.route_key(
                PhysicalKey::Code(KeyCode::Escape),
                ElementState::Pressed,
                true,
            );
            assert!(repeat.routed.is_consumed());
        }
        assert_eq!(
            router.top_modal(),
            Some(NodeId::from_raw(1, 0)),
            "repeats must not pop a second modal"
        );

        let released = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Released,
            false,
        );
        assert!(
            released.routed.is_consumed(),
            "release must mirror the original press's verdict, not whatever the stack looks like now"
        );
    }

    // ---- review P2#1: cancelling a drag must not disown the rest of the gesture ----

    #[test]
    fn escape_cancelling_a_drag_keeps_the_rest_of_the_gesture_consumed() {
        let mut router = InputRouter::new();
        let entries = vec![hud_button(1)];
        router.route(&cursor_moved(50.0, 20.0), &entries, 1.0);
        router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);

        let escape = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Pressed,
            false,
        );
        assert!(escape.routed.is_consumed());
        assert!(
            router.armed().is_none(),
            "the arm is cleared so the click can't fire"
        );

        // The mouse button is still physically held; motion and the
        // eventual release must stay mkui's, not leak to the host's
        // camera/look control.
        let moved = router.route(&cursor_moved(900.0, 900.0), &entries, 1.0);
        assert!(
            moved.routed.is_consumed(),
            "drag ownership must survive an Escape cancel"
        );

        let released = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(released.routed.is_consumed());
        assert!(
            released.click.is_none(),
            "the cancelled gesture must not fire a click"
        );
    }

    #[test]
    fn cursor_left_cancelling_a_drag_keeps_the_release_consumed() {
        let mut router = InputRouter::new();
        let entries = vec![hud_button(1)];
        router.route(&cursor_moved(50.0, 20.0), &entries, 1.0);
        router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);

        let left = router.route(
            &WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            },
            &entries,
            1.0,
        );
        assert!(left.routed.is_consumed());
        assert!(router.armed().is_none());

        let released = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(
            released.routed.is_consumed(),
            "the physical release still belongs to mkui after CursorLeft cancelled the arm"
        );
        assert!(released.click.is_none());
    }

    // ---- review P2#2: losing window focus must cancel pending activation ----

    #[test]
    fn window_focus_loss_cancels_pending_activation() {
        let mut router = InputRouter::new();
        let entries = vec![hud_button(1)];
        router.route(&cursor_moved(50.0, 20.0), &entries, 1.0);
        router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(router.armed().is_some());

        router.route(&WindowEvent::Focused(false), &entries, 1.0);
        assert!(
            router.armed().is_none(),
            "losing focus must cancel the pending activation"
        );

        // The user alt-tabs back and releases somewhere in the window —
        // the stale click must not fire, and the release is the host's.
        let released = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(
            released.click.is_none(),
            "stale click must not fire after a focus-loss cancel"
        );
        assert!(
            released.routed.is_ignored(),
            "the gesture was disowned on focus loss"
        );
    }

    // ---- "A focused text field swallows WASD/arrow keys ... while composing" ----

    #[test]
    fn focused_text_field_swallows_gameplay_keys() {
        let mut router = InputRouter::new();
        router.focus_text(NodeId::from_raw(7, 0));

        for code in [
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::ArrowUp,
        ] {
            let outcome = router.route_key(PhysicalKey::Code(code), ElementState::Pressed, false);
            assert!(
                outcome.routed.is_consumed(),
                "{code:?} must be swallowed while a text field is composing"
            );
        }
    }

    #[test]
    fn blurred_text_field_stops_swallowing_keys() {
        let mut router = InputRouter::new();
        router.focus_text(NodeId::from_raw(7, 0));
        router.blur_text();

        let outcome = router.route_key(
            PhysicalKey::Code(KeyCode::KeyW),
            ElementState::Pressed,
            false,
        );
        assert!(
            outcome.routed.is_ignored(),
            "once blurred, gameplay keys are the host's again"
        );
    }

    #[test]
    fn escape_blurs_a_focused_text_field_with_no_modal_open() {
        let mut router = InputRouter::new();
        router.focus_text(NodeId::from_raw(7, 0));

        let outcome = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Pressed,
            false,
        );
        assert!(outcome.routed.is_consumed());
        assert!(!router.is_text_focused(), "Escape blurs the field");
    }

    // ---- review P1#1: key ownership is locked in at press, not release ----

    #[test]
    fn key_press_before_focus_stays_ignored_through_release_even_after_focus() {
        let mut router = InputRouter::new();
        let press = router.route_key(
            PhysicalKey::Code(KeyCode::KeyW),
            ElementState::Pressed,
            false,
        );
        assert!(
            press.routed.is_ignored(),
            "no text focus yet: W is the host's"
        );

        // A text field gains focus while W is still physically held.
        router.focus_text(NodeId::from_raw(1, 0));

        let release = router.route_key(
            PhysicalKey::Code(KeyCode::KeyW),
            ElementState::Released,
            false,
        );
        assert!(
            release.routed.is_ignored(),
            "the release must mirror the press, not the now-focused state — \
             otherwise the host applies the press but never a matching release"
        );
    }

    #[test]
    fn key_consumed_while_composing_stays_consumed_through_release_even_after_blur() {
        let mut router = InputRouter::new();
        router.focus_text(NodeId::from_raw(1, 0));
        let press = router.route_key(
            PhysicalKey::Code(KeyCode::KeyW),
            ElementState::Pressed,
            false,
        );
        assert!(press.routed.is_consumed());

        // The field blurs while W is still physically held.
        router.blur_text();

        let release = router.route_key(
            PhysicalKey::Code(KeyCode::KeyW),
            ElementState::Released,
            false,
        );
        assert!(
            release.routed.is_consumed(),
            "the release must mirror the press even after blur"
        );
    }

    #[test]
    fn repeat_of_a_swallowed_key_replays_the_original_verdict() {
        let mut router = InputRouter::new();
        router.focus_text(NodeId::from_raw(1, 0));
        router.route_key(
            PhysicalKey::Code(KeyCode::KeyW),
            ElementState::Pressed,
            false,
        );

        router.blur_text();
        // OS key-repeat fires with the field already blurred; it must
        // still replay the press's original `Consumed` verdict.
        let repeat = router.route_key(
            PhysicalKey::Code(KeyCode::KeyW),
            ElementState::Pressed,
            true,
        );
        assert!(
            repeat.routed.is_consumed(),
            "a repeat must replay the original press's verdict, not re-query current focus"
        );
    }
}
