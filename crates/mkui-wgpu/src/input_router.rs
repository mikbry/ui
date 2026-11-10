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
    fn consumed() -> Self {
        Self {
            routed: Routed::Consumed,
            click: None,
        }
    }

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
    /// Whether the most recent Escape *press* was consumed, so the matching
    /// Escape *release* reports the same verdict without recomputing it
    /// against state the press already mutated (mirrors the press/release
    /// symmetry [`PointerState`] already relies on for `MouseInput`).
    escape_was_consumed: bool,
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
    pub fn route(
        &mut self,
        event: &WindowEvent,
        hit_entries: &[HitTestEntry],
        scale_factor: f64,
    ) -> RouteOutcome {
        match event {
            WindowEvent::CursorMoved { position, .. } => self.route_cursor_moved(*position),
            WindowEvent::CursorLeft { .. } => self.route_cursor_left(),
            WindowEvent::MouseInput { state, button, .. } => {
                self.route_mouse_input(*state, *button, hit_entries, scale_factor)
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.route_key(event.physical_key, event.state)
            }
            _ => RouteOutcome::ignored(),
        }
    }

    fn route_cursor_moved(&mut self, position: PhysicalPosition<f64>) -> RouteOutcome {
        self.pointer.update_cursor(position);
        // A drag in progress (something armed by an earlier press) must
        // keep consuming pointer motion even once the cursor has left the
        // armed control's rect — otherwise the host's camera/look control
        // would pick up the remainder of the drag the instant the cursor
        // crosses the control's edge.
        RouteOutcome::from_consumed(self.pointer.armed().is_some())
    }

    fn route_cursor_left(&mut self) -> RouteOutcome {
        let was_armed = self.pointer.armed().is_some();
        self.pointer.cancel();
        RouteOutcome::from_consumed(was_armed)
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
                RouteOutcome::from_consumed(armed.is_some())
            }
            ElementState::Released => {
                // A gesture that was armed stays consumed on release even
                // if the release lands off-node (drag-cancel) — the host
                // must not see the tail end of a drag it never got the
                // start of.
                let was_armed = self.pointer.armed().is_some();
                let click = self.pointer.on_release(hit_entries, cursor);
                RouteOutcome {
                    routed: Routed::from_consumed(was_armed || click.is_some()),
                    click,
                }
            }
        }
    }

    fn route_key(&mut self, physical_key: PhysicalKey, state: ElementState) -> RouteOutcome {
        if matches!(physical_key, PhysicalKey::Code(KeyCode::Escape)) {
            return self.route_escape(state);
        }

        if self.text_focus.is_some() {
            // A composing text field claims every other key — WASD, arrow
            // keys, and any other control-bound key must never reach the
            // host's gameplay input while the user is typing.
            return RouteOutcome::consumed();
        }

        RouteOutcome::ignored()
    }

    fn route_escape(&mut self, state: ElementState) -> RouteOutcome {
        if state != ElementState::Pressed {
            // The release half of an Escape chord mirrors whatever the
            // press decided — recomputing against post-press state (an
            // already-popped modal, an already-cleared arm) would silently
            // disagree with the press's own verdict.
            return RouteOutcome::from_consumed(self.escape_was_consumed);
        }

        let consumed = if self.pop_modal().is_some() {
            // Dialog precedence: the topmost open modal consumes Escape
            // before anything else — including a simultaneously in-flight
            // pointer drag or text focus — observes it.
            true
        } else {
            // No modal open: Escape still cancels mkui's own in-progress
            // state (an armed drag, a focused text field) so a stray
            // release can't resurrect it, but it falls through to the host
            // once that's done — e.g. the host's own pause menu.
            let had_armed = self.pointer.armed().is_some();
            self.pointer.cancel();
            let had_text_focus = self.text_focus.take().is_some();
            had_armed || had_text_focus
        };
        self.escape_was_consumed = consumed;
        RouteOutcome::from_consumed(consumed)
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

        let pressed = router.route_key(PhysicalKey::Code(KeyCode::Escape), ElementState::Pressed);
        assert!(pressed.routed.is_consumed());
        assert_eq!(
            router.top_modal(),
            Some(NodeId::from_raw(1, 0)),
            "Escape pops exactly the topmost modal"
        );

        let released = router.route_key(PhysicalKey::Code(KeyCode::Escape), ElementState::Released);
        assert!(
            released.routed.is_consumed(),
            "the matching release mirrors the press's verdict"
        );
    }

    #[test]
    fn escape_with_no_modal_falls_through_to_the_host() {
        let mut router = InputRouter::new();
        let outcome = router.route_key(PhysicalKey::Code(KeyCode::Escape), ElementState::Pressed);
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

        let outcome = router.route_key(PhysicalKey::Code(KeyCode::Escape), ElementState::Pressed);
        assert!(
            outcome.routed.is_consumed(),
            "Escape cancelling mkui's own in-progress drag is still mkui's to consume"
        );
        assert!(router.armed().is_none());
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
            let outcome = router.route_key(PhysicalKey::Code(code), ElementState::Pressed);
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

        let outcome = router.route_key(PhysicalKey::Code(KeyCode::KeyW), ElementState::Pressed);
        assert!(
            outcome.routed.is_ignored(),
            "once blurred, gameplay keys are the host's again"
        );
    }

    #[test]
    fn escape_blurs_a_focused_text_field_with_no_modal_open() {
        let mut router = InputRouter::new();
        router.focus_text(NodeId::from_raw(7, 0));

        let outcome = router.route_key(PhysicalKey::Code(KeyCode::Escape), ElementState::Pressed);
        assert!(outcome.routed.is_consumed());
        assert!(!router.is_text_focused(), "Escape blurs the field");
    }
}
