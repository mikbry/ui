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
//! sibling in-flight issue). [`InputRouter`] exposes the seams dialog/text-field
//! components plug into — a modal stack for Escape/backdrop-click precedence
//! and the Tab focus trap, a text-focus slot for key-swallowing — as plain
//! methods a component implementation calls; [`crate::components::dialog`]
//! (#188) is the first caller of the modal-stack half of that contract. The
//! router works today against whatever `HitTestEntry`s the walker already
//! produces (buttons); it generalizes to any future interactive node without
//! a shape change.
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
//! - **Backdrop click closes the topmost open dialog; a click on the
//!   dialog's own (otherwise inert) interior does not** (#188) —
//!   [`InputRouter::push_modal`] carries the dialog's panel rect and the
//!   explicit set of `NodeId`s it owns. A press that misses every *owned*
//!   `HitTestEntry` *and* lands outside the panel rect pops the topmost
//!   modal (backdrop dismiss); a press that misses every owned entry but
//!   still lands inside the rect is `Consumed` without popping anything,
//!   so blank dialog chrome swallows the click rather than leaking it to
//!   whatever the dialog is covering. Pointer targets are restricted to
//!   the topmost dialog's owned set *before* arming, so a lower (covered)
//!   dialog's control, or a host control the caller forgot to omit, can
//!   never fire even if its rect happens to overlap this dialog's panel.
//!   While any modal is open, every primary-button press is `Consumed` by
//!   one of these three outcomes.
//! - **Tab/Shift+Tab focus cannot leave the topmost open dialog** (#188) —
//!   while a modal is open, `route` derives that dialog's focusable set
//!   from the `HitTestEntry`s whose `NodeId` is in its *owned* set — never
//!   from geometry — and cycles [`InputRouter::trapped_focus_node`]
//!   forward (Tab) or backward (Shift+Tab) through exactly that set,
//!   wrapping at both ends. With no modal open, Tab is untouched by this
//!   contract and falls through like any other key.

use std::collections::HashMap;

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Modifiers, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use mkui_core::input::Routed;
use mkui_runtime::NodeId;

use crate::pointer::{left_button_state, ClickHit, PointerState};
use crate::types::{rect_contains, Rect};
use crate::walker::HitTestEntry;

/// One open dialog on [`InputRouter`]'s modal stack: the node identity
/// [`InputRouter::top_modal`] reports, the panel rect
/// [`InputRouter::route`] needs to tell a backdrop click (dismiss) apart
/// from a click on the dialog's own interior (swallow, don't dismiss), and
/// the explicit set of `NodeId`s this dialog owns (#188).
///
/// `owned` is **identity**, not geometry: it is exactly the list the
/// caller handed [`InputRouter::push_modal`], and it is what both pointer
/// arming and the Tab focus trap scope themselves to. An earlier revision
/// derived "belongs to this dialog" from whether a `HitTestEntry`'s rect
/// center fell inside `panel_rect` — geometry a host-supplied hit-test list
/// does not actually guarantee: a lower stacked dialog's (now-covered)
/// control, or an unrelated host control the host failed to omit once a
/// modal opened, can sit at any rect, including one that happens to overlap
/// this dialog's panel. Geometric containment would silently let such an
/// entry fire its action or join the focus trap; explicit ownership cannot,
/// because it's the dialog's own author who decides what's "in" it.
#[derive(Debug, Clone)]
struct ModalEntry {
    node: NodeId,
    panel_rect: Rect,
    owned: Vec<NodeId>,
}

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
    modal_stack: Vec<ModalEntry>,
    /// Node currently claiming all keyboard input, e.g. a composing text
    /// field (#185). `Some` makes every non-Escape key event `Consumed`.
    text_focus: Option<NodeId>,
    /// Node the Tab focus-trap cycle currently sits on within the topmost
    /// open modal (#188). `None` means "nothing focused yet in this
    /// dialog" — the next Tab lands on the first focusable entry inside
    /// its panel rect, Shift+Tab on the last. Reset to `None` whenever the
    /// modal stack changes (push or pop) so a freshly (re)topmost dialog
    /// always starts its own cycle from scratch rather than resuming
    /// wherever a now-closed or now-covered dialog left off.
    trap_focus: Option<NodeId>,
    /// Latched from the most recent `WindowEvent::ModifiersChanged`.
    /// Decides Tab's direction: forward when `false`, backward
    /// (Shift+Tab) when `true`.
    shift_held: bool,
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
    /// else observes the key. `panel_rect` is the dialog's own chrome rect
    /// (what [`crate::components::dialog`] painted it at) — `route` uses it
    /// to tell a backdrop click from a click on the dialog's interior.
    /// `owned` is the exact set of `NodeId`s this dialog's own interactive
    /// controls use — `route` restricts pointer arming and the Tab focus
    /// trap to this set (identity, not geometry), so a control that merely
    /// happens to render at a rect overlapping the panel — a lower,
    /// now-covered dialog's button, or a host control the caller forgot to
    /// omit — can never fire or be focused while this dialog is topmost
    /// (#188).
    pub fn push_modal(&mut self, node: NodeId, panel_rect: Rect, owned: Vec<NodeId>) {
        self.modal_stack.push(ModalEntry {
            node,
            panel_rect,
            owned,
        });
        self.trap_focus = None;
    }

    /// Close a modal surface explicitly (a button inside it, a backdrop
    /// click — anything other than the Escape path, which pops internally).
    /// Pops whichever modal is topmost; callers close modals in
    /// last-opened-first-closed order.
    pub fn pop_modal(&mut self) -> Option<NodeId> {
        let popped = self.modal_stack.pop().map(|entry| entry.node);
        self.trap_focus = None;
        popped
    }

    pub fn top_modal(&self) -> Option<NodeId> {
        self.modal_stack.last().map(|entry| entry.node)
    }

    /// Panel rect of the topmost open modal, if any.
    fn top_modal_panel_rect(&self) -> Option<Rect> {
        self.modal_stack.last().map(|entry| entry.panel_rect)
    }

    /// `NodeId`s the topmost open modal explicitly owns (empty slice if no
    /// modal is open, or if the open dialog declared no controls).
    fn top_modal_owned(&self) -> &[NodeId] {
        self.modal_stack
            .last()
            .map(|entry| entry.owned.as_slice())
            .unwrap_or(&[])
    }

    /// Node the Tab focus trap currently sits on inside the topmost open
    /// modal. Exposed so a host can paint a focus ring around it; `None`
    /// both when no modal is open and when one is open but Tab hasn't been
    /// pressed yet.
    pub fn trapped_focus_node(&self) -> Option<NodeId> {
        self.trap_focus
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
            WindowEvent::Focused(false) => self.route_focus_lost(),
            WindowEvent::MouseInput { state, button, .. } => {
                self.route_mouse_input(*state, *button, hit_entries, scale_factor)
            }
            WindowEvent::ModifiersChanged(modifiers) => self.route_modifiers_changed(*modifiers),
            WindowEvent::KeyboardInput { event, .. } => {
                self.route_key_with_hits(event.physical_key, event.state, event.repeat, hit_entries)
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

    /// Latch Shift's held state for Tab's direction (#188). Window focus
    /// loss can strand this latched `true` if the host never delivers the
    /// matching key-up, but that only ever flips Tab's cycle direction —
    /// unlike `armed`/`gesture_owned`, there's no stray action to fire, so
    /// this is deliberately not reset in [`InputRouter::route_focus_lost`].
    fn route_modifiers_changed(&mut self, modifiers: Modifiers) -> RouteOutcome {
        self.shift_held = modifiers.state().shift_key();
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
                if let Some(panel_rect) = self.top_modal_panel_rect() {
                    // A modal is open: pointer targets are restricted to
                    // the topmost dialog's own controls (identity, not
                    // geometry — see `ModalEntry`), so a press can never
                    // arm a lower (now-covered) dialog's button or a host
                    // control the caller forgot to omit, even if its rect
                    // happens to overlap this dialog's panel (#188).
                    let owned = self.top_modal_owned();
                    let scoped: Vec<HitTestEntry> = hit_entries
                        .iter()
                        .copied()
                        .filter(|entry| owned.contains(&entry.node))
                        .collect();
                    let armed = self.pointer.on_press(&scoped, cursor);

                    // Nothing behind an open modal may react to a click,
                    // so every outcome from here is `Consumed`. A press
                    // that missed every owned entry is either a backdrop
                    // click — outside the dialog's own panel, so it
                    // dismisses the topmost modal — or a click on the
                    // dialog's otherwise-inert interior (inside the
                    // panel), which is swallowed without closing anything.
                    if armed.is_none() && !rect_contains(panel_rect, cursor) {
                        self.pop_modal();
                    }
                    self.gesture_owned = true;
                    return RouteOutcome::from_consumed(true);
                }

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

    /// [`InputRouter::route_key_with_hits`] with an empty focusable set.
    /// Only the test suite calls this directly now (every pre-#188 test
    /// that doesn't care about the Tab focus trap); production code always
    /// goes through [`InputRouter::route`], which has real `HitTestEntry`s
    /// to hand `route_key_with_hits`.
    #[cfg(test)]
    fn route_key(
        &mut self,
        physical_key: PhysicalKey,
        state: ElementState,
        repeat: bool,
    ) -> RouteOutcome {
        self.route_key_with_hits(physical_key, state, repeat, &[])
    }

    fn route_key_with_hits(
        &mut self,
        physical_key: PhysicalKey,
        state: ElementState,
        repeat: bool,
        hit_entries: &[HitTestEntry],
    ) -> RouteOutcome {
        match state {
            ElementState::Pressed => {
                if repeat {
                    // An OS key-repeat for a key already tracked replays
                    // its original verdict verbatim: it must not pop a
                    // second modal off the stack, advance the Tab focus
                    // trap an extra step, nor re-query text focus against
                    // state that may have changed since the initial press
                    // (review P1#2). A repeat with no tracked entry (the
                    // initial press was missed, or this is the first event
                    // we've seen for the key) falls through to the
                    // fresh-press path below.
                    if let Some(&decision) = self.key_ownership.get(&physical_key) {
                        return RouteOutcome::from_consumed(decision.is_consumed());
                    }
                }
                let decision = self.decide_key_press(physical_key, hit_entries);
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
    fn decide_key_press(
        &mut self,
        physical_key: PhysicalKey,
        hit_entries: &[HitTestEntry],
    ) -> Routed {
        if matches!(physical_key, PhysicalKey::Code(KeyCode::Escape)) {
            return Routed::from_consumed(self.decide_escape_press());
        }

        if matches!(physical_key, PhysicalKey::Code(KeyCode::Tab)) {
            if let Some(consumed) = self.decide_tab_press(hit_entries) {
                return Routed::from_consumed(consumed);
            }
        }

        // A composing text field claims every other key — WASD, arrow
        // keys, and any other control-bound key must never reach the
        // host's gameplay input while the user is typing.
        Routed::from_consumed(self.text_focus.is_some())
    }

    /// Tab focus-trap cycling (#188): while a modal is open, Tab/Shift+Tab
    /// move [`InputRouter::trapped_focus_node`] forward/backward through
    /// the topmost dialog's own focusable entries — every `HitTestEntry`
    /// whose rect center falls inside its panel rect — wrapping at both
    /// ends so focus can never land on a node outside the dialog. Returns
    /// `None` when no modal is open, so the caller falls through to the
    /// ordinary (non-trapped) key-routing path; Tab is otherwise untouched
    /// by this contract.
    fn decide_tab_press(&mut self, hit_entries: &[HitTestEntry]) -> Option<bool> {
        // A modal must be open for Tab to be this contract's to trap at
        // all; `top_modal_owned` alone can't distinguish "no modal open"
        // from "modal open, declared zero controls", so check explicitly.
        self.top_modal_panel_rect()?;

        let owned = self.top_modal_owned();
        let focusable: Vec<NodeId> = hit_entries
            .iter()
            .filter(|entry| owned.contains(&entry.node))
            .map(|entry| entry.node)
            .collect();

        if focusable.is_empty() {
            // Nothing focusable yet, but Tab is still the modal's to
            // consume — it must not leak past an open dialog.
            self.trap_focus = None;
            return Some(true);
        }

        let len = focusable.len();
        let current = self
            .trap_focus
            .and_then(|node| focusable.iter().position(|&candidate| candidate == node));
        let next = match current {
            None => {
                if self.shift_held {
                    len - 1
                } else {
                    0
                }
            }
            Some(index) => {
                if self.shift_held {
                    (index + len - 1) % len
                } else {
                    (index + 1) % len
                }
            }
        };
        self.trap_focus = Some(focusable[next]);
        Some(true)
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

    /// A button-shaped `HitTestEntry` at an arbitrary rect, for dialog
    /// focusable-set tests that need more than one distinguishable control.
    fn button_at(idx: u32, rect: Rect) -> HitTestEntry {
        HitTestEntry {
            rect,
            node: NodeId::from_raw(idx, 0),
            action: None,
            corner_radii: CornerRadii::all(0.0),
        }
    }

    /// Arbitrary dialog panel rect used by every modal/#188 test that
    /// doesn't care about the exact geometry, only that it's a bounded
    /// region distinct from "the rest of the screen".
    fn dialog_panel_rect() -> Rect {
        Rect::new(Point::new(200.0, 200.0), Size::new(300.0, 200.0))
    }

    fn modifiers_changed(shift: bool) -> WindowEvent {
        let mut state = winit::keyboard::ModifiersState::empty();
        if shift {
            state |= winit::keyboard::ModifiersState::SHIFT;
        }
        WindowEvent::ModifiersChanged(state.into())
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
        router.push_modal(NodeId::from_raw(1, 0), dialog_panel_rect(), Vec::new());
        router.push_modal(NodeId::from_raw(2, 0), dialog_panel_rect(), Vec::new());

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
        router.push_modal(NodeId::from_raw(1, 0), dialog_panel_rect(), Vec::new());
        router.push_modal(NodeId::from_raw(2, 0), dialog_panel_rect(), Vec::new());

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

    // ---- #188: "Backdrop click ... close[s] the topmost dialog only" ----

    #[test]
    fn backdrop_click_closes_the_topmost_modal_and_is_consumed() {
        let mut router = InputRouter::new();
        router.push_modal(NodeId::from_raw(1, 0), dialog_panel_rect(), Vec::new());
        // Outside `dialog_panel_rect()` (200,200)-(500,400) and off every
        // hit entry.
        router.route(&cursor_moved(10.0, 10.0), &[], 1.0);

        let press = router.route(&mouse_input(ElementState::Pressed), &[], 1.0);
        assert!(
            press.routed.is_consumed(),
            "a backdrop click is mkui's, not the host's — nothing behind an open modal reacts"
        );
        assert!(
            router.top_modal().is_none(),
            "the backdrop click dismissed the (only) open modal"
        );

        let release = router.route(&mouse_input(ElementState::Released), &[], 1.0);
        assert!(
            release.routed.is_consumed(),
            "the matching release mirrors the press's gesture ownership"
        );
    }

    #[test]
    fn click_inside_dialog_interior_is_consumed_without_closing_it() {
        let mut router = InputRouter::new();
        router.push_modal(NodeId::from_raw(1, 0), dialog_panel_rect(), Vec::new());
        // Inside the panel rect but on no interactive entry (e.g. a label).
        router.route(&cursor_moved(250.0, 250.0), &[], 1.0);

        let press = router.route(&mouse_input(ElementState::Pressed), &[], 1.0);
        assert!(
            press.routed.is_consumed(),
            "a click on the dialog's own blank interior must not leak to the host"
        );
        assert_eq!(
            router.top_modal(),
            Some(NodeId::from_raw(1, 0)),
            "a click on the dialog's interior does not dismiss it"
        );
    }

    #[test]
    fn click_on_a_control_inside_an_open_dialog_still_fires_its_action() {
        let mut router = InputRouter::new();
        router.push_modal(
            NodeId::from_raw(1, 0),
            dialog_panel_rect(),
            vec![NodeId::from_raw(9, 0)],
        );
        let entries = vec![button_at(
            9,
            Rect::new(Point::new(220.0, 220.0), Size::new(80.0, 30.0)),
        )];
        router.route(&cursor_moved(240.0, 230.0), &entries, 1.0);

        let press = router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(press.routed.is_consumed());

        let release = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(release.routed.is_consumed());
        assert!(
            release.click.is_some(),
            "a control inside an open dialog still fires its click"
        );
        assert_eq!(
            router.top_modal(),
            Some(NodeId::from_raw(1, 0)),
            "clicking a control inside the dialog does not dismiss it — only a backdrop \
             click or Escape does"
        );
    }

    // ---- review: pointer targets must be restricted to the topmost
    // modal's *owned* set, not just hit-tested geometry ----

    #[test]
    fn click_on_an_unowned_entry_inside_the_panel_rect_does_not_fire_or_close() {
        // Regression: an earlier revision decided "belongs to this dialog"
        // by checking whether a HitTestEntry's rect fell inside the panel
        // rect — geometry, not identity. A control that is NOT in the
        // dialog's declared `owned` set but happens to render at a rect
        // that overlaps the panel (a decorative element, a host control
        // the caller forgot to omit) must never fire, even though it sits
        // squarely inside the panel rect.
        let mut router = InputRouter::new();
        router.push_modal(
            NodeId::from_raw(1, 0),
            dialog_panel_rect(),
            vec![NodeId::from_raw(9, 0)],
        );
        // Node 77 is NOT in the owned list, but its rect is well inside
        // `dialog_panel_rect()` ((200,200)-(500,400)).
        let entries = vec![button_at(
            77,
            Rect::new(Point::new(300.0, 300.0), Size::new(40.0, 20.0)),
        )];
        router.route(&cursor_moved(310.0, 305.0), &entries, 1.0);

        let press = router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(press.routed.is_consumed());
        let release = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(
            release.click.is_none(),
            "an unowned entry must never fire its action while a modal is open, \
             even when its rect overlaps the dialog's panel"
        );
        assert_eq!(
            router.top_modal(),
            Some(NodeId::from_raw(1, 0)),
            "landing inside the panel rect is an interior click, not a backdrop click"
        );
    }

    #[test]
    fn backdrop_click_on_an_unowned_background_button_still_dismisses_and_never_fires() {
        // Review P1 exactly: a background button sits *outside* the open
        // dialog's panel rect and is reachable by hit-testing. The original
        // bug let the press arm this unowned entry (since `on_press` was
        // called against the full, unscoped `hit_entries`), which left
        // `armed.is_some()` true — skipping the backdrop-dismiss branch
        // entirely — and fired the background button's click on release.
        // Scoping arming to the topmost modal's `owned` set before calling
        // `on_press` fixes both: the press can no longer arm an unowned
        // node, so it's correctly classified as a backdrop click.
        let mut router = InputRouter::new();
        router.push_modal(
            NodeId::from_raw(1, 0),
            dialog_panel_rect(), // (200,200)-(500,400)
            vec![NodeId::from_raw(9, 0)],
        );
        let entries = vec![button_at(
            77,
            Rect::new(Point::new(10.0, 10.0), Size::new(40.0, 20.0)),
        )];
        router.route(&cursor_moved(20.0, 15.0), &entries, 1.0);

        let press = router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(press.routed.is_consumed());
        assert!(
            router.top_modal().is_none(),
            "an unowned background button outside the panel must not block \
             backdrop dismissal"
        );

        let release = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(
            release.click.is_none(),
            "the background button must never fire its action"
        );
    }

    #[test]
    fn covered_dialogs_control_does_not_fire_while_another_dialog_is_on_top() {
        // Regression: node 5 belongs to the *bottom* dialog and sits at a
        // rect the *top* dialog's panel does not cover. Clicking it while
        // the top dialog is open must not fire the bottom dialog's action
        // — node 5 is not in the topmost (open) dialog's owned set — and
        // must be classified as a backdrop click against the top dialog
        // (outside its panel rect), dismissing it instead.
        let mut router = InputRouter::new();
        let bottom_rect = Rect::new(Point::new(0.0, 0.0), Size::new(600.0, 600.0));
        let top_rect = dialog_panel_rect(); // (200,200)-(500,400)
        let bottom_button = button_at(5, Rect::new(Point::new(10.0, 10.0), Size::new(40.0, 20.0)));
        router.push_modal(
            NodeId::from_raw(1, 0),
            bottom_rect,
            vec![NodeId::from_raw(5, 0)],
        );
        router.push_modal(NodeId::from_raw(2, 0), top_rect, Vec::new());

        let entries = vec![bottom_button];
        router.route(&cursor_moved(20.0, 15.0), &entries, 1.0);
        let press = router.route(&mouse_input(ElementState::Pressed), &entries, 1.0);
        assert!(press.routed.is_consumed());
        let release = router.route(&mouse_input(ElementState::Released), &entries, 1.0);
        assert!(
            release.click.is_none(),
            "the bottom dialog's own control must not fire while it's covered"
        );
        assert_eq!(
            router.top_modal(),
            Some(NodeId::from_raw(1, 0)),
            "the click, outside the top dialog's panel, is a backdrop click on the \
             topmost dialog — it closes the top dialog, leaving the bottom one open \
             (and still not having fired its control)"
        );
    }

    #[test]
    fn backdrop_click_closes_only_the_topmost_of_two_stacked_dialogs() {
        let mut router = InputRouter::new();
        let bottom_rect = Rect::new(Point::new(0.0, 0.0), Size::new(600.0, 600.0));
        let top_rect = dialog_panel_rect();
        router.push_modal(NodeId::from_raw(1, 0), bottom_rect, Vec::new());
        router.push_modal(NodeId::from_raw(2, 0), top_rect, Vec::new());

        // Inside the bottom dialog's (larger) rect, but outside the top
        // dialog's rect — a backdrop click relative to the topmost dialog.
        router.route(&cursor_moved(10.0, 10.0), &[], 1.0);
        let press = router.route(&mouse_input(ElementState::Pressed), &[], 1.0);
        assert!(press.routed.is_consumed());
        assert_eq!(
            router.top_modal(),
            Some(NodeId::from_raw(1, 0)),
            "closes exactly the topmost dialog, leaving the one beneath it open"
        );

        // A second backdrop click, now outside the remaining (bottom)
        // dialog's own rect, closes it too.
        router.route(&cursor_moved(700.0, 700.0), &[], 1.0);
        router.route(&mouse_input(ElementState::Pressed), &[], 1.0);
        assert!(router.top_modal().is_none());
    }

    // ---- #188: "Tab/gamepad focus cannot leave the open dialog while it's open" ----

    #[test]
    fn tab_cycles_forward_through_the_dialogs_own_controls_and_wraps() {
        let mut router = InputRouter::new();
        router.push_modal(
            NodeId::from_raw(1, 0),
            dialog_panel_rect(),
            vec![NodeId::from_raw(10, 0), NodeId::from_raw(11, 0)],
        );
        let entries = vec![
            button_at(
                10,
                Rect::new(Point::new(210.0, 210.0), Size::new(40.0, 20.0)),
            ),
            button_at(
                11,
                Rect::new(Point::new(260.0, 210.0), Size::new(40.0, 20.0)),
            ),
            // Not in the dialog's `owned` set, even though its rect sits
            // well inside `dialog_panel_rect()` — must never be focused by
            // Tab. Ownership, not geometry, decides membership (review).
            button_at(
                99,
                Rect::new(Point::new(400.0, 210.0), Size::new(40.0, 20.0)),
            ),
        ];

        let first = router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert!(first.routed.is_consumed());
        assert_eq!(router.trapped_focus_node(), Some(NodeId::from_raw(10, 0)));

        router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert_eq!(router.trapped_focus_node(), Some(NodeId::from_raw(11, 0)));

        // Wraps back to the first control, never landing on the
        // unowned node 99.
        router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert_eq!(router.trapped_focus_node(), Some(NodeId::from_raw(10, 0)));
    }

    #[test]
    fn tab_includes_an_owned_control_whose_rect_sits_outside_the_panel_rect() {
        // Complement of the above: ownership, not geometry, decides
        // membership in the other direction too. Node 10 is in the
        // dialog's `owned` set but its hit rect — a tooltip-offset button,
        // or a control a layout pass temporarily placed outside the
        // panel's current bounds — sits outside `dialog_panel_rect()`.
        // It must still be reachable by Tab.
        let mut router = InputRouter::new();
        router.push_modal(
            NodeId::from_raw(1, 0),
            dialog_panel_rect(), // (200,200)-(500,400)
            vec![NodeId::from_raw(10, 0)],
        );
        let entries = vec![button_at(
            10,
            Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 20.0)),
        )];

        let outcome = router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert!(outcome.routed.is_consumed());
        assert_eq!(router.trapped_focus_node(), Some(NodeId::from_raw(10, 0)));
    }

    #[test]
    fn tab_never_focuses_a_covered_lower_dialogs_button_with_overlapping_dialogs() {
        // Review P2 exactly: node 5 belongs to the *bottom* (now covered)
        // dialog, and its hit rect's center happens to fall inside the
        // *top* dialog's panel rect. The original bug derived "in this
        // dialog's focus trap" from that geometric overlap, so node 5
        // would enter the topmost dialog's Tab cycle even though it isn't
        // one of its own controls. Scoping to the topmost dialog's
        // explicit `owned` set fixes this: Tab must only ever reach node
        // 10, never node 5.
        let mut router = InputRouter::new();
        let bottom_rect = Rect::new(Point::new(0.0, 0.0), Size::new(600.0, 600.0));
        let top_rect = dialog_panel_rect(); // (200,200)-(500,400)
        router.push_modal(
            NodeId::from_raw(1, 0),
            bottom_rect,
            vec![NodeId::from_raw(5, 0)],
        );
        router.push_modal(
            NodeId::from_raw(2, 0),
            top_rect,
            vec![NodeId::from_raw(10, 0)],
        );

        let entries = vec![
            // Bottom dialog's own button — its center (300,310) lies
            // inside the top dialog's panel rect.
            button_at(
                5,
                Rect::new(Point::new(280.0, 300.0), Size::new(40.0, 20.0)),
            ),
            button_at(
                10,
                Rect::new(Point::new(210.0, 210.0), Size::new(40.0, 20.0)),
            ),
        ];

        for _ in 0..3 {
            router.route_key_with_hits(
                PhysicalKey::Code(KeyCode::Tab),
                ElementState::Pressed,
                false,
                &entries,
            );
            assert_eq!(
                router.trapped_focus_node(),
                Some(NodeId::from_raw(10, 0)),
                "the covered dialog's button must never enter the topmost dialog's cycle"
            );
        }
    }

    #[test]
    fn shift_tab_cycles_backward_and_wraps_to_the_last_control() {
        let mut router = InputRouter::new();
        router.push_modal(
            NodeId::from_raw(1, 0),
            dialog_panel_rect(),
            vec![NodeId::from_raw(10, 0), NodeId::from_raw(11, 0)],
        );
        let entries = vec![
            button_at(
                10,
                Rect::new(Point::new(210.0, 210.0), Size::new(40.0, 20.0)),
            ),
            button_at(
                11,
                Rect::new(Point::new(260.0, 210.0), Size::new(40.0, 20.0)),
            ),
        ];
        router.route(&modifiers_changed(true), &[], 1.0);

        router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert_eq!(
            router.trapped_focus_node(),
            Some(NodeId::from_raw(11, 0)),
            "Shift+Tab from nothing focused lands on the last control"
        );

        router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert_eq!(router.trapped_focus_node(), Some(NodeId::from_raw(10, 0)));
    }

    #[test]
    fn tab_repeat_does_not_advance_the_focus_trap_twice() {
        let mut router = InputRouter::new();
        router.push_modal(
            NodeId::from_raw(1, 0),
            dialog_panel_rect(),
            vec![NodeId::from_raw(10, 0), NodeId::from_raw(11, 0)],
        );
        let entries = vec![
            button_at(
                10,
                Rect::new(Point::new(210.0, 210.0), Size::new(40.0, 20.0)),
            ),
            button_at(
                11,
                Rect::new(Point::new(260.0, 210.0), Size::new(40.0, 20.0)),
            ),
        ];

        router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert_eq!(router.trapped_focus_node(), Some(NodeId::from_raw(10, 0)));

        for _ in 0..3 {
            let repeat = router.route_key_with_hits(
                PhysicalKey::Code(KeyCode::Tab),
                ElementState::Pressed,
                true,
                &entries,
            );
            assert!(repeat.routed.is_consumed());
        }
        assert_eq!(
            router.trapped_focus_node(),
            Some(NodeId::from_raw(10, 0)),
            "OS key-repeats must not advance the trap further"
        );
    }

    #[test]
    fn tab_with_no_focusable_controls_still_traps_the_key() {
        let mut router = InputRouter::new();
        router.push_modal(NodeId::from_raw(1, 0), dialog_panel_rect(), Vec::new());

        let outcome = router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &[],
        );
        assert!(
            outcome.routed.is_consumed(),
            "Tab must not leak past an open dialog even with nothing to focus yet"
        );
        assert!(router.trapped_focus_node().is_none());
    }

    #[test]
    fn tab_falls_through_to_the_host_with_no_modal_open() {
        let mut router = InputRouter::new();
        let entries = vec![button_at(
            10,
            Rect::new(Point::new(210.0, 210.0), Size::new(40.0, 20.0)),
        )];

        let outcome = router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert!(
            outcome.routed.is_ignored(),
            "with nothing open, Tab is the host's own focus-traversal key"
        );
    }

    #[test]
    fn escape_still_closes_a_dialog_opened_after_tab_cycled_its_focus() {
        // Regression guard: Tab-cycling the trap must not leave state that
        // confuses the (independently-tested) Escape/backdrop paths.
        let mut router = InputRouter::new();
        router.push_modal(
            NodeId::from_raw(1, 0),
            dialog_panel_rect(),
            vec![NodeId::from_raw(10, 0)],
        );
        let entries = vec![button_at(
            10,
            Rect::new(Point::new(210.0, 210.0), Size::new(40.0, 20.0)),
        )];
        router.route_key_with_hits(
            PhysicalKey::Code(KeyCode::Tab),
            ElementState::Pressed,
            false,
            &entries,
        );
        assert!(router.trapped_focus_node().is_some());

        let escape = router.route_key(
            PhysicalKey::Code(KeyCode::Escape),
            ElementState::Pressed,
            false,
        );
        assert!(escape.routed.is_consumed());
        assert!(router.top_modal().is_none());
        assert!(
            router.trapped_focus_node().is_none(),
            "closing the dialog clears its focus-trap state"
        );
    }
}
